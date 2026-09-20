//! Live caller probes. Only integration/caller_checks.py supplies their private server.
use super::*;
use std::time::Instant;

#[test]
#[ignore = "requires a private Herdr server; use integration/caller_checks.py"]
fn isolated_caller_discovery() -> Result<()> {
    let endpoint = std::env::var("WORKMUX_CALLER_SOCKET")
        .context("Run integration/caller_checks.py; do not use an inherited server")?;
    let backend = HerdrBackend::for_socket(&endpoint);
    let expected: Value = serde_json::from_str(&std::env::var("WORKMUX_CALLER_EXPECTED")?)?;
    let check = || -> Result<()> {
        if expected.is_null() {
            assert_eq!(backend.current_pane_id(), None);
            assert_eq!(backend.active_pane_id(), None);
            assert_eq!(backend.current_window_id()?, None);
            assert_eq!(backend.current_session_id()?, None);
            assert_eq!(backend.current_window_name()?, None);
            assert_eq!(backend.current_session(), None);
            assert!(backend.get_client_active_pane_path().is_err());
        } else {
            let key = |field: &str| backend.key(expected[field].as_str().unwrap());
            assert_eq!(backend.current_pane_id(), Some(key("terminal_id")?));
            assert_eq!(backend.active_pane_id(), Some(key("terminal_id")?));
            assert_eq!(backend.current_window_id()?, Some(key("tab_id")?));
            assert_eq!(backend.current_session_id()?, Some(key("workspace_id")?));
            assert_eq!(backend.current_session().as_deref(), Some("caller"));
            assert_eq!(backend.current_window_name()?.as_deref(), Some("origin"));
            assert_eq!(
                backend.get_client_active_pane_path()?,
                PathBuf::from(expected["cwd"].as_str().unwrap())
            );
        }
        Ok(())
    };
    check()?;
    if let Ok(sync) = std::env::var("WORKMUX_CALLER_SYNC") {
        let sync = PathBuf::from(sync);
        std::fs::write(sync.with_extension("ready"), b"ready")?;
        // Keep resolving while the second real UI client moves focus. The
        // Python driver verifies each focus change before it releases us.
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut checks = 0;
        let mut phase = 0;
        while !sync.with_extension("done").exists() {
            ensure!(Instant::now() < deadline, "Second client did not finish");
            check()?;
            checks += 1;
            if sync.with_extension(format!("phase-{phase}")).exists() {
                // Check again after the driver has confirmed the new focus.
                check()?;
                std::fs::write(sync.with_extension(format!("ack-{phase}")), b"ok")?;
                phase += 1;
            }
        }
        ensure!(checks > 0, "No caller checks overlapped focus changes");
        assert_eq!(phase, 7, "Each confirmed focus state must be checked");
        check()?;
        println!("CALLER_FOCUS_CHECKS={checks}");
    }
    println!("CALLER_DISCOVERY_PASSED");
    Ok(())
}
