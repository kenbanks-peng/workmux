# Literal input, paste, submit, and control keys

## Cases added

A real terminal process reads input in raw mode on a private Herdr server. The
Rust test calls `HerdrBackend::send_key` for each key, then checks the exact bytes
received by that process:

| Adapter key | Received bytes (hex) |
| --- | --- |
| Space (` `) | `20` |
| Backspace (`BSpace`) | `7f` |
| Ctrl-C (`C-c`) | `03` |
| Ctrl-D (`C-d`) | `04` |
| Ctrl-Z (`C-z`) | `1a` |
| Enter (`enter`) | `0d` |
| Escape | `1b` |
| Tab | `09` |
| Up | `1b 5b 41` |

Each check compares the full input stream, including a delayed check for duplicate
bytes or an extra submit. The test also checks that the target PID and foreground
tab stay unchanged. The receiver is in a background workspace. The Python runner
checks the final stream separately and fails if the Rust test did not run.
Missing Rust fixture variables cause an error, not a passing test.

## Test files

- `src/multiplexer/herdr/input_keys_tests.rs`: ignored live integration test.
- `src/multiplexer/herdr/integration/input_keys_checks.py`: executable runner,
  raw terminal receiver, private server setup and cleanup.
- `src/multiplexer/herdr/mod.rs`: two-line test module registration.

No production behavior changed. No changes were made to `STATUS.html` or
`README.md`.

## Validation

Host: Darwin arm64. Live server: Herdr 0.9.0, protocol 22.

1. `CARGO_BUILD_JOBS=1 python3 src/multiplexer/herdr/integration/input_keys_checks.py`
   - Exit 0. Rust result: **1 passed; 0 failed; 0 ignored; 1775 filtered out**.
   - All nine per-key checks passed. Final full-stream assertion passed.
   - Rust test duration: 1.56 seconds (does not include build/server setup).
2. `CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr::platform_tests::key_aliases_and_control_keys_use_native_encoding -- --exact`
   - Exit 0. **1 passed; 0 failed; 0 ignored; 1775 filtered out**.
   - Duration: 0.05 seconds. This is controlled-server evidence only.
3. `CARGO_BUILD_JOBS=1 cargo fmt --check`
   - Exit 0.
4. `python3 -m py_compile src/multiplexer/herdr/integration/input_keys_checks.py`
   - Exit 0.
5. `ruff check src/multiplexer/herdr/integration/input_keys_checks.py`
   - Exit 0. `All checks passed!`
6. `git diff --check`
   - Exit 0.

The first pre-resume build attempt reached a 1200-second tool timeout. A later
pre-resume attempt had no result before the user closed the agent tabs. Neither
attempt counts as test evidence. The results above are from fresh validation
after resume, with one Cargo job. The runner also enforces one Cargo job.

## Controlled versus live evidence

The existing `platform_tests::key_aliases_and_control_keys_use_native_encoding`
test checks native request encoding against a controlled server. It does not
prove terminal receipt.

The new runner starts the real Herdr binary with a temporary HOME, XDG paths,
configuration and socket through the existing `HerdrServer` fixture. Only its
test-owned server and panes are used and removed. The terminal receiver reads
from its real PTY; input is not simulated by a fake server. The Rust adapter
sends each tested key. Native input is used only to start the receiver.

## Remaining gaps and scope

No assigned key remains untested. No runtime blocker remains for this host.
These tests prove byte receipt, not shell editing, job control or signal delivery:
raw mode deliberately prevents Ctrl-C/D/Z from interrupting the receiver.
Up is checked in normal cursor mode, not application cursor mode. Linux and
other terminal modes were not tested in this run. Existing literal, Unicode,
multiline paste, submit and missing-target evidence was not rerun by this runner.

## Proposed Table 2 cells

- **Tested:** Preserve the existing evidence. Add: “Live terminal receipt of
  space, backspace, Ctrl-C/D/Z, Enter, Escape, Tab and Up through the adapter;
  exact received bytes; background focus and target PID retained (Herdr 0.9.0,
  Darwin arm64, raw terminal, normal cursor mode).”
- **Untested:** None in the assigned key-receipt gap. Shell signal/job-control
  effects, application cursor mode and Linux are outside this receipt test.
- **Percentage:** `100%` for the listed row cases and assigned gap, not a claim
  of all-platform or all-terminal-mode coverage.
- **Coverage marker:** `<td class="yes">✓</td>`.
