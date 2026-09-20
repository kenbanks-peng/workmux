# Ownership records and guarded target removal

## Scope and files

Assigned row: **Ownership records and guarded target removal**, STATUS.html,
Table 2. The previous estimate was 90%.

- Tests: `src/multiplexer/herdr/ownership_interruption_tests.rs`
- Module registration: `src/multiplexer/herdr/mod.rs` (two lines)
- No production behavior changes. No changes to STATUS.html or README.md.

## Cases added

1. `ownership_record_commit_failure_allows_same_owner_retry`
   - Force the final tab-record persist to fail, after the terminal owner is saved.
   - Check that the saved terminal owner remains valid, another owner is refused,
     and no close request occurs.
   - Remove the storage fault, retry the same owner, and execute guarded cleanup.
   - Check unrelated workspace, tab, and terminal survival.
2. `cleanup_capture_disconnect_allows_safe_retry`
   - Drop a process-info response during cleanup command preparation.
   - Check that preparation fails without a close request.
   - Retry preparation and execute the resulting worker command successfully.
   - Cover both tab and workspace targets; check unrelated targets survive.
3. `interrupted_target_removal_retries_without_touching_unrelated_targets`
   - Drop a close response before applying removal, then retry the same command.
   - Drop a close response after applying removal, then retry the same command.
   - The first case completes cleanup on retry. The second refuses the missing
     target without a second close request. This is safe refusal, not a successful
     idempotent return value.
   - Cover both tab and workspace targets; check unrelated targets survive.
4. `interrupted_close_retry_rejects_replacement_target`
   - Apply removal but lose its response. Reuse the container ID with a different
     terminal before retry.
   - Check that retry refuses changed contents without another close request.
   - Cover both tab and workspace targets; check replacement and unrelated
     targets survive.

## Evidence type and safety

These are **controlled-server** regression tests. Each test has a private Unix
socket and a disposable state directory. Child-test isolation sets
`XDG_STATE_HOME` without changing the parent process environment. The fixture
uses the real socket peer lifetime and the test process identity. It supplies
controlled protocol responses, including deliberate connection loss.

The tests execute the production ownership methods and embedded Python cleanup
worker through `/bin/sh`. The owned-record setup uses existing internal backend
helpers. The fixture checks each close target and rejects any attempt to close
an unrelated or replacement container. Snapshot checks also confirm survival.

No live Herdr instance was used. No terminal process survival is established by
these tests. The inherited server, user workspace, and other agents' targets
were not used or changed. No software was installed and no global configuration
was changed.

## Exact validation commands and results

After the interrupted session was resumed, all Cargo commands used one build job.

```sh
CARGO_BUILD_JOBS=1 cargo test ownership_interruption_tests -- --nocapture
```

Passed: **4 passed, 0 failed, 0 ignored, 1775 filtered out** (0.43 s test time).

```sh
CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr:: -- --nocapture
```

Final run passed: **36 passed, 0 failed, 13 ignored, 1730 filtered out**
(1.02 s test time). This includes existing controlled cleanup, identity, and
worker-survival tests. The 13 ignored tests require isolated live-server runs;
they were not run and are not counted as passed.

```sh
CARGO_BUILD_JOBS=1 cargo fmt --check
git diff --check
```

Both passed (exit 0). The first format check found module registration order;
that order was corrected before the final checks and test run.

Before the session interruption, two build attempts with
`CARGO_BUILD_JOBS=2 cargo test ownership_interruption_tests -- --nocapture`
reached tool time limits during dependency compilation. A third attempt had no
returned result. None is counted as passed. The resumed validation above is the
source of the test results.

## Remaining gaps and blockers

- Live isolated-server interruption tests are not added or run here. Real terminal
  and unrelated process survival across these faults remains unverified.
- The storage test injects a persist failure. It does not kill a process during
  serialization, atomic rename, or a multi-terminal ownership update. Disk-full,
  power-loss, and durable recovery are not established.
- The scheduling test interrupts command preparation. It does not kill the
  scheduler between capture and detached-worker launch. Existing worker-survival
  coverage is separate from that launch-boundary gap.
- Close-response loss is controlled fault injection, not an actual Herdr server
  crash. Server restart during removal and all possible native race windows
  remain outside this added evidence.
- There was no missing-runtime blocker for the controlled tests. Live testing was
  not attempted; no claim is made that the live runtime is unavailable.

## Proposed Table 2 cells

- **Tested:** Cleanup races; stale identity; extra panes; worker survival after
  scheduler exit. Controlled-server tests: partial ownership persist failure and
  same-owner retry; cleanup capture disconnect; close-response loss before and
  after removal; replacement-target refusal; unrelated-target survival on retry.
- **Untested:** Live interruption during ownership recording, worker launch and
  target removal; process termination inside record updates or detached launch;
  disk/power-loss recovery and real terminal survival across these faults.
- **Percentage:** **90%** (retain the estimate until live interruption and launch
  boundary coverage is available; this is not a measured coverage percentage).
- **Coverage marker:** **◐**, `class="unknown"` (partial).
