//! Capture acceptance probe. Only the private-server Python runner supplies inputs.
use super::*;

#[test]
#[ignore = "requires a private live server; use integration/capture_checks.py"]
fn isolated_capture_output() -> Result<()> {
    // Do not fall back to an inherited Herdr endpoint, or silently pass without inputs.
    let endpoint = std::env::var("WORKMUX_CAPTURE_SOCKET")?;
    let manifest = std::env::var("WORKMUX_CAPTURE_MANIFEST")?;
    let cases: Value = serde_json::from_slice(&std::fs::read(manifest)?)?;
    let backend = HerdrBackend::for_socket(&endpoint);
    for case in cases.as_array().context("Expected capture cases")? {
        let terminal = case["terminal_id"].as_str().context("Missing terminal")?;
        let lines = u16::try_from(case["lines"].as_u64().context("Missing lines")?)?;
        let expected = case["expected"].as_str().context("Missing expected text")?;
        let key = backend.key(terminal)?;
        let actual = backend
            .capture_pane(&key, lines)
            .context("Capture failed")?;
        // Keep large-output failures readable, but compare every byte and row.
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "{}",
            case["name"]
        );
        for (index, (actual, expected)) in actual.lines().zip(expected.lines()).enumerate() {
            assert_eq!(actual, expected, "{} row {index}", case["name"]);
        }
        assert_eq!(actual, expected, "{}", case["name"]);
    }
    Ok(())
}
