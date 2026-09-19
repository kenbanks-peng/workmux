# Session, window, and pane focus; pane zoom

## Status

The live zoom/resize regression passes and remains in automatic discovery. The two-client case is an explicit diagnostic, not a passing regression. A bounded diagnosis found that server focus is not a valid barrier for client input routing. It did not establish a reliable client-ready barrier or prove a Herdr defect.

## Files and cases

- `src/multiplexer/herdr/integration/test_focus_zoom.py`
  - `FocusZoomTests.test_resize_while_zoomed_preserves_target_and_shell`: zoom the right pane, then resize the client to 30×100, 50×160, and 24×80. Check zoom, selected pane, layout size changes, and real shell dimensions with `stty size`. Unzoom and check split restoration, terminal identity, and both original shells. Size deltas account for native UI borders and the sidebar.
  - This is the only discovered test in this file.
- `src/multiplexer/herdr/integration/focus_zoom_diagnostic.py`
  - Explicit diagnostic with one-client and two-client modes. Send each focus request once. Wait for the requested server focus, then type a shell probe through the requesting UI. Check which live shell receives it. Record timestamps, observed pane IDs, input results, and snapshots after input in a JSON trace.
  - Requests are serialized, not simultaneous. No focus request or input is retried. Wrong-target input and timeouts return exit status 1. A passing invocation returns 0 but does not establish reliable support.
  - `--settle-seconds 0.25` and `--inspect-snapshots` are diagnostic timing controls, not test fixes. Neither is enabled by default.
  - Its filename excludes it from `test_*.py` discovery. No skips or expected-failure markers hide the limitation.

No production code or shared fixture change is included. The earlier startup connection-refusal retry was removed. A startup failure remains visible, not retried.

## Safety and evidence type

**Live-server evidence:** Herdr 0.9.0, protocol 22, Python 3.14.7 on macOS. Each invocation uses the existing `HerdrServer` fixture to create private HOME/XDG/config/socket directories under `/tmp`, a disposable server, real UI clients in owned PTYs, and shell panes. Cleanup stops only those owned resources. No inherited main-server socket or existing target is used.

**Controlled-server evidence:** none added. These checks use no fake server. They test native Herdr primitives, not a Workmux adapter command path. Existing adapter selection, zoom, and retained-background-focus coverage remains separate.

## Bounded focus diagnosis

### Predictions and observations

1. **A problem specific to two clients:** a one-client control should pass. This was not confirmed. The low-instrumentation one-client control timed out in 3/3 runs, all at the first left request. Thus, not every timeout requires two clients.
2. **Client state/input routing lags server focus:** timing changes should affect the result. This was observed. Extra snapshot round trips made all 12 initial instrumented runs pass. The equivalent low-instrumentation matrix below still failed. Snapshot queries can change the timing enough to obscure the symptom.
3. **The test accepts an unchanged old focus value:** the selected pane should already equal the next target. The wrong-input trace rules out this simple explanation for that request. Request 2 ended with the left pane selected. Request 3 observed a change to the right pane, but the next command still ran in the left shell. The server still reported right after the command.

Low-instrumentation matrix, three independent invocations per cell:

| Clients | Delay after server focus | Passed | Failed |
| --- | --- | --- | --- |
| 1 | 0 s | 0 | 3 focus timeouts |
| 1 | 0.25 s | 3 | 0 |
| 2 | 0 s | 2 | 1 wrong-target input |
| 2 | 0.25 s | 3 | 0 |

The delay is after the focus wait. It cannot directly explain why the first-request timeout did not recur in those separate delayed runs. Small samples and scheduling variation remain limits. No fixed delay was accepted as a valid test barrier.

Exact matrix command (executed before removal of the earlier fixture startup retry):

