//! Process metadata contracts; only the explicitly ignored test uses real Herdr.
use super::*;

#[test]
fn successful_process_response_without_shell_pid_is_an_error() {
    for process in [
        json!({"foreground_processes": []}),
        json!({"shell_pid": null, "foreground_processes": []}),
    ] {
        let probe = Probe::new(vec![
            snap(snapshot()),
            snap(snapshot()),
            (
                "pane.process_info",
                json!({"pane_id":"p1"}),
                Some(json!({"result":{"process_info":process}})),
            ),
        ]);
        let key = probe.backend.key("term1").unwrap();
        let error = probe.backend.get_live_pane_info(&key).unwrap_err();
        assert!(
            error.to_string().contains("Missing Herdr shell PID"),
            "{error:#}"
        );
        probe.finish();
    }
}

#[test]
fn discovered_terminal_replaced_before_action_does_not_receive_input() {
    let mut replacement = snapshot();
    // A reused pane ID must not retarget a handle for an old terminal lifetime.
    replacement["panes"][0]["terminal_id"] = json!("term2");
    let probe = Probe::new(vec![
        snap(snapshot()),
        snap(snapshot()),
        (
            "pane.process_info",
            json!({"pane_id":"p1"}),
            Some(json!({"result":{"process_info":{"shell_pid":100}}})),
        ),
        snap(replacement),
    ]);
    let key = probe.backend.key("term1").unwrap();
    assert_eq!(
        probe.backend.get_live_pane_info(&key).unwrap().unwrap().pid,
        Some(100)
    );
    assert!(probe.backend.send_keys(&key, "must-not-run").is_err());
    probe.finish(); // No pane.send_text request is permitted.
}

#[test]
#[ignore = "requires a private Herdr instance; use integration/process_metadata_checks.py"]
fn isolated_process_exit_and_replacement() -> Result<()> {
    // No fallback to inherited HERDR_SOCKET_PATH is allowed.
    let endpoint = std::env::var("WORKMUX_HERDR_PROCESS_METADATA_SOCKET")?;
    let backend = HerdrBackend::for_socket(&endpoint);
    ensure!(
        backend.client.snapshot()?.tabs.is_empty(),
        "Expected a fresh private instance"
    );
    let cwd = std::env::current_dir()?;
    let create = |name| {
        backend.create_session(CreateSessionParams {
            prefix: "metadata-",
            name,
            cwd: &cwd,
            initial_window_name: Some("shell"),
        })
    };
    let wait_for_exit = |key: &str| -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match backend.get_live_pane_info(key) {
                Ok(None) => return Ok(()),
                Ok(Some(_)) => {}
                // Exit can occur between snapshot discovery and process_info.
                // The adapter reports this rejection, then returns None on a
                // fresh snapshot. Do not suppress unrelated server errors.
                Err(error) if error.to_string().contains("pane_not_found") => {}
                Err(error) => return Err(error),
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "Owned terminal did not disappear"
            );
            thread::sleep(Duration::from_millis(50));
        }
    };
    let old = create("old")?;
    let discovered = backend.get_live_pane_info(&old)?.unwrap();
    let identity = ProcessIdentity::read(discovered.pid.unwrap())?;
    let exit_file = cwd.join("exit-now");
    // The test owns this shell. A file gate makes exit occur after discovery,
    // without sending any termination request to another terminal.
    backend.send_keys(&old, "while [ ! -f exit-now ]; do sleep 0.05; done; exit")?;
    backend.send_key(&old, "Enter")?;
    std::fs::write(&exit_file, "exit")?;
    wait_for_exit(&old)?;
    assert!(!identity.is_live());
    assert!(backend.send_keys(&old, "touch stale-action").is_err());

    let replacement = create("replacement")?;
    let info = backend.get_live_pane_info(&replacement)?.unwrap();
    assert_ne!(replacement, old);
    assert!(!identity.is_live());
    assert!(ProcessIdentity::read(info.pid.unwrap())?.is_live());
    assert!(backend.get_live_pane_info(&old)?.is_none());
    assert!(backend.send_keys(&old, "touch stale-action").is_err());
    backend.send_keys(&replacement, "touch replacement-ready")?;
    backend.send_key(&replacement, "Enter")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !cwd.join("replacement-ready").exists() {
        ensure!(
            std::time::Instant::now() < deadline,
            "Replacement shell did not accept input"
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert!(!cwd.join("stale-action").exists());

    // exec replaces the running program, but preserves the terminal and PID.
    // An action with the discovered handle must still target this lifetime.
    backend.send_keys(&replacement, "exec /bin/sleep 60")?;
    backend.send_key(&replacement, "Enter")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let refreshed = backend.get_live_pane_info(&replacement)?.unwrap();
        if refreshed.current_command.as_deref() == Some("sleep") {
            assert_eq!(refreshed.pid, info.pid);
            break;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "Replacement program not reported: {refreshed:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    backend.send_key(&replacement, "C-c")?;
    wait_for_exit(&replacement)?;
    Ok(())
}
