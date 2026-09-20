# Focus observation and user acknowledgement

## Scope and cases added

Assigned Table 2 row: **Focus observation and user acknowledgement**.
Assigned gap: **Programmatic focus must not count as user acknowledgement**.

Test file: `src/multiplexer/herdr/integration/acknowledgement_checks.py`.
Run this file directly. It uses the existing `HerdrServer`, `Fixture`,
`launch_status_agent`, `assert_status`, and `FocusEvents` helpers. No shared
registration or production changes were needed.

Four executable live integration tests cover:

- No UI client.
- One attached UI client.
- Two attached UI clients.
- Both UI clients closed before programmatic focus.

Each test covers `pane.focus`, `tab.focus`, `workspace.focus`, and
`workmux open feature`, for each of `waiting`, `done`, and `working`.
This gives 48 state/operation/client cases. Each case checks:

1. A status update on an already-focused pane remains set.
2. Focus moves to a separate test-owned workspace.
3. The tested operation returns focus to the agent pane and emits focus events.
4. Workmux JSON status and native Herdr status remain set during a bounded
   observation interval (at least 0.5 seconds).
5. Repeated focus on the same target also preserves status.

Each test ends with a positive control: explicit `set-window-status clear`
clears Workmux JSON status. Native status after explicit clear is not asserted:
Herdr can fall back to detected agent status after Workmux authority is removed.

## Exact validation commands and results

Commands ran from the repository root. All final commands exited 0.

```sh
CARGO_BUILD_JOBS=1 cargo build
```

Result: `Finished dev profile [unoptimized + debuginfo] target(s) in 47.49s`.
Build log: `/tmp/workmux-acknowledgement-build.log`.

```sh
WORKMUX_HERDR_LOG_DIR=/tmp/workmux-acknowledgement-evidence-final python3 src/multiplexer/herdr/integration/acknowledgement_checks.py
```

Result: **4 tests passed in 140.330s**. All 48 subtest cases passed.
Log: `/tmp/workmux-acknowledgement-tests-final.log`.
Four `focus-protocol.json` files contain 12 recorded focus cases each.
Snapshots report Herdr **0.9.0**, protocol **22**.

```sh
WORKMUX_HERDR_LOG_DIR=/tmp/workmux-acknowledgement-existing python3 src/multiplexer/herdr/integration/support_checks.py focus focus-protocol
```

Result: both existing restriction/capability checks passed. Their output retains
`BLOCKED focus acknowledgement`: the protocol still cannot prove user
acknowledgement. Log: `/tmp/workmux-acknowledgement-existing.log`.

```sh
ruff check src/multiplexer/herdr/integration/acknowledgement_checks.py
ruff format --check src/multiplexer/herdr/integration/acknowledgement_checks.py
git diff --check
```

Results: `All checks passed!`; `1 file already formatted`; no whitespace errors.

### Earlier attempts

The previous session's Cargo attempts did not establish a successful build.
Two calls timed out (120 and 600 seconds); the last call had no result before
the session was interrupted. Validation above used a new build with one job.

The first live run used:

```sh
WORKMUX_HERDR_LOG_DIR=/tmp/workmux-acknowledgement-evidence python3 src/multiplexer/herdr/integration/acknowledgement_checks.py
```

Result: 4 failures in 140.811s, all at the final explicit-clear control.
All focus-preservation subtests passed. The initial control wrongly expected
native `unknown` after clear. Native Herdr instead reported detected agent
`claude` as `idle`. The backend removes Workmux authority on clear, so the
control now checks the authoritative Workmux JSON status. No production fix
was needed. The complete corrected suite passed as recorded above.

The initial lint run also found a loop-variable binding warning and formatting
differences. Both were corrected before final validation.

## Evidence boundaries and safety

**Live evidence:** all four new tests and both existing checks used real Herdr
0.9.0 servers. UI tests used the real Herdr client in test-owned PTYs. Each test
used a separate temporary HOME, XDG directories, configuration, socket, and
repository. Only fixture-owned clients and servers were closed. No inherited
main server, user tab, or other agent target was used.

**Controlled evidence:** the agent process is a test stub that calls the real
registration hook. Status transitions are issued through real Workmux CLI
commands. This is not proof of a particular coding agent's event hooks. No fake
Herdr server was used. No human keyboard acknowledgement is claimed by the
new tests.

## Remaining gaps and blockers

- Protocol 22 focus events have no reliable user/client origin. API calls can
  emit them with no UI attached.
- The existing live protocol check confirms manual focus changes snapshots
  but does not emit the same focus events. Snapshot focus survives detach.
- Safe automatic acknowledgement of `waiting`/`done` after real user attention
  remains blocked. These tests establish false-acknowledgement prevention,
  not that acknowledgement is implemented.
- Observation is bounded, not proof against arbitrary delayed changes. No
  dashboard/sidebar acknowledgement path is added by this test.
- The full test suite was not run; validation was limited to this row.

## Proposed Table 2 cells

- **Tested:** API focus events; manual focus; two clients; detach. Live regression
  tests prove pane/tab/workspace API focus and Workmux open preserve
  waiting/done/working with zero, one, or two clients and after detach, including
  already-focused updates and repeated focus. Explicit clear still works.
- **Untested:** Reliable user-origin acknowledgement and safe automatic status
  clearing remain blocked by protocol 22; programmatic focus preservation is
  now tested.
- **Percentage:** **80%** (coverage estimate, not a measured code coverage value).
- **Coverage marker:** **◐**, class **unknown**. Do not mark this row complete.
