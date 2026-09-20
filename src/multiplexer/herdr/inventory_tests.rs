//! Inventory race contracts. Only the isolated test uses a live Herdr server.
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;

struct InventoryServer {
    _root: tempfile::TempDir,
    backend: HerdrBackend,
    stop: mpsc::Sender<()>,
    worker: thread::JoinHandle<Vec<Value>>,
}

impl InventoryServer {
    fn new(mut respond: impl FnMut(&Value) -> Value + Send + 'static) -> Self {
        let root = tempfile::tempdir().unwrap();
        let endpoint = root.path().join("api.sock");
        let listener = UnixListener::bind(&endpoint).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
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
                        let mut response = respond(&request);
                        response["id"] = request["id"].clone();
                        writeln!(stream, "{response}").unwrap();
                        requests.push(request);
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
            requests
        });
        Self {
            backend: HerdrBackend::for_socket(endpoint.to_str().unwrap()),
            _root: root,
            stop,
            worker,
        }
    }

    fn finish(self) -> Vec<Value> {
        self.stop.send(()).unwrap();
        self.worker.join().unwrap()
    }
}

#[test]
#[ignore = "requires inventory_checks.py --adapter-test-binary on a disposable server"]
fn isolated_simultaneous_same_name_creation() -> Result<()> {
    // Require the fixture's endpoint; never use the inherited main server.
    let endpoint = std::env::var("WORKMUX_HERDR_INVENTORY_SOCKET")?;
    let cwd = std::env::current_dir()?;
    ensure!(
        std::fs::canonicalize(&endpoint)?.starts_with(&cwd),
        "Inventory socket must be inside the disposable fixture directory"
    );
    let backend = HerdrBackend::for_socket(&endpoint);
    let before = backend.client.snapshot()?;
    let barrier = std::sync::Barrier::new(2);
    let keys = thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = &barrier;
                let endpoint = &endpoint;
                let cwd = &cwd;
                scope.spawn(move || -> Result<String> {
                    let backend = HerdrBackend::for_socket(endpoint);
                    barrier.wait();
                    let key = backend.create_window_in_session(CreateWindowInSessionParams {
                        session_name: "parent",
                        name: Some("same-window"),
                        cwd,
                    })?;
                    backend.finish_pane_setup(std::slice::from_ref(&key))?;
                    Ok(key)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Result<Vec<_>>>()
    })?;
    assert_ne!(keys[0], keys[1]);
    let after = backend.client.snapshot()?;
    let tabs: Vec<_> = after
        .tabs
        .iter()
        .filter(|tab| tab.label == "same-window")
        .collect();
    assert_eq!(tabs.len(), 2);
    assert_ne!(tabs[0].tab_id, tabs[1].tab_id);
    assert!(
        backend
            .tab(&WindowTarget::new(
                "same-window".into(),
                Some("parent".into())
            ))
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    for key in &keys {
        let pane = backend.pane(key)?;
        assert!(tabs.iter().any(|tab| tab.tab_id == pane.tab_id));
    }
    assert_eq!(after.tabs.len(), before.tabs.len() + 2);
    assert_eq!(after.focused_tab_id, before.focused_tab_id);

    let results = thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = &barrier;
                let endpoint = &endpoint;
                let cwd = &cwd;
                scope.spawn(move || -> Result<String> {
                    let backend = HerdrBackend::for_socket(endpoint);
                    barrier.wait();
                    let key = backend.create_session(CreateSessionParams {
                        prefix: "",
                        name: "same-session",
                        cwd,
                        initial_window_name: None,
                    })?;
                    backend.finish_pane_setup(std::slice::from_ref(&key))?;
                    Ok(key)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut created = std::collections::HashSet::new();
    for result in results {
        match result {
            Ok(key) => {
                assert!(created.insert(backend.pane(&key)?.workspace_id));
            }
            Err(error) => assert!(format!("{error:#}").contains("already exists"), "{error:#}"),
        }
    }
    assert!(!created.is_empty());
    let after = backend.client.snapshot()?;
    let actual: std::collections::HashSet<_> = after
        .workspaces
        .iter()
        .filter(|workspace| workspace.label == "same-session")
        .map(|workspace| workspace.workspace_id.clone())
        .collect();
    assert_eq!(actual, created);
    if created.len() == 2 {
        assert!(
            backend
                .session("same-session")
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    } else {
        assert!(created.contains(&backend.session("same-session")?.workspace_id));
    }
    assert_eq!(after.focused_tab_id, before.focused_tab_id);
    println!(
        "HERDR_INVENTORY_CONCURRENT_PASSED sessions={}",
        created.len()
    );
    Ok(())
}

fn inventory(tabs: Value) -> Value {
    json!({"result":{"snapshot":{"version":"0.9.0", "protocol":22,
        "workspaces":[{"workspace_id":"original-parent", "label":"parent"}],
        "tabs":tabs, "panes":[]}}})
}

#[test]
fn parent_removed_after_lookup_does_not_retry_or_allocate_in_a_replacement() {
    let server = InventoryServer::new(|request| match request["method"].as_str().unwrap() {
        "session.snapshot" => inventory(json!([])),
        // The parent existed at lookup but was removed before layout.apply.
        // A same-label replacement must not receive a retried allocation.
        "layout.apply" => json!({"error":{"code":"workspace_not_found"}}),
        _ => json!({"error":{"code":"unexpected_request"}}),
    });
    let error = server
        .backend
        .create_window_in_session(CreateWindowInSessionParams {
            session_name: "parent",
            name: Some("new-window"),
            cwd: Path::new("/tmp"),
        })
        .unwrap_err();
    assert!(format!("{error:#}").contains("workspace_not_found"));
    let requests = server.finish();
    assert_eq!(
        requests.len(),
        2,
        "no retry, focus, cleanup, or fallback mutation"
    );
    assert_eq!(requests[0]["method"], "session.snapshot");
    assert_eq!(requests[1]["method"], "layout.apply");
    assert_eq!(requests[1]["params"]["workspace_id"], "original-parent");
    assert_eq!(requests[1]["params"]["tab_label"], "new-window");
    assert_eq!(requests[1]["params"]["focus"], false);
    assert!(requests[1]["params"].get("tab_id").is_none());
}

#[test]
fn removed_placement_target_is_not_replaced_by_its_label() {
    let mut snapshots = 0;
    let server = InventoryServer::new(move |request| {
        assert_eq!(
            request["method"], "session.snapshot",
            "must not allocate or move a tab"
        );
        snapshots += 1;
        let id = if snapshots == 1 {
            "original-tab"
        } else {
            "replacement-tab"
        };
        inventory(json!([{"workspace_id":"original-parent", "tab_id":id, "label":"anchor"}]))
    });
    let original = server.backend.client.snapshot().unwrap();
    let key = server.backend.key(&original.tabs[0].tab_id).unwrap();
    let error = server
        .backend
        .create_tab_in_workspace(
            "original-parent",
            CreateWindowParams {
                prefix: "wm-",
                name: "new-window",
                cwd: Path::new("/tmp"),
                after_window: Some(&key),
            },
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("Invalid Herdr placement target"));
    assert_eq!(server.finish().len(), 2);
}
