use super::*;

fn options() -> PaneSetupOptions<'static> {
    PaneSetupOptions {
        run_commands: true,
        prompt_file_path: None,
        worktree_root: None,
        lima_vm_name: None,
        resume_mode: ResumeMode::None,
    }
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_setup_lifetime() -> Result<()> {
    let backend = HerdrBackend::for_socket(&std::env::var("HERDR_SOCKET_PATH")?);
    let cwd = std::env::current_dir()?;
    let initial = backend.create_session(CreateSessionParams {
        prefix: "",
        name: "setup-lifetime",
        cwd: &cwd,
        initial_window_name: Some("empty"),
    })?;
    backend.setup_panes(&initial, &[], &cwd, options(), &Config::default(), None)?;
    assert!(backend.pane(&initial).is_ok());
    assert!(
        backend
            .respawn_pane(&initial, &cwd, Some("exit 99"))
            .is_err()
    );
    assert!(backend.initial_launches.lock().unwrap().is_empty());
    let original = backend.client.snapshot()?;

    let new_window = |name| {
        backend.create_window_in_session(CreateWindowInSessionParams {
            session_name: "setup-lifetime",
            name: Some(name),
            cwd: &cwd,
        })
    };
    let failed = new_window("prepare-failure")?;
    let config: Config = serde_json::from_value(json!({
        "sandbox": {"enabled": true, "backend": "lima", "target": "all"}
    }))?;
    let panes = serde_json::from_value::<Vec<PaneConfig>>(json!([
        {"command": "touch MUST_NOT_RUN"}
    ]))?;
    let error = backend
        .setup_panes(&failed, &panes, &cwd, options(), &config, None)
        .unwrap_err();
    assert!(error.to_string().contains("Lima VM name missing"));
    assert_eq!(backend.client.snapshot()?.panes.len(), original.panes.len());
    assert_eq!(backend.client.snapshot()?.tabs.len(), original.tabs.len());
    assert!(!cwd.join("MUST_NOT_RUN").exists());
    assert!(backend.launches.lock().unwrap().is_empty());
    assert!(backend.pending_launch.lock().unwrap().is_none());

    // An error before allocation must also release the untouched initial shell
    // and remove the pending handshake. It must not close that shell.
    let early = new_window("early-failure")?;
    let panes = serde_json::from_value::<Vec<PaneConfig>>(json!([
        {}, {"command": "touch MUST_NOT_RUN", "split": "vertical", "target": 99}
    ]))?;
    let error = backend
        .setup_panes(&early, &panes, &cwd, options(), &Config::default(), None)
        .unwrap_err();
    assert!(error.to_string().contains("Invalid target pane index"));
    assert!(backend.pane(&early).is_ok());
    assert!(backend.respawn_pane(&early, &cwd, Some("exit 99")).is_err());
    assert!(backend.pending_launch.lock().unwrap().is_none());
    assert!(backend.initial_launches.lock().unwrap().is_empty());

    // A later error must not cancel an already delivered command. Identity
    // injection must allow shell builtins and apply to the entire command list.
    let late = new_window("late-failure")?;
    let marker = cwd.join("setup-identity");
    let command = format!(
        "cd /; printf '%s\\n' \"$WORKMUX_STATUS_BACKEND\" \"$WORKMUX_STATUS_INSTANCE\" \"$WORKMUX_STATUS_PANE_ID\" > {}",
        agent::shell_quote(&marker.to_string_lossy())
    );
    let panes = serde_json::from_value::<Vec<PaneConfig>>(json!([
        {"command": command},
        {"command": "touch MUST_NOT_RUN", "split": "vertical", "target": 99}
    ]))?;
    let error = backend
        .setup_panes(&late, &panes, &cwd, options(), &Config::default(), None)
        .unwrap_err();
    assert!(error.to_string().contains("Invalid target pane index"));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let identity = loop {
        let text = std::fs::read_to_string(&marker).unwrap_or_default();
        if text.lines().count() == 3 {
            break text;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "Command did not complete"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let identity: Vec<_> = identity.lines().collect();
    assert_eq!(identity[0], "herdr");
    assert_eq!(identity[1], backend.instance_id());
    assert!(backend.pane(identity[2]).is_ok());
    assert!(
        backend
            .respawn_pane(identity[2], &cwd, Some("exit 99"))
            .is_err()
    );
    assert!(backend.launches.lock().unwrap().is_empty());
    assert!(backend.pending_launch.lock().unwrap().is_none());
    assert!(backend.initial_launches.lock().unwrap().is_empty());
    assert!(backend.fresh.lock().unwrap().is_empty());
    assert!(!cwd.join("MUST_NOT_RUN").exists());
    backend.kill_session("setup-lifetime")?;
    Ok(())
}
