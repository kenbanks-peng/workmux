use super::*;

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_cleanup_race() -> Result<()> {
    let backend = HerdrBackend::for_socket(&std::env::var("HERDR_SOCKET_PATH")?);
    let mode = std::env::var("WORKMUX_HERDR_CLEANUP_MODE")?;
    let cwd = std::env::current_dir()?;
    let key = backend.create_session(CreateSessionParams {
        prefix: "",
        name: "cleanup-race",
        cwd: &cwd,
        initial_window_name: Some("owned"),
    })?;
    backend.finish_pane_setup(std::slice::from_ref(&key))?;
    let original = backend.pane(&key)?;
    std::fs::write(cwd.join("arm-cleanup"), "")?;
    if mode.starts_with("deferred") {
        let command = if mode.ends_with("workspace") {
            backend.shell_kill_session_cmd("cleanup-race")?
        } else {
            backend.shell_close_window_by_id_guard_cmd(&backend.key(&original.tab_id)?)?
        };
        let output = std::process::Command::new("sh")
            .args(["-c", &command])
            .output()?;
        ensure!(
            !output.status.success(),
            "Cleanup must report the retained container"
        );
        ensure!(String::from_utf8_lossy(&output.stderr).contains("preserved unexpected occupants"));
    } else {
        let result = if mode.ends_with("workspace") {
            backend.kill_session("cleanup-race")
        } else {
            backend.kill_window("owned")
        };
        let error = result.unwrap_err();
        ensure!(
            error.to_string().contains("preserved unexpected occupants"),
            "{error:#}"
        );
    }
    ensure!(
        backend.pane(&key).is_err(),
        "Captured terminal was not closed"
    );
    Ok(())
}

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_launch_ownership() -> Result<()> {
    let backend = HerdrBackend::for_socket(&std::env::var("HERDR_SOCKET_PATH")?);
    let cwd = std::env::current_dir()?;
    let key = backend.create_session(CreateSessionParams {
        prefix: "",
        name: "launch-owner",
        cwd: &cwd,
        initial_window_name: Some("owned"),
    })?;
    backend.set_window_ownership(&key, "live-owner", true)?;
    let original = backend.pane(&key)?;
    // Move the owned terminal to a tab whose cached owner differs.
    let destination = backend.new_tab(&original.workspace_id, Some("destination"), &cwd, None)?;
    backend.set_window_ownership(&destination, "stale-tab-owner", true)?;
    let target = backend.pane(&destination)?;
    backend.client.request(
        "pane.move",
        json!({
            "pane_id": original.pane_id,
            "destination": {"type":"tab", "tab_id":target.tab_id,
                "target_pane_id":target.pane_id, "split":"right"}, "focus":false
        }),
    )?;
    backend.kill_pane(&destination)?;
    let replacement = backend.move_launch(&key, &cwd, None, "right", 0.5, true)?;
    let owner = backend.verified_terminal(&backend.pane(&replacement)?)?;
    assert_eq!(owner.token.as_deref(), Some("live-owner"));
    assert!(owner.primary);
    let split = backend.move_launch(&replacement, &cwd, None, "right", 0.5, false)?;
    let owner = backend.verified_terminal(&backend.pane(&split)?)?;
    assert_eq!(owner.token.as_deref(), Some("live-owner"));
    assert!(!owner.primary);
    backend.kill_session("launch-owner")?;
    Ok(())
}
