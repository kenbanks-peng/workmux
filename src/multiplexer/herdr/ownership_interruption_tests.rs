//! Controlled protocol/storage fault tests, not live-terminal acceptance tests.
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
struct State {
    requests: Vec<Value>,
    drop_method: Option<&'static str>,
    apply_close_before_drop: bool,
    removed: bool,
    replaced: bool,
}

struct Server {
    _directory: tempfile::TempDir,
    endpoint: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new() -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("api.sock");
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = state.clone();
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream.set_nonblocking(false).unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                if line.is_empty() {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap();
                let mut state = shared.lock().unwrap();
                state.requests.push(request.clone());
                let dropped = state.drop_method == Some(method);
                if dropped {
                    state.drop_method = None;
                }
                let result = match method {
                    "session.snapshot" => snapshot(&state),
                    "pane.process_info" => json!({"process_info":{"shell_pid":std::process::id()}}),
                    "tab.close" | "workspace.close" => {
                        let key = if method == "tab.close" {
                            "tab_id"
                        } else {
                            "workspace_id"
                        };
                        // Fail the test if cleanup ever targets the unrelated container.
                        assert_eq!(request["params"][key], "owned");
                        assert!(!state.replaced, "cleanup removed a replacement target");
                        if !dropped || state.apply_close_before_drop {
                            state.removed = true;
                        }
                        json!({})
                    }
                    _ => panic!("Unexpected request: {request}"),
                };
                if !dropped {
                    writeln!(stream, "{}", json!({"id":"workmux", "result":result})).unwrap();
                }
            }
        });
        Ok(Self {
            _directory: directory,
            endpoint: path.to_str().unwrap().into(),
            state,
            stop,
            thread: Some(thread),
        })
    }

    fn fault(&self, method: &'static str, apply: bool) {
        let mut state = self.state.lock().unwrap();
        state.drop_method = Some(method);
        state.apply_close_before_drop = apply;
    }

    fn closes(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r["method"].as_str().unwrap().ends_with(".close"))
            .count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn snapshot(state: &State) -> Value {
    let mut workspaces = vec![json!({"workspace_id":"foreign", "label":"unrelated"})];
    let mut tabs = vec![json!({"tab_id":"foreign", "workspace_id":"foreign", "label":"unrelated"})];
    let mut panes = vec![
        json!({"pane_id":"foreign-pane", "terminal_id":"foreign-terminal", "tab_id":"foreign", "workspace_id":"foreign", "cwd":"/", "focused":false}),
    ];
    if !state.removed {
        workspaces.push(json!({"workspace_id":"owned", "label":"owned"}));
        tabs.push(json!({"tab_id":"owned", "workspace_id":"owned", "label":"owned"}));
        panes.push(json!({"pane_id":"owned-pane", "terminal_id":if state.replaced { "replacement-terminal" } else { "owned-terminal" }, "tab_id":"owned", "workspace_id":"owned", "cwd":"/", "focused":false}));
    }
    json!({"snapshot":{"version":"0.9.0", "protocol":22, "workspaces":workspaces, "tabs":tabs, "panes":panes}})
}

fn isolated(name: &str, test: impl FnOnce() -> Result<()>) -> Result<()> {
    let name = format!("multiplexer::herdr::ownership_interruption_tests::{name}");
    if !crate::test_support::is_isolated_child(&name) {
        let root = tempfile::tempdir()?;
        crate::test_support::run_isolated_test(
            &name,
            root.path(),
            &[("XDG_STATE_HOME", root.path())],
        );
        return Ok(());
    }
    test()?;
    println!("{}", crate::test_support::ISOLATED_TEST_CANARY);
    Ok(())
}

fn owned_backend(server: &Server) -> Result<HerdrBackend> {
    let backend = HerdrBackend::for_socket(&server.endpoint);
    backend.own_tab("owned", "owned-terminal")?;
    // The workspace marker uses the same ID in this fixture. Only workspace
    // tests replace the tab cache with this marker; live terminal records are authoritative.
    Ok(backend)
}

fn command(backend: &HerdrBackend, workspace: bool) -> Result<String> {
    if workspace {
        std::fs::write(backend.record_path("owned")?, backend.client.boot()?)?;
    }
    backend.deferred_command(
        if workspace {
            deferred::Action::CloseWorkspace
        } else {
            deferred::Action::CloseTab
        },
        &backend.key("owned")?,
    )
}

fn execute(command: &str) -> Result<std::process::Output> {
    Ok(std::process::Command::new("/bin/sh")
        .args(["-c", command])
        .output()?)
}

