//! Controlled-server contracts, not evidence of live label limits.
use super::platform_tests::{Probe, snap, snapshot};
use super::*;

#[test]
fn window_labels_preserve_empty_and_long_unicode_and_report_rejection() {
    for label in [String::new(), "界🙂".repeat(4096)] {
        for rejected in [false, true] {
            for at_pane in [false, true] {
                let response = if rejected {
                    json!({"error":{"code":"invalid_label"}})
                } else {
                    json!({"result":{}})
                };
                let mut steps = vec![snap(snapshot())];
                if at_pane {
                    steps.push(snap(snapshot())); // Qualified pane key needs boot identity.
                }
                steps.push((
                    "tab.rename",
                    json!({"tab_id":"t1","label":label}),
                    Some(response),
                ));
                let probe = Probe::new(steps);
                let result = if at_pane {
                    let key = probe.backend.key("term1").unwrap();
                    probe.backend.rename_window_at_pane(&key, &label)
                } else {
                    probe.backend.rename_window("window", &label)
                };
                if rejected {
                    assert!(result.unwrap_err().to_string().contains("invalid_label"));
                } else {
                    result.unwrap();
                }
                probe.finish();
            }
        }
    }
}

#[test]
fn session_labels_preserve_empty_and_long_unicode_and_report_rejection() -> Result<()> {
    const TEST: &str = "multiplexer::herdr::label_tests::session_labels_preserve_empty_and_long_unicode_and_report_rejection";
    if !crate::test_support::is_isolated_child(TEST) {
        let root = tempfile::tempdir()?;
        crate::test_support::run_isolated_test(
            TEST,
            root.path(),
            &[("XDG_STATE_HOME", root.path())],
        );
        return Ok(());
    }
    for label in [String::new(), "界🙂".repeat(4096)] {
        for rejected in [false, true] {
            let response = if rejected {
                json!({"error":{"code":"invalid_label"}})
            } else {
                json!({"result":{}})
            };
            let probe = Probe::new(vec![
                snap(snapshot()), // Boot identity.
                snap(snapshot()), // Resolve the old session.
                snap(snapshot()), // Check for a new-name collision.
                (
                    "workspace.rename",
                    json!({"workspace_id":"w1","label":label}),
                    Some(response),
                ),
            ]);
            let boot = probe.backend.client.boot()?;
            let path = probe.backend.record_path("w1")?;
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, boot)?;
            let result = probe.backend.rename_session("session", &label);
            if rejected {
                assert!(result.unwrap_err().to_string().contains("invalid_label"));
            } else {
                result?;
            }
            probe.finish();
        }
    }
    println!("{}", crate::test_support::ISOLATED_TEST_CANARY);
    Ok(())
}
