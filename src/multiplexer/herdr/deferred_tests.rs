use super::*;

#[test]
#[ignore = "requires an isolated Herdr 0.9.0 server; use integration/run.py"]
fn isolated_deferred_cleanup() -> Result<()> {
    let endpoint = std::env::var("WORKMUX_HERDR_DEFERRED_SOCKET")?;
    let backend = HerdrBackend::for_socket(&endpoint);
    let cwd = std::env::current_dir()?;
    let key = backend.create_session(CreateSessionParams {
        prefix: "",
        name: "deferred",
        cwd: &cwd,
        initial_window_name: Some("owned"),
    })?;
    backend.finish_pane_setup(std::slice::from_ref(&key))?;
    let tab = backend.key(&backend.pane(&key)?.tab_id)?;
    let command = backend.shell_close_window_by_id_guard_cmd(&tab)?;
    assert!(command.contains(" _herdr-deferred "));
    // The absolute workmux executable needs no interpreter on PATH.
    let focus = backend.shell_select_window_cmd("owned")?;
    assert!(
        std::process::Command::new("/bin/sh")
            .args(["-c", &focus])
            .env("PATH", "")
            .status()?
            .success()
    );
    // A later unowned pane must block the saved cleanup command.
    let original = backend.pane(&key)?;
    let response = backend.client.request(
        "pane.split",
        json!({
            "target_pane_id": original.pane_id, "direction": "right", "focus": false,
        }),
    )?;
    assert_eq!(response["pane"]["tab_id"], original.tab_id);
    let status = std::process::Command::new("sh")
        .args(["-c", &command])
        .status()?;
    assert!(!status.success());
    assert!(backend.pane(&key).is_ok());
    for pane in backend.client.snapshot()?.panes {
        if pane.tab_id == original.tab_id && pane.terminal_id != original.terminal_id {
            backend
                .client
                .request("pane.close", json!({"pane_id":pane.pane_id}))?;
        }
    }
    // Lifetime checks are performed by the helper, not only by its Rust caller.
    let stale = command.replace(&backend.client.boot()?, "stale-server-lifetime");
    let status = std::process::Command::new("sh")
        .args(["-c", &stale])
        .status()?;
    assert!(!status.success());
    assert!(backend.pane(&key).is_ok());
    backend.schedule_window_close("owned", Duration::from_secs(1))?;
    // The Python runner checks closure after this entire test process exits.
    // A thread owned by this process cannot satisfy that check.
    Ok(())
}
