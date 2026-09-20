# Pane split and dimension queries

## Cases added

Test file: `src/multiplexer/herdr/dimensions_tests.rs`.
Live runner: `src/multiplexer/herdr/integration/dimensions_checks.py`.

- Controlled server: both split axes query fresh geometry after an earlier 80-cell dimension query. An 8-cell request is rejected after the target shrinks to 8, 1, or 0 cells. The fixture puts an unrelated rectangle first to check target selection.
- Controlled server: percentage splits of zero-cell and one-cell targets are rejected on both axes. All ten cases prohibit allocation or other mutation requests.
- Live server: attach a real Herdr UI to a private PTY, query dimensions, resize that PTY, then reject an absolute split based on the old dimensions. Pane count does not change.
- Live server: horizontal, vertical, then horizontal splits produce a nested four-pane layout. Checks cover positive child dimensions, unchanged sibling dimensions, stable tab membership, and pane count. The deepest split uses an absolute cell size.
- Live server: a 50% horizontal split at the two-cell boundary creates two positive-width panes. Removing that test-owned child restores the two-cell target.
- Live server: shrink the nested target to 1x1. Absolute and percentage split requests are rejected without allocation. Percentage rejection covers both axes.

## Small defect fixed

Before the size check, a live 50% horizontal split of a one-cell target succeeded but produced a **zero-width child**. The test failed with actual dimensions `(0, 1)` rather than `(1, 1)`.

`src/multiplexer/herdr/mod.rs` now rejects splits when the queried split axis has fewer than two cells, before launch allocation. The two-cell acceptance test checks that this guard does not reject the valid 50% boundary. No other production files changed.

## Validation

All final Cargo commands used `CARGO_BUILD_JOBS=1`. Earlier interrupted builds are not acceptance evidence.

```sh
CARGO_BUILD_JOBS=1 cargo test dimensions_tests -- --nocapture
```

Result: **1 passed, 0 failed, 1 ignored**, 1775 filtered out. The ignored test requires the private live runner; it is not a controlled-server pass. Missing live socket configuration returns an error rather than a silent pass.

```sh
CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr::
```

Final result: **33 passed, 0 failed, 14 ignored**, 1730 filtered out; 1.18 seconds. This includes the controlled dimension test. Other ignored live probes were not run.

The compiled test binary in this worktree was `target/debug/deps/workmux-3c4143229c1a27ae`. Cargo prints the current path in its `Running unittests` line; its hash can change with the build configuration.

```sh
for run in 1 2 3; do
  python3 src/multiplexer/herdr/integration/dimensions_checks.py \
    target/debug/deps/workmux-3c4143229c1a27ae || exit
done
```

Final result: **three separate private-server runs passed**. Each run: **1 passed, 0 failed, 0 ignored**, 1776 filtered out. Durations: 1.82, 1.93, and 1.73 seconds. Each printed:

```text
HERDR_DIMENSIONS_PASSED old=114 resized=40 minimum=1x1
```

```sh
ruff check src/multiplexer/herdr/integration/dimensions_checks.py
ruff format --check src/multiplexer/herdr/integration/dimensions_checks.py
rustfmt --edition 2024 --check src/multiplexer/herdr/dimensions_tests.rs
git diff --check
```

Results: all passed; Python formatter reported one file already formatted.

Initial fixture runs failed on inherited nonblocking socket mode and missing snapshot fields. These fixture defects were corrected. The first live wait also assumed the full PTY width was available to panes; the Herdr sidebar invalidated that assumption. The wait now accepts sidebar-reduced geometry. No unresolved test failure remains.

## Evidence boundary and safety

Controlled evidence uses a private Unix socket with scripted responses. It establishes adapter validation and fresh queries, not real terminal geometry.

Live evidence uses installed **Herdr 0.9.0, protocol 22**, the existing `HerdrServer` fixture, and a real Herdr UI in a test-owned PTY. The fixture isolates HOME, XDG paths, configuration, socket, process, and targets. Only that PTY is resized. The runner stops its child test process before fixture cleanup, including on failure. It never selects the inherited main server.

## Remaining gaps

- A resize *inside* `split_pane`, after its final dimension read but before the server processes `pane.move`, is not deterministically tested. The live test covers resize between the caller's earlier query and the split call, not atomic size preservation during an in-flight mutation.
- Two-cell successful splitting is tested horizontally at 50%, not vertically or at every supported ratio. Small-layout rounding at asymmetric ratios remains untested.
- No SSH or runtime blocker prevented these local live tests. Other platform rows and ignored probes remain outside this assignment.

## Proposed Table 2 cells

- **Tested:** Dimensions; split sizes; invalid limits; unsupported stacked splits; fresh dimension queries after resize; no allocation on invalid tiny targets; live two-cell 50% split; live mixed-axis nested splits and unchanged siblings.
- **Untested:** Resize after the final dimension read but before split mutation; vertical two-cell success; asymmetric-ratio rounding in minimum layouts.
- **Percentage:** 95% (estimate, not line coverage).
- **Coverage marker:** `◐` (partial; retain `class="unknown"`).

`STATUS.html` and `README.md` were not changed.
