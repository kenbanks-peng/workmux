//! Process-isolated selection checks. Controlled sockets are not live terminal evidence.
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const PROBE: &str = "multiplexer::herdr::remote_detection_tests::isolated_remote_selection";

struct Server {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new(path: PathBuf) -> Self {
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (done, count) = (stop.clone(), calls.clone());
        let worker = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut line = String::new();
                        BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        let request: Value = serde_json::from_str(&line).unwrap();
                        assert_eq!(request["method"], "session.snapshot");
                        count.fetch_add(1, Ordering::SeqCst);
                        writeln!(
                            stream,
                            "{}",
                            json!({"id":request["id"], "result":{"snapshot":{
                                "version":"0.9.0", "protocol":22,
                                "workspaces":[], "tabs":[], "panes":[]
                            }}})
                        )
                        .unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self {
            path,
            stop,
            calls,
            worker: Some(worker),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn controlled_case(case: &str) {
    // Short paths also work within the macOS Unix socket path limit.
    let root = tempfile::Builder::new()
        .prefix("wm-detect-")
        .tempdir_in("/tmp")
        .unwrap();
    let selected = Server::new(root.path().join("selected.sock"));
    let inherited = Server::new(root.path().join("inherited.sock"));
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", PROBE, "--ignored", "--nocapture"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.path())
        .env("WORKMUX_DETECTION_CASE", case)
        .env("WORKMUX_DETECTION_SELECTED", &selected.path)
        .env("WORKMUX_BACKEND", "tmux")
        .env("HERDR_SOCKET_PATH", &inherited.path)
        .env("TMUX", "/test-owned/stale-tmux,1,0")
        .env("WEZTERM_PANE", "42")
        .env("ZELLIJ", "1")
        .env("KITTY_WINDOW_ID", "43")
        .env("HERDR_ACTIVE_PANE_ID", "stale-inherited-pane")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("REMOTE_SELECTION_PASSED"));
    assert_eq!(
        inherited.calls.load(Ordering::SeqCst),
        0,
        "must not contact the inherited instance"
    );
    assert_eq!(selected.calls.load(Ordering::SeqCst) > 0, case != "missing");
}

#[test]
fn remote_environment_override_selects_only_requested_instance() {
    controlled_case("override");
}

#[test]
fn explicit_instance_ignores_conflicting_remote_environment() {
    controlled_case("explicit");
}

#[test]
fn missing_remote_instance_does_not_fall_back_to_inherited_instance() {
    controlled_case("missing");
}

/// Entry point shared by controlled subprocess tests and the private SSH runner.
#[test]
#[ignore = "requires test-owned endpoints; use integration/detection_checks.py"]
fn isolated_remote_selection() -> Result<()> {
    let case = std::env::var("WORKMUX_DETECTION_CASE")?;
    let selected = std::env::var("WORKMUX_DETECTION_SELECTED")?;
    if std::env::var_os("WORKMUX_DETECTION_REQUIRE_SSH").is_some() {
        ensure!(
            !std::env::var("SSH_CONNECTION")?.is_empty(),
            "not an SSH session"
        );
    }
    // Verify that the runner actually supplied the conflicting environment.
    assert_eq!(std::env::var("WORKMUX_BACKEND")?, "tmux");
    for name in [
        "TMUX",
        "WEZTERM_PANE",
        "ZELLIJ",
        "KITTY_WINDOW_ID",
        "HERDR_ACTIVE_PANE_ID",
    ] {
        ensure!(!std::env::var(name)?.is_empty(), "missing conflict: {name}");
    }
    assert_ne!(std::env::var("HERDR_SOCKET_PATH")?, selected);
    assert_eq!(detect_backend_strict()?, BackendType::Tmux);
    let endpoint = if case == "missing" {
        format!("{selected}.missing")
    } else {
        selected
    };
    let backend = match case.as_str() {
        "override" => {
            // This probe runs alone in a child process. No other test reads its environment.
            unsafe {
                std::env::set_var("WORKMUX_BACKEND", "herdr");
                std::env::set_var("HERDR_SOCKET_PATH", &endpoint);
            }
            assert_eq!(detect_backend_strict()?, BackendType::Herdr);
            assert_eq!(detect_backend(), BackendType::Herdr);
            create_backend(detect_backend_strict()?)
        }
        "explicit" | "missing" => create_backend_for_instance(BackendType::Herdr, &endpoint),
        _ => bail!("unknown selection case: {case}"),
    };
    if case == "missing" {
        assert!(backend.resolve_instance_id().is_err());
        assert!(backend.is_running().is_err());
    } else {
        assert_eq!(
            PathBuf::from(backend.resolve_instance_id()?),
            std::fs::canonicalize(endpoint)?
        );
        assert!(backend.is_running()?);
    }
    println!("REMOTE_SELECTION_PASSED {case}");
    Ok(())
}
