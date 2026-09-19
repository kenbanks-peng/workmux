//! Controlled-server history contracts, not evidence of terminal emulation.
use super::*;

type Step = (&'static str, Value, Option<Value>);

fn history_steps() -> Vec<Step> {
    vec![
        snap(snapshot()),
        snap(snapshot()),
        snap(snapshot()),
        (
            "pane.layout",
            json!({"pane_id":"p1"}),
            Some(
                json!({"result":{"layout":{"panes":[{"pane_id":"p1","rect":{"width":80,"height":24}}]}}}),
            ),
        ),
        (
            "pane.get",
            json!({"pane_id":"p1"}),
            Some(
                json!({"result":{"pane":{"scroll":{"viewport_rows":24,"max_offset_from_bottom":1500}}}}),
            ),
        ),
    ]
}

fn batch(steps: &mut Vec<Step>, first: usize, last: usize, revision: Option<u64>, text: String) {
    steps.push(("pane.copy_motion", json!({"pane_id":"p1","cursor":{"row":last,"col":0},"motion":"line_end","content_revision":revision}), Some(json!({"result":{"cursor":{"row":last,"col":79},"content_revision":42}}))));
    steps.push(("pane.selection.read", json!({"pane_id":"p1","anchor":{"row":first,"col":0},"cursor":{"row":last,"col":79},"content_revision":42}), Some(json!({"result":{"text":text}}))));
}

#[test]
fn history_over_1000_preserves_batches_and_trims_blank_viewport() {
    let mut steps = history_steps();
    let mut rows: Vec<_> = (0..1500).map(|i| format!("row-{i:04}")).collect();
    rows.extend(vec![String::new(); 24]);
    for first in (0..1524).step_by(256) {
        let last = (first + 255).min(1523);
        batch(
            &mut steps,
            first,
            last,
            (first != 0).then_some(42),
            rows[first..=last].join("\n"),
        );
    }
    let probe = Probe::new(steps);
    let key = probe.backend.key("term1").unwrap();
    let output = probe.backend.capture_pane(&key, 1500).unwrap();
    assert_eq!(output, rows[..1500].join("\n"));
    assert_eq!(output.lines().next(), Some("row-0000"));
    assert_eq!(output.lines().last(), Some("row-1499"));
    probe.finish();
}

#[test]
fn history_copy_unwrapping_recovers_physical_rows() {
    let mut steps = history_steps();
    let mut rows: Vec<_> = (0..1500).map(|i| format!("row-{i:04}")).collect();
    rows.extend(vec![String::new(); 24]);
    // Native selection can join soft-wrapped rows. The adapter must split
    // that batch and return physical rows, not silently lose a row.
    batch(&mut steps, 0, 255, None, rows[..256].join(""));
    batch(&mut steps, 0, 127, Some(42), rows[..128].join("\n"));
    batch(&mut steps, 128, 255, Some(42), rows[128..256].join("\n"));
    for first in (256..1524).step_by(256) {
        let last = (first + 255).min(1523);
        batch(
            &mut steps,
            first,
            last,
            Some(42),
            rows[first..=last].join("\n"),
        );
    }
    let probe = Probe::new(steps);
    let key = probe.backend.key("term1").unwrap();
    assert_eq!(
        probe.backend.capture_pane(&key, 1500).unwrap(),
        rows[..1500].join("\n")
    );
    probe.finish();
}

#[test]
fn history_revision_change_discards_partial_output() {
    let mut steps = history_steps();
    batch(
        &mut steps,
        0,
        255,
        None,
        vec!["first batch"; 256].join("\n"),
    );
    steps.push(("pane.copy_motion", json!({"pane_id":"p1","cursor":{"row":511,"col":0},"motion":"line_end","content_revision":42}), Some(json!({"error":{"code":"stale_content_revision"}}))));
    let probe = Probe::new(steps);
    let key = probe.backend.key("term1").unwrap();
    assert!(probe.backend.capture_pane(&key, 1500).is_none());
    probe.finish();
}
