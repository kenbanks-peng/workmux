//! Dimension/split regressions. Live probes require the private Python runner.
use super::*;

#[test]
fn dimensions_refresh_and_reject_sizes_after_shrink() -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    // Both axes include zero/one-cell targets and a target that shrank since
    // the caller last queried it. No mutation is permitted on these paths.
    for direction in [SplitDirection::Horizontal, SplitDirection::Vertical] {
        for (extent, size, expected, queries_expected) in [
            (0, Some(8), "exceeds available space", 2),
            (1, Some(8), "exceeds available space", 2),
            (8, Some(8), "exceeds available space", 2),
            (0, None, "at least 2 cells", 2),
            (1, None, "at least 2 cells", 2),
        ] {
            let directory = tempfile::tempdir()?;
            let socket = directory.path().join("api.sock");
            let listener = UnixListener::bind(&socket)?;
            listener.set_nonblocking(true)?;
            let backend = HerdrBackend::for_socket(socket.to_str().unwrap());
            let (stop, stopped) = std::sync::mpsc::channel();
            let server = thread::spawn(move || {
                let mut queries = 0;
                loop {
                    let (mut stream, _) = match listener.accept() {
                        Ok(connection) => connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if !matches!(
                                stopped.try_recv(),
                                Err(std::sync::mpsc::TryRecvError::Empty)
                            ) {
                                break;
                            }
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        Err(error) => panic!("{error}"),
                    };
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let result = match request["method"].as_str().unwrap() {
                        "session.snapshot" => json!({"snapshot":{
                            "version":"0.9.0", "protocol":22,
                            "workspaces":[{"workspace_id":"w1","label":"test"}],
                            "tabs":[{"workspace_id":"w1","tab_id":"t1","label":"test"}],
                            "panes":[{"workspace_id":"w1","tab_id":"t1","pane_id":"p1","terminal_id":"term1","focused":false,"cwd":"/tmp"}]
                        }}),
                        "pane.layout" => {
                            assert_eq!(request["params"], json!({"pane_id":"p1"}));
                            queries += 1;
                            let size = if queries == 1 { 80 } else { extent };
                            // Put an unrelated rectangle first to catch wrong-pane selection.
                            json!({"layout":{"panes":[
                                {"pane_id":"unrelated","rect":{"width":999,"height":999}},
                                {"pane_id":"p1","rect":{"width":size,"height":size}}
                            ]}})
                        }
                        method => panic!("unexpected mutation: {method}"),
                    };
                    writeln!(stream, "{}", json!({"id":"workmux","result":result})).unwrap();
                }
                assert_eq!(
                    queries, queries_expected,
                    "split must obtain fresh dimensions"
                );
            });
            let key = backend.key("term1")?;
            let before = backend.pane_dimensions(&key)?;
            assert_eq!((before.width, before.height), (80, 80));
            let error = backend
                .split_pane(&key, &direction, directory.path(), size, None, None)
                .unwrap_err();
            stop.send(())?;
            server.join().unwrap();
            assert!(error.to_string().contains(expected), "{error:#}");
        }
    }
    Ok(())
}