```sh
for clients in 1 2; do
  for delay in 0 0.25; do
    for run in 1 2 3; do
      CARGO_BUILD_JOBS=2 python3 src/multiplexer/herdr/integration/focus_zoom_diagnostic.py \
        --clients "$clients" --settle-seconds "$delay" \
        --trace "/tmp/workmux-focus-minimal-$clients-$delay-$run.json"
    done
  done
done
```

The original focus unittest was also re-run five times before removal from discovery: one pass, three wrong-input failures, and one timeout. Instrumentation did not establish a fix.

### Final explicit diagnostic

After removing the shared fixture retry:

```sh
for run in 1 2 3; do
  CARGO_BUILD_JOBS=2 python3 src/multiplexer/herdr/integration/focus_zoom_diagnostic.py \
    --trace "/tmp/workmux-focus-final-$run.json"
  printf 'diagnostic %s exit %s\n' "$run" "$?"
done
```

Results:

| Run | Exit | Result |
| --- | --- | --- |
| 1 | 1 | Focus timeout |
| 2 | 1 | Request 3 expected `right`, received `left` |
| 3 | 0 | Four requests passed |

Trace excerpt from run 2 (seconds from diagnostic start):

| Phase | Time | Server focus | Input result |
| --- | --- | --- | --- |
| Request 2 completed | 3.3306 | `w1:p1` (left) | left |
| Request 3 sent | 3.3307 | — | — |
| Request 3 observed | 3.4774 | `w1:p2` (right) | — |
| Request 3 input completed | 3.6567 | `w1:p2` (right) | **left** |
| Failure snapshot | 3.6989 | `w1:p2` (right) | — |

**Conclusion:** waiting for server focus is an insufficient test synchronization condition. Live input can reach a different pane after that condition holds. This is consistent with stale client input state, but the checks do not decode the rendered screen or inspect client internals. They cannot distinguish a stale render from stale input state, or establish which internal ordering causes it. The result is an observed limitation, not a proven Workmux or Herdr defect. Protocol 22 provides no per-client focus acknowledgement to replace the invalid barrier. No Herdr change, arbitrary-delay regression, or retry was added.

## Passing-suite validation

Run the independent zoom case:

```sh
PYTHONPATH=src/multiplexer/herdr/integration CARGO_BUILD_JOBS=2 \
  python3 -m unittest \
  test_focus_zoom.FocusZoomTests.test_resize_while_zoomed_preserves_target_and_shell -v
```

Run the discovered suite:

```sh
CARGO_BUILD_JOBS=2 python3 -m unittest discover \
  -s src/multiplexer/herdr/integration -p test_focus_zoom.py -v
```

Final five-repeat command:

```sh
for run in 1 2 3 4 5; do
  CARGO_BUILD_JOBS=2 python3 -m unittest discover \
    -s src/multiplexer/herdr/integration -p test_focus_zoom.py -v \
    > /tmp/workmux-zoom-only-$run.log 2>&1
  result=$?
  printf 'zoom %s exit %s: ' "$run" "$result"
  tail -4 /tmp/workmux-zoom-only-$run.log
  if [ "$result" -ne 0 ]; then exit "$result"; fi
done
```

| Run | Tests | Time | Exit | Result |
| --- | --- | --- | --- | --- |
| 1 | 1 | 3.938 s | 0 | OK |
| 2 | 1 | 5.181 s | 0 | OK |
| 3 | 1 | 4.218 s | 0 | OK |
| 4 | 1 | 2.607 s | 0 | OK |
| 5 | 1 | 2.706 s | 0 | OK |

The independent-case command above also passed: 1 test in 4.612 s, exit 0.

Final static checks:

```sh
ruff format --check src/multiplexer/herdr/integration/test_focus_zoom.py src/multiplexer/herdr/integration/focus_zoom_diagnostic.py
ruff check src/multiplexer/herdr/integration/test_focus_zoom.py src/multiplexer/herdr/integration/focus_zoom_diagnostic.py
git diff --check
```

