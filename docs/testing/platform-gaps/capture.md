# Terminal output capture

## Scope and test files

Table 2 row: **Terminal output capture**.

- `src/multiplexer/herdr/capture_contract_tests.rs`: three controlled-server tests at the capture adapter interface.
- `src/multiplexer/herdr/capture_tests.rs`: one ignored live-server probe. Missing input variables cause an error, not a silent pass.
- `src/multiplexer/herdr/integration/capture_checks.py`: executable runner for three private-server cases. It invokes the Rust probe and compares every output byte and row.
- Registration only: two lines each in `mod.rs` and `platform_tests.rs` in the same Herdr directory.

No production behavior changed. `STATUS.html` and `README.md` were not changed.

## Cases added

### Controlled-server evidence

1. Capture 1,500 history rows across 256-row batches. Remove the 24 blank viewport rows without losing output.
2. Recover physical rows when a selection response joins soft-wrapped rows. Split and read that batch again.
3. Reject partial output when the server reports a stale content revision after the first batch. Subsequent reads must use the first revision.

These tests use a socket server with fixed responses. They do not prove live terminal behavior.

### Live-server evidence

All three cases use the real Herdr 0.9.0 server, shell PTY, UI client, and Rust capture adapter.

1. **History above 1,000 rows:** emit 2,200 numbered rows and an end marker. Compare exact tails for requests of 1,000, 1,001, 1,024, and 2,000 rows.
2. **Large-output truncation:** emit 9,000 numbered 120-character rows and an end marker (1,089,012 bytes). Compare exact tails of 1, 10, 1,001, and 4,096 rows. This exercises recent reads and multi-batch history reads.
3. **Terminal sequences:** emit 200 blocks containing SGR color, carriage return with erase-line, backspace overwrite, cursor-left overwrite, OSC title, OSC-8 hyperlink, tab expansion, and UTF-8 text. Compare rendered output for 9-row and 1,001-row captures. Escape sequences must not appear as literal text.

Each case starts a separate disposable server with private HOME, XDG paths, configuration, socket, workspace, pane, and UI PTY. Only that private PTY is resized. Cleanup runs in `finally`. No inherited endpoint, user tab, or other agent target is used.

## Exact validation commands and results

Run from the worktree root. Environment: Herdr 0.9.0, Python 3.14.7, rustc 1.98.0. Cargo used one build job after the resume instruction.

```sh
CARGO_BUILD_JOBS=1 cargo test capture --no-run --message-format=json \
  > /tmp/workmux-capture-build.jsonl 2>/tmp/workmux-capture-build.log
```

Passed. Build finished in 58.55 seconds. Test binary: `target/debug/deps/workmux-3c4143229c1a27ae`. The hash can differ on another host; use the executable field in the Cargo JSON output.

```sh
CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr::platform_tests:: -- --nocapture
```

Passed: **8 passed, 0 failed, 0 ignored**, including all three new controlled-server tests and the existing capture boundary test.

```sh
CARGO_BUILD_JOBS=1 cargo test capture -- --nocapture
```

Passed: **15 passed, 0 failed, 1 ignored**. The ignored test is the live probe; the runner below executes it explicitly.

```sh
WORKMUX_HERDR_LOG_DIR=/tmp/workmux-capture-live-fixed \
  python3 src/multiplexer/herdr/integration/capture_checks.py \
  --test-binary target/debug/deps/workmux-3c4143229c1a27ae

python3 src/multiplexer/herdr/integration/capture_checks.py \
  --test-binary target/debug/deps/workmux-3c4143229c1a27ae
```

Both runs passed, exit 0. Each run reported:

```text
PASS live history-above-1000: 4 exact captures
PASS live large-output-truncation: 4 exact captures
PASS live terminal-sequences: 2 exact captures
```

Thus each run executed three live probe invocations and ten exact capture comparisons. Optional server logs and fixture API traces from the first successful run are in `/tmp/workmux-capture-live-fixed`; they are local artifacts, not committed evidence.

```sh
ruff check src/multiplexer/herdr/integration/capture_checks.py
ruff format --check src/multiplexer/herdr/integration/capture_checks.py
rustfmt --check --edition 2024 src/multiplexer/herdr/capture_tests.rs src/multiplexer/herdr/capture_contract_tests.rs
python3 -m py_compile src/multiplexer/herdr/integration/capture_checks.py
git diff --check
```

All passed, exit 0.

### Earlier validation attempts

The pre-resume builds were interrupted or terminated. They are not passing evidence.

The first live run passed history but failed the 10-row large-output comparison. The fixture accepted the initial 120-column server layout before the UI attached. The UI then reduced the pane width, which wrapped the 120-character source rows. The fixture now sizes its own PTY to 180 columns and waits for a pane width of at least 140 before output starts. Both complete reruns passed. This was a test setup defect; no adapter fix was required.

## Remaining gaps and blockers

No runtime blocker remains for the assigned cases. Live coverage is representative, not exhaustive:

- Soft-wrap recovery is covered only by controlled responses, not a live soft-wrapped history case.
- Content changes during multi-batch reads are covered only by a controlled stale-revision response.
- Alternate-screen transitions, arbitrary terminal sequences, and maximum retained-history limits were not tested.

## Proposed Table 2 cells

- **Tested:** Keep existing evidence. Add: “History above 1,000 rows; 1,500-row batching, soft-wrap recovery and stale-revision rejection (controlled server). Exact live history tails up to 2,000 rows; truncation to 1/10/1,001/4,096 rows after more than 1 MiB of output; rendered SGR, CR/erase-line, backspace, cursor motion, OSC title/link, tabs and Unicode through recent and history reads (private Herdr 0.9.0 server).”
- **Untested:** “Live soft-wrap recovery and concurrent output during history reads; alternate-screen transitions; maximum retained-history limits.”
- **Percentage:** **95%** (estimate, not a code-coverage measurement).
- **Coverage marker:** **◐**, class `unknown`. Do not claim 100%.
