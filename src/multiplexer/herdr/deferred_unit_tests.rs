use super::client::Client;
use super::deferred::{Action, Operation, run};
use super::identity::ProcessIdentity;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::thread;
use std::time::Duration;

fn snapshot(panes: Value, exists: bool) -> Value {
    json!({"snapshot": {
        "version": "0.9.0", "protocol": 22,
        "workspaces": if exists { json!([{"workspace_id":"workspace", "label":"owned"}]) } else { json!([]) },
        "tabs": if exists { json!([{"tab_id":"tab", "workspace_id":"workspace", "label":"owned"}]) } else { json!([]) },
        "panes": panes,
    }})
}

fn pane(id: &str, terminal: &str) -> Value {
    json!({"pane_id":id, "terminal_id":terminal, "tab_id":"tab",
        "workspace_id":"workspace", "focused":false, "cwd":"/"})
}

// The first response lets the scheduler capture the real socket/peer lifetime.
// Remaining responses describe the state seen by the deferred worker.
fn exercise(
    action: Action,
    responses: Vec<Value>,
    stale_boot: bool,
    stale_shell: bool,
) -> (anyhow::Result<()>, Vec<Value>) {
    exercise_with_runner(action, responses, stale_boot, stale_shell, |operation| {
        run(&serde_json::to_string(operation).unwrap())
    })
}

fn exercise_with_runner(
    action: Action,
    responses: Vec<Value>,
    stale_boot: bool,
    stale_shell: bool,
    runner: impl FnOnce(&Operation) -> anyhow::Result<()>,
) -> (anyhow::Result<()>, Vec<Value>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("api.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for response in std::iter::once(snapshot(json!([]), true)).chain(responses) {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "Missing worker request"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            // Darwin can reject timeout changes after the peer has closed.
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            if request.is_empty() {
                requests.push(Value::Null);
                continue;
            }
            requests.push(serde_json::from_str(&request).unwrap());
            writeln!(stream, "{}", json!({"id":"workmux", "result":response})).unwrap();
        }
        requests
    });
    let endpoint = path.to_str().unwrap().to_string();
    let boot = Client::new(endpoint.clone()).boot().unwrap();
    let mut shell = ProcessIdentity::read(std::process::id()).unwrap();
    if stale_shell {
        shell.start = "stale-shell".into();
    }
    let operation = Operation {
        endpoint,
        boot: if stale_boot {
            "stale-server".into()
        } else {
            boot
        },
        action,
        target: if action.workspace() {
            "workspace"
        } else {
            "tab"
        }
        .into(),
        terminals: [("terminal".into(), shell)].into(),
    };
    let result = runner(&operation);
    (result, server.join().unwrap())
}

// Invoked as a separate test process by deferred_worker_outlives_scheduler.
#[test]
fn deferred_scheduler_process() {
    if let Ok(script) = std::env::var("WORKMUX_HERDR_TEST_DEFERRED_SCRIPT") {
        super::util::run_detached_sh_c(&script).unwrap();
    }
}

#[test]
fn deferred_worker_outlives_scheduler() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("scheduler-exited");
    let (result, requests) = exercise_with_runner(
        Action::CloseTab,
        vec![
            snapshot(json!([pane("owned", "terminal")]), true),
            json!({"process_info":{"shell_pid":std::process::id()}}),
            snapshot(json!([pane("owned", "terminal")]), true),
            json!({}),
        ],
        false,
        false,
        |operation| {
            // A marker holds the worker until the scheduling process has exited.
            let script = format!(
                "n=0; while [ ! -f {} ]; do [ \"$n\" -lt 500 ] || exit 1; n=$((n + 1)); /bin/sleep 0.01; done; {}",
                super::agent::shell_quote(marker.to_str().unwrap()),
                operation.command()?,
            );
            let output = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "multiplexer::herdr::deferred_unit_tests::deferred_scheduler_process",
                ])
                .env("WORKMUX_HERDR_TEST_DEFERRED_SCRIPT", script)
                .output()?;
            anyhow::ensure!(output.status.success(), "Scheduler failed: {output:?}");
            std::fs::write(&marker, "exited")?;
            Ok(())
        },
    );
    result.unwrap();
    assert_eq!(requests.last().unwrap()["method"], "tab.close");
}

