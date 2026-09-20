# Live pane metadata and process identity

## Scope and files

This report covers one row in STATUS.html, Table 2. No production code changed.

- `src/multiplexer/herdr/process_metadata_tests.rs`: two controlled-server tests and one live integration test.
- `src/multiplexer/herdr/platform_tests.rs`: three-line registration; reuses the existing controlled-server fixture.
- `src/multiplexer/herdr/integration/process_metadata_checks.py`: dedicated live test runner.

## Cases added

### Controlled-server evidence

- A successful process response with an omitted `shell_pid` returns `Missing Herdr shell PID`.
- A successful process response with `shell_pid: null` returns the same error.
- Discovery returns metadata for one terminal. Before an input action, the server reuses its pane ID for a different terminal. The old terminal handle is rejected. No input request is sent to the replacement.

These tests establish adapter behavior, not live terminal behavior.

### Live-server evidence

The runner starts a disposable Herdr 0.9.0 instance with protocol 22. It uses private HOME, XDG paths, socket, state and configuration. It does not use the inherited main server. Cleanup stops only the instance that the runner created.

The test:

1. Creates a test-owned shell and discovers its PID and terminal handle.
2. Uses a file gate to let that shell exit after discovery.
3. Checks that the process identity is no longer live, metadata is absent, and input through the old handle is rejected.
4. Creates a replacement shell. Checks that it has a distinct terminal handle and a live process identity. The old handle still returns no metadata and cannot send input.
5. Checks that the replacement shell accepts input.
6. Replaces the shell program with `exec /bin/sleep 60`. Checks that refreshed metadata reports `sleep` with the same PID.
7. Sends Ctrl-C through the discovered replacement handle and waits for the terminal to disappear.

No extra agent, main-server target, global configuration or installed software was used.

## Validation

All final commands ran from the worktree root on macOS. Cargo used `CARGO_BUILD_JOBS=1` after the resume instruction.

| Exact command | Result |
| --- | --- |
| `CARGO_BUILD_JOBS=1 cargo test process_metadata_tests` | 2 passed, 0 failed, 1 ignored, 1775 filtered out. Exit 0. Initial resumed validation. |
| `CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr::platform_tests` | Final run: 7 passed, 0 failed, 1 ignored, 1770 filtered out. Exit 0. |
| `for i in 1 2 3 4 5; do CARGO_BUILD_JOBS=1 python3 src/multiplexer/herdr/integration/process_metadata_checks.py || exit $?; done` | Five fresh private instances. Each run: 1 passed, 0 failed, 0 ignored, 1777 filtered out. All exit 0. Test times: 0.96, 0.79, 0.91, 0.86 and 0.95 seconds. |
| `CARGO_BUILD_JOBS=1 cargo fmt --check` | Exit 0. |
| `ruff check src/multiplexer/herdr/integration/process_metadata_checks.py` | All checks passed. Exit 0. |
| `git diff --check` | Exit 0. |

The ignored Rust test is executed explicitly by the Python runner. The runner checks the test count so an incorrect test filter cannot produce a false pass.

### Earlier incomplete or failed runs

- Before the resume instruction, Cargo builds used two jobs. Several tool calls timed out during dependency compilation. The agent tabs were then closed. These attempts are not passing evidence.
- The first resumed live run passed. A second live run failed with `pane_not_found`: the shell exited between the metadata snapshot and the process query. The test's exit polling incorrectly required every intermediate query to succeed. Polling now accepts only this specific transient error, and still requires a later successful result with no metadata. Other errors fail the test. No adapter behavior changed.
- The first lint run found a missing explicit `check` argument on a subprocess call. This was corrected before final validation.

## Remaining limits

No runtime requirement blocked the final tests. Missing-PID success responses and pane-ID reuse are controlled-server evidence only.

The live tests cover exit between caller discovery and a later action, and an exec replacement with the same PID. They do not force exit or replacement in the smaller interval between an action's final internal snapshot and its input RPC. They do not force kernel PID reuse. The full repository suite was not run.

## Proposed table cells

- **Tested:** Retain the current evidence. Add: successful process responses with omitted/null shell PID are rejected; reused pane IDs cannot retarget old terminal handles (controlled server). Shell exit after discovery rejects later input; a new shell cannot be reached through the old handle; exec replacement refreshes the command with the same PID and accepts Ctrl-C through the existing handle (private live Herdr 0.9.0, protocol 22).
- **Untested:** Forced live exit/replacement between an action's final internal snapshot and its input RPC; kernel PID reuse. Missing-PID success and pane-ID reuse have controlled-server evidence only.
- **Percentage:** 95%, a conservative estimate, not a measured code-coverage value.
- **Coverage marker:** `◐` with class `unknown`. Keep partial coverage because the narrow in-action race is not forced by these tests.