fn assert_unrelated_survives(backend: &HerdrBackend) -> Result<()> {
    let snapshot = backend.client.snapshot()?;
    assert!(
        snapshot
            .workspaces
            .iter()
            .any(|w| w.workspace_id == "foreign")
    );
    assert!(snapshot.tabs.iter().any(|t| t.tab_id == "foreign"));
    assert!(
        snapshot
            .panes
            .iter()
            .any(|p| p.terminal_id == "foreign-terminal")
    );
    Ok(())
}

#[test]
fn ownership_record_commit_failure_allows_same_owner_retry() -> Result<()> {
    isolated(
        "ownership_record_commit_failure_allows_same_owner_retry",
        || {
            let server = Server::new()?;
            let backend = owned_backend(&server)?;
            let key = backend.key("owned-terminal")?;
            let tab_path = backend.record_path("owned")?;
            // Interrupt the final atomic persist, after terminal ownership was saved.
            std::fs::remove_file(&tab_path)?;
            std::fs::create_dir(&tab_path)?;
            assert!(backend.set_window_ownership(&key, "owner", true).is_err());
            let terminal = backend.terminal_record("owned-terminal")?.unwrap();
            assert_eq!(terminal.token.as_deref(), Some("owner"));
            assert!(terminal.primary);
            assert!(
                backend
                    .set_window_ownership(&key, "other-owner", true)
                    .is_err()
            );
            assert_eq!(server.closes(), 0);
            assert_unrelated_survives(&backend)?;
            std::fs::remove_dir(&tab_path)?;
            backend.set_window_ownership(&key, "owner", true)?;
            assert_eq!(
                backend.record("owned")?.unwrap().token.as_deref(),
                Some("owner")
            );
            assert!(execute(&command(&backend, false)?)?.status.success());
            assert!(server.state.lock().unwrap().removed);
            assert_unrelated_survives(&backend)
        },
    )
}

#[test]
fn cleanup_capture_disconnect_allows_safe_retry() -> Result<()> {
    isolated("cleanup_capture_disconnect_allows_safe_retry", || {
        for workspace in [false, true] {
            let server = Server::new()?;
            let backend = owned_backend(&server)?;
            server.fault("pane.process_info", false);
            assert!(command(&backend, workspace).is_err());
            assert_eq!(server.closes(), 0);
            assert_unrelated_survives(&backend)?;
            assert!(execute(&command(&backend, workspace)?)?.status.success());
            assert_eq!(server.closes(), 1);
            assert!(server.state.lock().unwrap().removed);
            assert_unrelated_survives(&backend)?;
        }
        Ok(())
    })
}

#[test]
fn interrupted_target_removal_retries_without_touching_unrelated_targets() -> Result<()> {
    isolated(
        "interrupted_target_removal_retries_without_touching_unrelated_targets",
        || {
            for workspace in [false, true] {
                for applied in [false, true] {
                    let server = Server::new()?;
                    let backend = owned_backend(&server)?;
                    let command = command(&backend, workspace)?;
                    server.fault(
                        if workspace {
                            "workspace.close"
                        } else {
                            "tab.close"
                        },
                        applied,
                    );
                    let first = execute(&command)?;
                    assert!(!first.status.success());
                    assert!(
                        String::from_utf8_lossy(&first.stderr)
                            .contains("Incomplete Herdr response")
                    );
                    assert_eq!(server.state.lock().unwrap().removed, applied);
                    assert_unrelated_survives(&backend)?;
                    let retry = execute(&command)?;
                    if applied {
                        assert!(!retry.status.success());
                        assert!(
                            String::from_utf8_lossy(&retry.stderr)
                                .contains("target no longer exists")
                        );
                        assert_eq!(server.closes(), 1);
                    } else {
                        assert!(retry.status.success(), "{retry:?}");
                        assert_eq!(server.closes(), 2);
                    }
                    assert!(server.state.lock().unwrap().removed);
                    assert_unrelated_survives(&backend)?;
                }
            }
            Ok(())
        },
    )
}

#[test]
fn interrupted_close_retry_rejects_replacement_target() -> Result<()> {
    isolated("interrupted_close_retry_rejects_replacement_target", || {
        for workspace in [false, true] {
            let server = Server::new()?;
            let backend = owned_backend(&server)?;
            let command = command(&backend, workspace)?;
            server.fault(
                if workspace {
                    "workspace.close"
                } else {
                    "tab.close"
                },
                true,
            );
            assert!(!execute(&command)?.status.success());
            {
                let mut state = server.state.lock().unwrap();
                state.removed = false;
                state.replaced = true;
            }
            let retry = execute(&command)?;
            assert!(!retry.status.success());
            assert!(String::from_utf8_lossy(&retry.stderr).contains("contents changed"));
            assert_eq!(server.closes(), 1);
            assert!(
                backend
                    .client
                    .snapshot()?
                    .panes
                    .iter()
                    .any(|p| p.terminal_id == "replacement-terminal")
            );
            assert_unrelated_survives(&backend)?;
        }
        Ok(())
    })
}
