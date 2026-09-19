use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

#[test]
fn session_open_preserves_unowned_workspaces() -> Result<()> {
    const TEST: &str =
        "multiplexer::herdr::session_tests::session_open_preserves_unowned_workspaces";
    if !crate::test_support::is_isolated_child(TEST) {
        let root = tempfile::tempdir()?;
        crate::test_support::run_isolated_test(
            TEST,
            root.path(),
            &[("XDG_STATE_HOME", root.path())],
        );
        return Ok(());
    }
    for (labels, ownership, expected) in [
        (vec![], None, Ok("feature")),
        (vec!["wm-feature"], Some(true), Ok("feature")),
        (vec!["wm-feature"], None, Ok("feature-2")),
        (vec!["wm-feature"], Some(false), Ok("feature-2")),
        (vec!["wm-feature", "wm-feature-2"], None, Ok("feature-3")),
        (
            vec!["wm-feature", "wm-feature"],
            Some(true),
            Err("ambiguous (2 matches)"),
        ),
    ] {
        let root = tempfile::tempdir()?;
        let endpoint = root.path().join("api.sock");
        let listener = UnixListener::bind(&endpoint)?;
        let workspaces: Vec<_> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| json!({"workspace_id":format!("w{i}"), "label":label}))
            .collect();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], "session.snapshot"); // Read-only, even on collision.
                writeln!(
                    stream,
                    "{}",
                    json!({"id":"workmux", "result":{"snapshot":{
                        "version":"0.9.0", "protocol":22, "workspaces":workspaces,
                        "tabs":[], "panes":[]
                    }}})
                )
                .unwrap();
            }
        });
        let backend = HerdrBackend::for_socket(endpoint.to_str().unwrap());
        let boot = backend.client.boot()?;
        if let Some(current) = ownership {
            let path = backend.record_path("w0")?;
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, if current { &boot } else { "previous-boot" })?;
        }
        let result = backend.resolve_session_open_name("wm-", "feature");
        server.join().unwrap();
        match expected {
            Ok(name) => assert_eq!(result?, name),
            Err(message) => assert!(result.unwrap_err().to_string().contains(message)),
        }
    }
    println!("{}", crate::test_support::ISOLATED_TEST_CANARY);
    Ok(())
}