#[test]
fn deferred_version_compatibility() {
    for (version, accepted) in [
        ("0.8.99", false),
        ("0.9.0", true),
        ("0.9.1", true),
        ("0.10.0", true),
        ("1.0.0", true),
        ("invalid", false),
        ("0.9", false),
        ("0.9.0-rc.1", false),
    ] {
        for protocol in [21, 22, 23] {
            let accepted = accepted && protocol == 22;
            let mut state = snapshot(json!([]), true);
            state["snapshot"]["version"] = json!(version);
            state["snapshot"]["protocol"] = json!(protocol);
            let mut responses = vec![state];
            if accepted {
                responses.push(json!({}));
            }
            let (result, requests) = exercise(Action::FocusTab, responses, false, false);
            assert_eq!(
                result.is_ok(),
                accepted,
                "version {version}, protocol {protocol}"
            );
            assert_eq!(requests.len(), if accepted { 3 } else { 2 });
        }
    }
}

#[test]
fn deferred_focus_uses_captured_target() {
    for action in [Action::FocusTab, Action::FocusWorkspace] {
        let (result, requests) = exercise(
            action,
            vec![snapshot(json!([]), true), json!({})],
            false,
            false,
        );
        result.unwrap();
        let (method, key, target) = if action.workspace() {
            ("workspace.focus", "workspace_id", "workspace")
        } else {
            ("tab.focus", "tab_id", "tab")
        };
        assert_eq!(requests[2]["method"], method);
        assert_eq!(requests[2]["params"][key], target);
    }
}

#[test]
fn deferred_close_includes_extra_panes_and_closes_container() {
    for action in [Action::CloseTab, Action::CloseWorkspace] {
        let initial = snapshot(
            json!([pane("old", "terminal"), pane("extra", "extra")]),
            true,
        );
        let process = json!({"process_info":{"shell_pid":std::process::id()}});
        let (result, requests) = exercise(
            action,
            vec![
                initial.clone(),
                process,
                snapshot(
                    json!([pane("moved", "terminal"), pane("later", "later")]),
                    true,
                ),
                json!({}),
            ],
            false,
            false,
        );
        result.unwrap();
        let closes: Vec<_> = requests
            .iter()
            .filter(|r| r["method"].as_str().unwrap().ends_with(".close"))
            .collect();
        assert_eq!(closes.len(), 1);
        let (method, key, target) = if action.workspace() {
            ("workspace.close", "workspace_id", "workspace")
        } else {
            ("tab.close", "tab_id", "tab")
        };
        assert_eq!(closes[0]["method"], method);
        assert_eq!(closes[0]["params"][key], target);
    }
}

#[test]
fn deferred_close_rejects_missing_owned_terminal_and_stale_shell() {
    let initial = snapshot(json!([pane("owned", "terminal")]), true);
    let mut moved = pane("moved", "terminal");
    moved["tab_id"] = json!("another-tab");
    moved["workspace_id"] = json!("another-workspace");
    let foreign = snapshot(json!([moved, pane("foreign", "foreign")]), true);
    let process = json!({"process_info":{"shell_pid":std::process::id()}});
    for (responses, stale_shell, expected) in [
        (vec![foreign.clone()], false, "contents changed"),
        (
            vec![initial.clone(), process.clone(), foreign],
            false,
            "contents changed",
        ),
        (vec![initial.clone(), process], true, "process changed"),
        (
            vec![initial, json!({"process_info":{"shell_pid":1}})],
            false,
            "process changed",
        ),
    ] {
        let (result, requests) = exercise(Action::CloseTab, responses, false, stale_shell);
        assert!(result.unwrap_err().to_string().contains(expected));
        assert!(
            requests
                .iter()
                .all(|r| !r["method"].as_str().unwrap().ends_with(".close"))
        );
    }
}

#[test]
fn deferred_worker_rejects_stale_server_before_sending_bytes() {
    let (result, requests) = exercise(Action::CloseTab, vec![json!({})], true, false);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("server lifetime changed")
    );
    assert_eq!(requests[1], Value::Null);
}

#[test]
fn deferred_worker_rejects_missing_target_and_malformed_snapshot() {
    let mut malformed = snapshot(json!([]), true);
    malformed["snapshot"]["panes"] = json!(null);
    for (response, expected) in [
        (snapshot(json!([]), false), "target no longer exists"),
        (malformed, "Malformed Herdr snapshot"),
    ] {
        let (result, requests) = exercise(Action::FocusTab, vec![response], false, false);
        assert!(result.unwrap_err().to_string().contains(expected));
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["method"], "session.snapshot");
    }
}

#[test]
fn deferred_worker_rejects_invalid_payload_and_protocol() {
    for payload in ["{}", "null", r#"{"action":"close-everything"}"#] {
        assert!(run(payload).is_err());
    }
    let mut invalid = snapshot(json!([]), true);
    invalid["snapshot"]["protocol"] = json!(21);
    let (result, requests) = exercise(Action::FocusTab, vec![invalid], false, false);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Unsupported Herdr server")
    );
    assert_eq!(requests.len(), 2);
}
