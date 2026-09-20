//! Adapter contract tests. These do not establish real-server acceptance.
#[path = "capture_contract_tests.rs"]
mod capture_contract_tests;
#[path = "process_metadata_tests.rs"]
mod process_metadata_tests;

use super::*;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;

pub(super) struct Probe {
    _directory: tempfile::TempDir,
    pub(super) backend: HerdrBackend,
    stop: mpsc::Sender<()>,
    server: thread::JoinHandle<Vec<Value>>,
}

impl Probe {
    pub(super) fn new(steps: Vec<(&str, Value, Option<Value>)>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("api.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (stop, stopped) = mpsc::channel();
        let mut steps: VecDeque<_> = steps
            .into_iter()
            .map(|(method, params, response)| (method.to_string(), params, response))
            .collect();
        let server = thread::spawn(move || {
            let mut unexpected = Vec::new();
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut line = String::new();
                        BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        let request: Value = serde_json::from_str(&line).unwrap();
                        let response = if let Some((method, params, response)) = steps.pop_front() {
                            assert_eq!(request["method"], method);
                            assert_eq!(request["params"], params);
                            response
                        } else {
                            unexpected.push(request);
                            Some(json!({"error":{"code":"unexpected_request"}}))
                        };
                        // None simulates loss of the response after receipt of the request.
                        if let Some(mut response) = response {
                            response["id"] = json!("workmux");
                            writeln!(stream, "{response}").unwrap();
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if !matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("{error}"),
                }
            }
            assert!(steps.is_empty(), "expected requests were not sent");
            unexpected
        });
        Self {
            backend: HerdrBackend::for_socket(path.to_str().unwrap()),
            _directory: directory,
            stop,
            server,
        }
    }

    pub(super) fn finish(self) {
        self.stop.send(()).unwrap();
        assert!(
            self.server.join().unwrap().is_empty(),
            "unexpected request or retry"
        );
    }
}

pub(super) fn snapshot() -> Value {
    json!({"version":"0.9.0", "protocol":22,
        "workspaces":[{"workspace_id":"w1","label":"session"}],
        "tabs":[{"workspace_id":"w1","tab_id":"t1","label":"window"}],
        "panes":[{"workspace_id":"w1","tab_id":"t1","pane_id":"p1",
            "terminal_id":"term1","focused":false,"cwd":"/old"}]})
}

pub(super) fn snap(value: Value) -> (&'static str, Value, Option<Value>) {
    (
        "session.snapshot",
        json!({}),
        Some(json!({"result":{"snapshot":value}})),
    )
}

#[test]
fn lost_mutation_response_is_not_retried() {
    let probe = Probe::new(vec![
        snap(snapshot()),
        ("workspace.create", json!({"label":"once"}), None),
    ]);
    assert!(
        probe
            .backend
            .client
            .request("workspace.create", json!({"label":"once"}))
            .is_err()
    );
    probe.finish();
}

#[test]
fn pane_labels_preserve_empty_and_long_values_and_report_rejection() {
    for label in [String::new(), "界".repeat(4096)] {
        for rejected in [false, true] {
            let response = if rejected {
                json!({"error":{"code":"invalid_label"}})
            } else {
                json!({"result":{}})
            };
            let probe = Probe::new(vec![
                snap(snapshot()),
                snap(snapshot()),
                (
                    "pane.rename",
                    json!({"pane_id":"p1","label":label}),
                    Some(response),
                ),
            ]);
            let key = probe.backend.key("term1").unwrap();
            let result = probe.backend.set_pane_name(&key, &label);
            if rejected {
                assert!(result.unwrap_err().to_string().contains("invalid_label"));
            } else {
                result.unwrap();
            }
            probe.finish();
        }
    }
}

#[test]
fn key_aliases_and_control_keys_use_native_encoding() {
    for (input, encoded) in [
        (" ", "space"),
        ("BSpace", "backspace"),
        ("C-c", "ctrl+c"),
        ("C-d", "ctrl+d"),
        ("C-z", "ctrl+z"),
        ("enter", "enter"),
        ("Escape", "Escape"),
        ("Tab", "Tab"),
        ("Up", "Up"),
    ] {
        let probe = Probe::new(vec![
            snap(snapshot()),
            snap(snapshot()),
            (
                "pane.send_keys",
                json!({"pane_id":"p1","keys":[encoded]}),
                Some(json!({"result":{}})),
            ),
        ]);
        let key = probe.backend.key("term1").unwrap();
        probe.backend.send_key(&key, input).unwrap();
        probe.finish();
    }
}

#[test]
fn capture_zero_empty_unicode_and_recent_limit() {
    for (lines, text, expected) in [
        (0, "", ""),
        (1, "", ""),
        (1, "old\n界🙂", "界🙂"),
        (
            2,
            "old\n\u{1b}[31mred\u{1b}[0m\nlast",
            "\u{1b}[31mred\u{1b}[0m\nlast",
        ),
        (976, "boundary", "boundary"),
    ] {
        let mut steps = vec![snap(snapshot()), snap(snapshot())];
        if lines > 0 {
            steps.extend([snap(snapshot()),
                ("pane.layout", json!({"pane_id":"p1"}), Some(json!({"result":{"layout":{"panes":[{"pane_id":"p1","rect":{"width":80,"height":24}}]}}}))),
                ("pane.read", json!({"pane_id":"p1","source":"recent","lines":u32::from(lines)+24}), Some(json!({"result":{"read":{"text":text}}})))]);
        }
        let probe = Probe::new(steps);
        let key = probe.backend.key("term1").unwrap();
        assert_eq!(
            probe.backend.capture_pane(&key, lines).as_deref(),
            Some(expected)
        );
        probe.finish();
    }
}

#[test]
fn live_metadata_refreshes_and_handles_disappearance_during_discovery() {
    let mut changed = snapshot();
    changed["panes"][0]["foreground_cwd"] = json!("/new");
    changed["panes"][0]["title"] = json!("replacement");
    let mut gone = snapshot();
    gone["panes"] = json!([]);
    let process =
        |pid| Some(json!({"result":{"process_info":{"shell_pid":pid,"foreground_processes":[]}}}));
    let probe = Probe::new(vec![
        snap(snapshot()),
        snap(snapshot()),
        ("pane.process_info", json!({"pane_id":"p1"}), process(100)),
        snap(changed.clone()),
        ("pane.process_info", json!({"pane_id":"p1"}), process(200)),
        snap(changed),
        (
            "pane.process_info",
            json!({"pane_id":"p1"}),
            Some(json!({"error":{"code":"pane_not_found"}})),
        ),
        snap(gone),
    ]);
    let key = probe.backend.key("term1").unwrap();
    let first = probe.backend.get_live_pane_info(&key).unwrap().unwrap();
    assert_eq!(first.pid, Some(100));
    assert_eq!(first.working_dir, PathBuf::from("/old"));
    let second = probe.backend.get_live_pane_info(&key).unwrap().unwrap();
    assert_eq!(second.pid, Some(200));
    assert_eq!(second.working_dir, PathBuf::from("/new"));
    assert_eq!(second.title.as_deref(), Some("replacement"));
    assert!(
        probe
            .backend
            .get_live_pane_info(&key)
            .unwrap_err()
            .to_string()
            .contains("pane_not_found")
    );
    assert!(probe.backend.get_live_pane_info(&key).unwrap().is_none());
    probe.finish();
}
