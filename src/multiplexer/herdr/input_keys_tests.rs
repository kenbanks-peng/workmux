//! Live terminal receipt, not controlled-server request encoding.
use super::*;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a private live Herdr server; use integration/input_keys_checks.py"]
fn isolated_input_key_receipt() -> Result<()> {
    // Missing fixture setup is an error, not a passing live test.
    let endpoint = std::env::var("WORKMUX_HERDR_INPUT_KEYS_SOCKET")?;
    let terminal = std::env::var("WORKMUX_HERDR_INPUT_KEYS_TERMINAL")?;
    let received = std::env::var("WORKMUX_HERDR_INPUT_KEYS_RECEIVED")?;
    let backend = HerdrBackend::for_socket(&endpoint);
    let key = backend.key(&terminal)?;
    let original = backend.client.snapshot()?;
    let pid = backend.get_live_pane_info(&key)?.unwrap().pid;
    let mut expected = Vec::new();

    // These bytes are terminal input conventions, independent of the native
    // key names sent by the adapter. The receiver disables signal processing
    // so Ctrl-C/D/Z can be checked without terminating or suspending it.
    for (name, bytes) in [
        (" ", &b" "[..]),
        ("BSpace", &b"\x7f"[..]),
        ("C-c", &b"\x03"[..]),
        ("C-d", &b"\x04"[..]),
        ("C-z", &b"\x1a"[..]),
        ("enter", &b"\r"[..]),
        ("Escape", &b"\x1b"[..]),
        ("Tab", &b"\t"[..]),
        ("Up", &b"\x1b[A"[..]),
    ] {
        backend.send_key(&key, name)?;
        expected.extend_from_slice(bytes);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let actual = std::fs::read(&received)?;
            if actual == expected {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "key {name:?}: expected {expected:?}, received {actual:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // Also detect delayed duplicate bytes or an implicit submit.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(std::fs::read(&received)?, expected, "key {name:?}");
        let current = backend.client.snapshot()?;
        assert_eq!(current.focused_tab_id, original.focused_tab_id);
        assert_eq!(backend.get_live_pane_info(&key)?.unwrap().pid, pid);
        println!("PASS live key {name:?}: {bytes:02x?}");
    }
    println!("HERDR_INPUT_KEYS_RECEIPT_PASSED");
    Ok(())
}