Results: `2 files already formatted`; `All checks passed!`; no whitespace errors. All returned exit 0.

No Cargo build was needed. Earlier combined-suite validation also passed the zoom case 5/5; that combined suite was not green because of the focus failures.

## Validation after the coordinator pause

The worktree changes were inspected before resuming. No interrupted build was counted as a pass. No Cargo command was needed. The resumed checks used `CARGO_BUILD_JOBS=1`.

```sh
export CARGO_BUILD_JOBS=1
ruff format --check src/multiplexer/herdr/integration/test_focus_zoom.py src/multiplexer/herdr/integration/focus_zoom_diagnostic.py
ruff check src/multiplexer/herdr/integration/test_focus_zoom.py src/multiplexer/herdr/integration/focus_zoom_diagnostic.py
git diff --check
for run in 1 2 3; do
  python3 -m unittest discover -s src/multiplexer/herdr/integration \
    -p test_focus_zoom.py -v > /tmp/workmux-zoom-resumed-$run.log 2>&1
  result=$?
  printf 'run %s exit %s\n' "$run" "$result"
  tail -6 /tmp/workmux-zoom-resumed-$run.log
  if [ "$result" -ne 0 ]; then exit "$result"; fi
done
PYTHONPATH=src/multiplexer/herdr/integration CARGO_BUILD_JOBS=1 \
  python3 -m unittest test_focus_zoom.FocusZoomTests.test_resize_while_zoomed_preserves_target_and_shell -v
```

Results: formatting and lint passed; no whitespace errors. Discovery ran exactly one test per invocation: 1.374 s, 1.475 s, and 1.268 s, all exit 0. The independent case also passed in 1.337 s, exit 0. Historical `CARGO_BUILD_JOBS=2` commands above record earlier completed checks, not new build permission.

## Diagnostic run instructions

Run only the explicit two-client diagnostic:

```sh
CARGO_BUILD_JOBS=1 python3 src/multiplexer/herdr/integration/focus_zoom_diagnostic.py \
  --trace /tmp/workmux-focus-diagnostic.json
```

Compare one client, or add one timing control at a time:

```sh
python3 src/multiplexer/herdr/integration/focus_zoom_diagnostic.py \
  --clients 1 --trace /tmp/workmux-focus-one-client.json
python3 src/multiplexer/herdr/integration/focus_zoom_diagnostic.py \
  --settle-seconds 0.25 --trace /tmp/workmux-focus-delayed.json
python3 src/multiplexer/herdr/integration/focus_zoom_diagnostic.py \
  --inspect-snapshots --trace /tmp/workmux-focus-extra-snapshot.json
```

Optional `WORKMUX_HERDR_LOG_DIR=/tmp/workmux-focus-artifacts` retains fixture logs, API requests, and raw UI PTY output from owned instances. Raw output length alone is not proof of rendered focus. The diagnostic does not run the zoom test it imports for fixture setup.

## Remaining gaps and proposed Table 2 cells

- Reliable two-client focus/input behavior remains unvalidated. No valid passing regression was established within this bounded diagnosis.
- Competing targets across terminal windows or multiplexer sessions, truly simultaneous requests, unequal client sizes, and zoom resize through a Workmux command path remain outside this new coverage.
- No missing runtime, SSH access, or software installation blocked these checks. The blocker is the missing reliable client-ready observation and the timing-sensitive results.

Proposed cells:

- **Tested:** Selection; zoom; retained background focus; live zoomed client shrink/grow, shell dimensions, unzoom, and shell survival.
- **Untested:** Reliable two-client focus and input routing; competing targets across sessions/windows; simultaneous requests. Explicit diagnostic records wrong-target input and focus timeouts.
- **Percentage:** 85% (provisional; only the resize gap has passing new evidence).
- **Coverage marker:** `◐` (`unknown`). Not `✓` or 100%.

`STATUS.html` and `README.md` were not changed. No merge was done.