fn rendezvous(name: &str) -> Result<()> {
    std::fs::write(format!("{name}-ready"), "")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !Path::new(&format!("{name}-continue")).exists() {
        ensure!(
            std::time::Instant::now() < deadline,
            "resize runner did not respond: {name}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[test]
#[ignore = "requires private server; use integration/dimensions_checks.py"]
fn isolated_dimensions_nested_and_resize() -> Result<()> {
    // Missing configuration is an error, never a silent pass.
    let endpoint = std::env::var("WORKMUX_HERDR_DIMENSIONS_SOCKET")?;
    let backend = HerdrBackend::for_socket(&endpoint);
    ensure!(
        backend.client.snapshot()?.panes.is_empty(),
        "expected an empty private server"
    );
    let cwd = std::env::current_dir()?;
    let root = backend.create_session(CreateSessionParams {
        prefix: "",
        name: "dimensions",
        cwd: &cwd,
        initial_window_name: None,
    })?;
    rendezvous("attach")?;
    let old = backend.pane_dimensions(&root)?;
    ensure!(old.width > 80);
    rendezvous("shrink")?;
    let count = backend.client.snapshot()?.panes.len();
    let error = backend
        .split_pane(
            &root,
            &SplitDirection::Horizontal,
            &cwd,
            Some(old.width / 2),
            None,
            None,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("exceeds available space"),
        "{error:#}"
    );
    assert_eq!(backend.client.snapshot()?.panes.len(), count);
    let resized = backend.pane_dimensions(&root)?;
    assert!(resized.width < old.width / 2);
    let right = backend.split_pane(
        &root,
        &SplitDirection::Horizontal,
        &cwd,
        None,
        Some(50),
        None,
    )?;
    let right_before = backend.pane_dimensions(&right)?;
    let root_before = backend.pane_dimensions(&root)?;
    let bottom = backend.split_pane(
        &right,
        &SplitDirection::Vertical,
        &cwd,
        None,
        Some(50),
        None,
    )?;
    let right_after = backend.pane_dimensions(&right)?;
    let bottom_before = backend.pane_dimensions(&bottom)?;
    assert_eq!(right_after.width, right_before.width);
    assert!(right_after.height > 0 && right_after.height < right_before.height);
    assert!(bottom_before.height > 0 && bottom_before.height < right_before.height);
    let nested = backend.split_pane(
        &bottom,
        &SplitDirection::Horizontal,
        &cwd,
        Some(bottom_before.width / 2),
        None,
        None,
    )?;
    let nested_dims = backend.pane_dimensions(&nested)?;
    assert!(nested_dims.width > 0 && nested_dims.width < bottom_before.width);
    assert_eq!(nested_dims.height, bottom_before.height);
    let root_after = backend.pane_dimensions(&root)?;
    assert_eq!(
        (root_after.width, root_after.height),
        (root_before.width, root_before.height)
    );
    let panes = backend.client.snapshot()?.panes;
    assert_eq!(panes.len(), 4);
    let tab = backend.pane(&root)?.tab_id;
    assert!(panes.iter().all(|pane| pane.tab_id == tab));
    rendezvous("boundary")?;
    assert_eq!(backend.pane_dimensions(&nested)?.width, 2);
    let boundary = backend.split_pane(
        &nested,
        &SplitDirection::Horizontal,
        &cwd,
        None,
        Some(50),
        None,
    )?;
    assert!(backend.pane_dimensions(&boundary)?.width > 0);
    assert!(backend.pane_dimensions(&nested)?.width > 0);
    assert_eq!(backend.client.snapshot()?.panes.len(), 5);
    backend.kill_pane(&boundary)?;
    assert_eq!(backend.pane_dimensions(&nested)?.width, 2);
    // A tiny attached viewport exercises the server's minimum geometry rather
    // than inferring terminal behavior from a scripted rectangle.
    rendezvous("minimum")?;
    let minimum = backend.pane_dimensions(&nested)?;
    assert_eq!(minimum.width, 1);
    let error = backend
        .split_pane(
            &nested,
            &SplitDirection::Horizontal,
            &cwd,
            Some(1),
            None,
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("exceeds available space"));
    assert_eq!(backend.client.snapshot()?.panes.len(), 4);
    for direction in [SplitDirection::Horizontal, SplitDirection::Vertical] {
        let error = backend
            .split_pane(&nested, &direction, &cwd, None, Some(50), None)
            .unwrap_err();
        assert!(error.to_string().contains("at least 2 cells"), "{error:#}");
        assert_eq!(backend.client.snapshot()?.panes.len(), 4);
    }
    println!(
        "HERDR_DIMENSIONS_PASSED old={} resized={} minimum={}x{}",
        old.width, resized.width, minimum.width, minimum.height
    );
    Ok(())
}
