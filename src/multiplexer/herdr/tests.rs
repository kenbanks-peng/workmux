//! The Python fixture supplies private servers and invokes these probes alone.
//! An ordinary cargo test run does not establish real-server acceptance.
use super::*;

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_core_operations() -> Result<()> {
    let Ok(endpoint) = std::env::var("WORKMUX_HERDR_CORE_SOCKET") else {
        return Ok(());
    };
    let other_endpoint = std::env::var("WORKMUX_HERDR_OTHER_SOCKET")?;
    let backend = HerdrBackend::for_socket(&endpoint);
    let other = create_backend_for_instance(BackendType::Herdr, &other_endpoint);
    let cwd = std::env::current_dir()?;
    assert!(backend.is_running()?);
    assert_eq!(
        PathBuf::from(backend.resolve_instance_id()?),
        std::fs::canonicalize(&endpoint)?
    );
    assert_eq!(
        PathBuf::from(other.resolve_instance_id()?),
        std::fs::canonicalize(&other_endpoint)?
    );
    assert!(backend.current_pane_id().is_none()); // No last-focus fallback.
    assert!(
        backend
            .create_window(CreateWindowParams {
                prefix: "",
                name: "external",
                cwd: &cwd,
                after_window: None
            })
            .is_err()
    );
    assert!(backend.client.snapshot()?.tabs.is_empty());

    let first = backend.create_session(CreateSessionParams {
        prefix: "wm-",
        name: "one",
        cwd: &cwd,
        initial_window_name: Some("root"),
    })?;
    let foreign = other.create_session(CreateSessionParams {
        prefix: "wm-",
        name: "one",
        cwd: &cwd,
        initial_window_name: Some("root"),
    })?;
    let first_pane = backend.pane(&first)?;
    let workspace = first_pane.workspace_id.clone();
    let first_tab = backend.key(&first_pane.tab_id)?;
    assert_eq!(first_pane.tab_id, "w1:t1");
    assert_eq!(first_pane.workspace_id, "w1");
    assert_ne!(backend.server_boot_id()?, other.server_boot_id()?);
    assert!(other.select_pane(&first).is_err());
    assert!(backend.select_pane(&foreign).is_err());
    assert_eq!(
        backend.get_all_session_names()?,
        HashSet::from(["wm-one".into()])
    );
    assert_eq!(
        backend.get_window_names_in_session("wm-one")?,
        HashSet::from(["root".into()])
    );
    assert!(
        backend
            .create_session(CreateSessionParams {
                prefix: "wm-",
                name: "one",
                cwd: &cwd,
                initial_window_name: None
            })
            .is_err()
    );
    assert_eq!(backend.client.snapshot()?.workspaces.len(), 1);
    let focus_before = backend.client.snapshot()?.focused_tab_id;
    let last = backend.create_window_in_session(CreateWindowInSessionParams {
        session_name: "wm-one",
        name: Some("last"),
        cwd: &cwd,
    })?;
    let middle = backend.create_tab_in_workspace(
        &workspace,
        CreateWindowParams {
            prefix: "wm-",
            name: "middle",
            cwd: &cwd,
            after_window: Some(&first_tab),
        },
    )?;
    assert_eq!(backend.client.snapshot()?.focused_tab_id, focus_before);
    let tabs = backend.client.snapshot()?.tabs;
    assert_eq!(
        tabs.iter()
            .map(|tab| tab.label.as_str())
            .collect::<Vec<_>>(),
        ["root", "wm-middle", "last"]
    );
    let after_middle = backend.key(&backend.pane(&middle)?.tab_id)?;
    let placed = backend.create_tab_in_workspace(
        &workspace,
        CreateWindowParams {
            prefix: "",
            name: "placed",
            cwd: &cwd,
            after_window: Some(&after_middle),
        },
    )?;
    assert_eq!(
        backend
            .client
            .snapshot()?
            .tabs
            .iter()
            .map(|tab| tab.label.as_str())
            .collect::<Vec<_>>(),
        ["root", "wm-middle", "placed", "last"]
    );
    backend.kill_pane(&placed)?;
    assert!(
        backend
            .create_tab_in_workspace(
                &workspace,
                CreateWindowParams {
                    prefix: "",
                    name: "bad",
                    cwd: &cwd,
                    after_window: Some("unverified")
                }
            )
            .is_err()
    );
    assert_eq!(backend.client.snapshot()?.tabs.len(), 3);
    let dimensions = backend.pane_dimensions(&middle)?;
    assert!(dimensions.width > 0 && dimensions.height > 0);
    backend.select_pane(&middle)?;
    let info = backend.get_live_pane_info(&middle)?.unwrap();
    assert_eq!(info.working_dir, cwd);
    assert!(info.pid.is_some());
    assert_eq!(info.session.as_deref(), Some("wm-one"));
    assert_eq!(info.window.as_deref(), Some("wm-middle"));

    // Labels are data, not shell commands or public IDs.
    let label = "renamed ; $(touch must-not-exist) Ω";
    backend.rename_window_at_pane(&middle, label)?;
    backend.set_pane_name(&middle, "pane Ω")?;
    assert_eq!(backend.pane(&middle)?.label.as_deref(), Some("pane Ω"));
    backend.rename_session("wm-one", "renamed workspace")?;
    let workspace_key = backend.key(&workspace)?;
    let target = WindowTarget::new(label.into(), Some(workspace_key));
    assert!(backend.window_target_exists(&target)?);
    backend.select_window_target(&target)?;
    assert!(!cwd.join("must-not-exist").exists());
    assert_eq!(
        other.get_all_session_names()?,
        HashSet::from(["wm-one".into()])
    );
    assert_eq!(
        other.get_all_window_names()?,
        HashSet::from(["root".into()])
    );
    backend.rename_window(label, "middle")?;
    backend.select_window("", "middle")?;
    backend.switch_to_session("", "renamed workspace")?;

    let second = backend.create_session(CreateSessionParams {
        prefix: "",
        name: "second",
        cwd: &cwd,
        initial_window_name: Some("middle"),
    })?;
    let second_pane = backend.pane(&second)?;
    let second_tab = backend.key(&second_pane.tab_id)?;
    assert!(
        backend
            .create_tab_in_workspace(
                &workspace,
                CreateWindowParams {
                    prefix: "",
                    name: "bad",
                    cwd: &cwd,
                    after_window: Some(&second_tab)
                }
            )
            .is_err()
    );
    assert_eq!(backend.client.snapshot()?.tabs.len(), 4);
    assert!(backend.select_window("", "middle").is_err());
    assert!(
        backend
            .window_target_exists(&WindowTarget::new("middle".into(), None))
            .is_err()
    );
    assert!(!backend.window_target_exists(&WindowTarget::new("missing".into(), None))?);
    backend.select_window_target(&WindowTarget::new("middle".into(), Some("second".into())))?;
    backend.kill_session("second")?;
    assert!(!backend.session_exists("second")?);
    assert!(backend.get_live_pane_info(&second)?.is_none());

    let split = backend.split_pane(
        &middle,
        &SplitDirection::Horizontal,
        &cwd,
        None,
        Some(30),
        None,
    )?;
    let split_info = backend.get_live_pane_info(&split)?.unwrap();
    assert_eq!(
        split_info.window_id,
        backend.get_live_pane_info(&middle)?.unwrap().window_id
    );
    assert!(backend.pane_dimensions(&split)?.width < dimensions.width);
    backend.zoom_pane(&split)?;
    let pane = backend.pane(&split)?;
    assert_eq!(
        backend
            .client
            .request("pane.layout", json!({"pane_id":pane.pane_id}))?["layout"]["zoomed"],
        true
    );
    backend.kill_pane(&split)?;
    assert!(backend.get_live_pane_info(&split)?.is_none());
    assert!(backend.kill_pane(&split).is_err());

    // A public address changes across workspaces; the stable terminal key follows it.
    let destination = backend.client.request(
        "workspace.create",
        json!({"label":"native", "cwd":cwd, "focus":false}),
    )?;
    let native: Pane = serde_json::from_value(destination["root_pane"].clone())?;
    let before = backend.get_live_pane_info(&last)?.unwrap().pid;
    let last_pane = backend.pane(&last)?;
    backend.client.request("pane.move", json!({"pane_id":last_pane.pane_id,"destination":{"type":"tab","tab_id":native.tab_id,"target_pane_id":native.pane_id,"split":"right"},"focus":false}))?;
    assert_ne!(backend.pane(&last)?.pane_id, last_pane.pane_id);
    assert_eq!(backend.get_live_pane_info(&last)?.unwrap().pid, before);
    backend.select_pane(&last)?;
    assert!(backend.kill_window("native").is_err());
    assert!(backend.kill_session("native").is_err());
    backend.kill_window("middle")?;
    assert!(backend.get_live_pane_info(&middle)?.is_none());
    backend.kill_session("renamed workspace")?;
    other.kill_session("wm-one")?;
    println!("HERDR_CORE_OPERATIONS_PASSED");
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_selection() -> Result<()> {
    let Ok(expected) = std::env::var("WORKMUX_HERDR_EXPECT_BACKEND") else {
        return Ok(());
    };
    assert_eq!(detect_backend_strict()?.to_string(), expected);
    assert_eq!(detect_backend().to_string(), expected);
    if expected == "herdr" {
        let backend = create_backend(detect_backend_strict()?);
        assert_eq!(
            PathBuf::from(backend.instance_id()),
            std::fs::canonicalize(std::env::var("HERDR_SOCKET_PATH")?)?
        );
        assert!(backend.is_running()?);
    }
    println!("HERDR_SELECTION_PASSED");
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_protocol_refusal() -> Result<()> {
    let Ok(endpoint) = std::env::var("WORKMUX_HERDR_REFUSAL_SOCKET") else {
        return Ok(());
    };
    let client = Client::new(endpoint);
    let error = client
        .request("workspace.create", json!({"label":"forbidden"}))
        .unwrap_err();
    assert!(
        error.to_string().contains("Unsupported Herdr server"),
        "{error:#}"
    );
    println!("HERDR_PROTOCOL_REFUSAL_PASSED");
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_server_replacement() -> Result<()> {
    let Ok(endpoint) = std::env::var("WORKMUX_HERDR_RESTART_SOCKET") else {
        return Ok(());
    };
    let backend = HerdrBackend::for_socket(&endpoint);
    let pane = backend.client.snapshot()?.panes.remove(0);
    let old_key = backend.key(&pane.terminal_id)?;
    std::fs::write("restart-ready", "ready")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !Path::new("restart-continue").exists() {
        ensure!(
            std::time::Instant::now() < deadline,
            "Fixture did not restart its server"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let error = backend
        .client
        .request("workspace.create", json!({"label":"forbidden"}))
        .unwrap_err();
    assert!(
        error.to_string().contains("server lifetime changed"),
        "{error:#}"
    );
    let fresh = HerdrBackend::for_socket(&endpoint);
    assert!(fresh.select_pane(&old_key).is_err());
    assert!(backend.server_boot_id().is_err());
    println!("HERDR_REPLACEMENT_REFUSAL_PASSED");
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_current_context() -> Result<()> {
    if std::env::var_os("WORKMUX_HERDR_CALLER_PROBE").is_none() {
        return Ok(());
    }
    let backend = HerdrBackend::new();
    let caller = backend.current_pane_id().context("Missing native caller")?;
    let workspace = backend
        .current_session_id()?
        .context("Missing caller workspace")?;
    let tab = backend.current_window_id()?.context("Missing caller tab")?;
    assert_eq!(backend.current_session().as_deref(), Some("caller"));
    assert_eq!(backend.current_window_name()?.as_deref(), Some("root"));
    assert_eq!(backend.active_pane_id().as_deref(), Some(caller.as_str()));
    assert_eq!(
        backend.rightmost_window_id()?.as_deref(),
        Some(tab.as_str())
    );
    let cwd = backend.get_client_active_pane_path()?;
    let tail = backend.create_window(CreateWindowParams {
        prefix: "wm-",
        name: "tail",
        cwd: &cwd,
        after_window: None,
    })?;
    let tail_tab = backend.key(&backend.pane(&tail)?.tab_id)?;
    let middle = backend.create_window(CreateWindowParams {
        prefix: "wm-",
        name: "middle",
        cwd: &cwd,
        after_window: Some(&tab),
    })?;
    assert_eq!(
        backend.rightmost_window_id()?.as_deref(),
        Some(tail_tab.as_str())
    );
    assert_eq!(
        backend.current_session_id()?.as_deref(),
        Some(workspace.as_str())
    );
    assert_eq!(backend.current_window_id()?.as_deref(), Some(tab.as_str()));
    // Focus another tab; caller resolution must not become last-focus resolution.
    backend.select_pane(&middle)?;
    assert_eq!(backend.current_pane_id().as_deref(), Some(caller.as_str()));
    backend.select_pane(&caller)?;
    backend.kill_window("wm-middle")?;
    backend.kill_window("wm-tail")?;
    assert_eq!(backend.get_all_live_pane_info()?.len(), 1);
    println!("HERDR_CURRENT_CONTEXT_PASSED");
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_split_size_limits() -> Result<()> {
    let Ok(endpoint) = std::env::var("WORKMUX_HERDR_SIZE_SOCKET") else {
        return Ok(());
    };
    let backend = HerdrBackend::for_socket(&endpoint);
    let cwd = std::env::current_dir()?;
    let initial = backend.create_session(CreateSessionParams {
        prefix: "",
        name: "size-limits",
        cwd: &cwd,
        initial_window_name: None,
    })?;
    let original = backend.pane(&initial)?;
    let original_pid = backend.get_live_pane_info(&initial)?.unwrap().pid;
    let original_snapshot = backend.client.snapshot()?;
    let check_unchanged = || -> Result<()> {
        let snapshot = backend.client.snapshot()?;
        assert_eq!(snapshot.panes.len(), original_snapshot.panes.len());
        assert_eq!(snapshot.tabs.len(), original_snapshot.tabs.len());
        assert_eq!(snapshot.focused_tab_id, original_snapshot.focused_tab_id);
        assert_eq!(backend.pane(&initial)?.terminal_id, original.terminal_id);
        assert_eq!(
            backend.get_live_pane_info(&initial)?.unwrap().pid,
            original_pid
        );
        Ok(())
    };
    let original_layout = backend
        .client
        .request("layout.export", json!({"tab_id": original.tab_id}))?;
    for direction in [SplitDirection::Horizontal, SplitDirection::Vertical] {
        // Config accepts these values. The server cannot keep their requested
        // proportions, so refusal is required before allocating a command tab.
        for percentage in [1, 5, 9, 91, 95, 99, 100] {
            let panes: Vec<PaneConfig> = serde_json::from_value(json!([
                {}, {"split": direction, "percentage": percentage}
            ]))?;
            let result = backend.setup_panes(
                &initial,
                &panes,
                &cwd,
                PaneSetupOptions {
                    run_commands: true,
                    prompt_file_path: None,
                    worktree_root: None,
                    lima_vm_name: None,
                    resume_mode: ResumeMode::None,
                },
                &Config::default(),
                None,
            );
            assert!(result.unwrap_err().to_string().contains("10–90%"));
            check_unchanged()?;
        }
        for size in [0, 1] {
            let result = backend.split_pane(
                &initial,
                &direction,
                &cwd,
                Some(size),
                None,
                Some("exit 99"),
            );
            assert!(result.unwrap_err().to_string().contains("10–90%"));
            check_unchanged()?;
        }
        // Boundary values must remain valid (avoid subtraction rounding at 90%).
        for percentage in [10, 50, 90] {
            let split =
                backend.split_pane(&initial, &direction, &cwd, None, Some(percentage), None)?;
            let layout = backend
                .client
                .request("layout.export", json!({"tab_id": original.tab_id}))?;
            let actual = layout["layout"]["root"]["ratio"].as_f64().unwrap();
            assert!((actual - f64::from(100 - percentage) / 100.0).abs() < 1e-6);
            backend.kill_pane(&split)?;
        }
    }
    check_unchanged()?;
    assert_eq!(
        backend
            .client
            .request("layout.export", json!({"tab_id": original.tab_id}),)?,
        original_layout
    );
    println!("HERDR_SPLIT_SIZE_LIMITS_PASSED");
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_launch_and_input_contract() -> Result<()> {
    let Ok(endpoint) = std::env::var("WORKMUX_HERDR_INPUT_SOCKET") else {
        return Ok(());
    };
    let backend = HerdrBackend::for_socket(&endpoint);
    let original = backend.client.snapshot()?;
    let pane = original
        .panes
        .iter()
        .find(|p| p.label.as_deref() == Some("input-agent"))
        .unwrap();
    let key = backend.key(&pane.terminal_id)?;
    let pid = backend.get_live_pane_info(&key)?.unwrap().pid;
    let cwd = PathBuf::from(std::env::var("WORKMUX_HERDR_INPUT_REPO")?);
    let mut expected = Vec::new();
    let mut check_input = |bytes: &[u8]| -> Result<()> {
        expected.extend_from_slice(bytes);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::fs::read("input").ok().as_ref() != Some(&expected) {
            ensure!(
                std::time::Instant::now() < deadline,
                "Input differs: {:?}",
                std::fs::read("input")
            );
            thread::sleep(Duration::from_millis(20));
        }
        thread::sleep(Duration::from_millis(100));
        assert_eq!(std::fs::read("input")?, expected);
        assert_eq!(
            backend.client.snapshot()?.focused_tab_id,
            original.focused_tab_id
        );
        Ok(())
    };
    backend.send_text_fragment(&key, "fragment 雪")?;
    check_input("fragment 雪".as_bytes())?;
    backend.paste_text(&key, "literal\n$HOME; λ")?;
    check_input("\x1b[200~literal\n$HOME; λ\x1b[201~".as_bytes())?;
    backend.send_enter(&key)?;
    check_input(b"\r")?;
    backend.send_key(&key, "C-c")?;
    check_input(b"\x03")?;
    backend.paste_and_submit(&key, "one")?;
    check_input(b"\x1b[200~one\x1b[201~\r")?;
    backend.send_keys_to_agent(&key, "!literal", Some("claude"))?;
    check_input(b"!literal\r")?;
    assert!(backend.clear_pane(&key).is_err());
    assert!(backend.respawn_pane(&key, &cwd, Some("exit 99")).is_err());
    let missing = backend.key("missing-terminal")?;
    assert!(backend.capture_pane(&missing, 0).is_none());
    assert!(backend.capture_pane(&missing, 100).is_none());
    assert!(backend.send_keys(&missing, "no").is_err());
    assert!(backend.clear_pane(&missing).is_err());
    assert!(
        backend
            .split_pane(&key, &SplitDirection::Stacked, &cwd, None, None, None)
            .unwrap_err()
            .to_string()
            .contains("Zellij")
    );
    check_input(b"")?;

    for before_wait in [true, false] {
        let first = backend.create_window_in_session(CreateWindowInSessionParams {
            session_name: "parent",
            name: Some("cancel-probe"),
            cwd: &cwd,
        })?;
        let handshake = backend.create_handshake()?;
        let script = handshake.script_content(&backend.get_default_shell()?);
        let controlled = backend.respawn_pane(&first, &cwd, Some(&script))?;
        if before_wait {
            drop(handshake);
        } else {
            handshake.wait()?;
            backend.clear_pane(&controlled)?;
        }
        backend.cancel_pane_launch(&controlled)?;
        assert!(backend.pane(&controlled).is_err());
    }
    let first = backend.create_window_in_session(CreateWindowInSessionParams {
        session_name: "parent",
        name: Some("finished-setup"),
        cwd: &cwd,
    })?;
    backend.setup_panes(
        &first,
        &[],
        &cwd,
        PaneSetupOptions {
            run_commands: true,
            prompt_file_path: None,
            worktree_root: None,
            lima_vm_name: None,
            resume_mode: ResumeMode::None,
        },
        &Config::default(),
        None,
    )?;
    assert!(backend.respawn_pane(&first, &cwd, Some("exit 99")).is_err());
    backend.kill_pane(&first)?;
    // Shared preparation failure after readiness must cancel the new terminal,
    // not leave an initialized shell waiting for an undeliverable command.
    let first = backend.create_window_in_session(CreateWindowInSessionParams {
        session_name: "parent",
        name: Some("prepare-failure"),
        cwd: &cwd,
    })?;
    let config: Config = serde_json::from_value(
        json!({"sandbox":{"enabled":true,"backend":"lima","target":"all"}}),
    )?;
    let panes: Vec<PaneConfig> = serde_json::from_value(json!([{"command":"echo MUST_NOT_RUN"}]))?;
    let error = backend
        .setup_panes(
            &first,
            &panes,
            &cwd,
            PaneSetupOptions {
                run_commands: true,
                prompt_file_path: None,
                worktree_root: None,
                lima_vm_name: None,
                resume_mode: ResumeMode::None,
            },
            &config,
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("Lima VM name missing"));
    assert_eq!(backend.client.snapshot()?.panes.len(), original.panes.len());
    assert_eq!(backend.client.snapshot()?.tabs.len(), original.tabs.len());
    assert_eq!(backend.get_live_pane_info(&key)?.unwrap().pid, pid);
    check_input(b"")?;
    println!("HERDR_LAUNCH_INPUT_PASSED");
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_persisted_identity_contract() -> Result<()> {
    let Ok(endpoint) = std::env::var("WORKMUX_HERDR_IDENTITY_SOCKET") else {
        return Ok(());
    };
    let backend = HerdrBackend::for_socket(&endpoint);
    let cwd = std::env::current_dir()?;
    let key = backend.create_session(CreateSessionParams {
        prefix: "",
        name: "owned",
        cwd: &cwd,
        initial_window_name: None,
    })?;
    backend.set_window_ownership(&key, "owner-1", true)?;
    backend.finish_pane_setup(std::slice::from_ref(&key))?;
    let pane = backend.pane(&key)?;
    let initial = backend.verified_terminal(&pane)?;
    let tab = backend.key(&pane.tab_id)?;
    backend.rename_window_at_pane(&key, "not-an-ownership-label")?;
    backend
        .client
        .request("tab.move", json!({"tab_id":pane.tab_id,"insert_index":0}))?;
    assert_eq!(
        backend.owned_window_targets("owner-1")?[0]
            .target
            .window_id
            .as_deref(),
        Some(tab.as_str())
    );
    let destination = backend.client.request(
        "workspace.create",
        json!({"label":"foreign","cwd":cwd,"focus":false}),
    )?;
    let native: Pane = serde_json::from_value(destination["root_pane"].clone())?;
    backend.client.request(
        "pane.rename",
        json!({"pane_id":native.pane_id,"label":"sidebar"}),
    )?;
    assert!(
        backend
            .kill_pane(&backend.key(&native.terminal_id)?)
            .is_err()
    );
    backend.client.request("pane.move", json!({"pane_id":pane.pane_id,"destination":{"type":"tab","tab_id":native.tab_id,"target_pane_id":native.pane_id,"split":"right"},"focus":false}))?;
    let moved = backend.pane(&key)?;
    assert_ne!(moved.pane_id, pane.pane_id);
    assert_eq!(backend.verified_terminal(&moved)?.shell, initial.shell);
    assert!(
        backend
            .verified_record(
                &backend.tab(&WindowTarget::with_id(
                    "foreign".into(),
                    None,
                    backend.key(&native.tab_id)?
                ))?,
                &backend.client.snapshot()?,
            )
            .is_ok()
    );
    assert_eq!(backend.owned_window_targets("owner-1")?.len(), 1);
    // Remove only the fixture's native terminal through its owner (Herdr).
    backend
        .client
        .request("pane.close", json!({"pane_id":native.pane_id}))?;
    let targets = backend.owned_window_targets("owner-1")?;
    assert_eq!(targets.len(), 1);
    assert!(targets[0].is_primary);
    backend.set_window_ownership(&key, "owner-1", true)?;
    assert!(
        backend
            .set_window_ownership(&key, "different-owner", true)
            .is_err()
    );
    drop(backend);
    let backend = HerdrBackend::for_socket(&endpoint);
    assert_eq!(backend.owned_window_targets("owner-1")?.len(), 1);
    let moved = backend.pane(&key)?;
    let mut record = backend.verified_terminal(&moved)?;
    let start = record.shell.start.clone();
    record.shell.start = "reused-pid-start".into();
    backend.save_terminal_record(&record)?;
    assert!(backend.kill_pane(&key).is_err());
    assert!(backend.kill_window("foreign").is_err());
    assert!(backend.get_live_pane_info(&key)?.is_some());
    record.shell.start = start;
    record.endpoint = "/foreign/socket".into();
    backend.save_terminal_record(&record)?;
    assert!(backend.kill_pane(&key).is_err());
    record.endpoint = backend.instance_id();
    backend.save_terminal_record(&record)?;
    backend.kill_pane(&key)?;
    println!("HERDR_PERSISTED_IDENTITY_PASSED");
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_native_popup_caller() -> Result<()> {
    let Ok(terminal) = std::env::var("WORKMUX_HERDR_POPUP_TERMINAL") else {
        return Ok(());
    };
    let backend = HerdrBackend::new();
    let key = backend
        .current_pane_id()
        .context("Native popup caller unavailable")?;
    assert_eq!(backend.pane(&key)?.terminal_id, terminal);
    assert_eq!(
        backend.current_window_id()?,
        Some(backend.key(&backend.pane(&key)?.tab_id)?)
    );
    assert_eq!(
        backend.current_session_id()?,
        Some(backend.key(&backend.pane(&key)?.workspace_id)?)
    );
    println!("HERDR_NATIVE_POPUP_IDENTITY_PASSED");
    Ok(())
}
