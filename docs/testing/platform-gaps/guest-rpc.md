# Guest-to-host RPC operations

## Scope and cases

Test file: `src/multiplexer/herdr/integration/guest_rpc_checks.py`.
Run this standalone unittest module after building Workmux. No shared test
registration change is required.

- **Spawn:** the guest `add` command creates a worktree and a tab in the
  supervisor's Herdr workspace. Background focus stays on the parent tab.
  A command in the new terminal checks its directory and shell response.
  A duplicate spawn fails without a second tab.
- **Exec:** the guest `host-exec` command returns exact stdout, stderr, host
  working directory, and exit code 7. A command outside the allowlist returns
  127 and does not create its file.
- **Merge:** the guest merges a committed change into main. The main worktree
  receives the file. The source worktree and tab are removed. The parent
  terminal remains.
- **Close:** the guest closes the target tab, keeps the worktree, and leaves
  the parent shell usable.
- **Authentication:** a wrong token cannot create a worktree or tab.

## Evidence boundary and safety

The tests start a **real private Herdr server** with the existing `HerdrServer`
fixture. Each case owns its HOME, XDG paths, configuration, repository, server,
and terminal targets. The fixture does not inherit the main server endpoint.
The tests run the real Workmux guest CLI, supervisor, authenticated TCP RPC,
host child commands, Git, and Herdr terminal operations.

The **container launcher is controlled**, not live. A private `docker` script
captures the guest variables from the real supervisor and waits for test
completion. Guest CLI processes run locally with those variables and no host
Herdr identity. The script neither starts nor stops a real container. It is
placed only in the private server's PATH. Herdr responses are not mocked.
Host-exec sandbox enforcement is explicitly disabled in the private config.

This proves local guest-mode RPC integration with live Herdr. It does not
prove container or VM networking, mounts, isolation, SSH, or runtime launch.
No global configuration or software installation is required.

## Validation

Environment: macOS; Herdr 0.9.0, protocol 22 (checked by the fixture).

| Exact command | Result |
| --- | --- |
| `CARGO_BUILD_JOBS=1 cargo build --quiet` | Exit 0. |
| `python3 src/multiplexer/herdr/integration/guest_rpc_checks.py -v` | Final run: 5 tests passed in 8.373 seconds. Exit 0. |
| `python3 src/multiplexer/herdr/integration/guest_rpc_checks.py -v` | Repeat: 5 tests passed in 8.190 seconds. Exit 0. |
| `ruff check src/multiplexer/herdr/integration/guest_rpc_checks.py` | All checks passed. Exit 0. |
| `ruff format --check src/multiplexer/herdr/integration/guest_rpc_checks.py` | 1 file already formatted. Exit 0. |
| `python3 -m py_compile src/multiplexer/herdr/integration/guest_rpc_checks.py` | Exit 0. |
| `git diff --check` | Exit 0. |

The original two-job build was interrupted when the session closed. It is not
counted as passed. The resumed build used one job.

Earlier runs of the same integration command exposed test setup errors:
5 tests with 10 failure records (the fixture used a main worktree where a
linked worktree was required); then 3 failures (the detached supervisor lost
its verified terminal ancestry); then 1 failure (shell output redirection
captured only the second command). The fixture now uses a linked worktree,
a foreground supervisor, and grouped shell output. These were test fixture
errors, not product defects. No production files changed. The final and
repeat runs have no failing or skipped cases.

## Combined branch validation before final integration

Merged `herdr` at `dd9e4538` into this branch without conflicts. Combined
commit tested: `80614b30`. All other agents' changes were retained. The only
STATUS.html edit is the Guest-to-host RPC operations row in Table 2.

| Exact command | Combined result |
| --- | --- |
| `CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr::` | Exit 0: 71 passed, 0 failed, 22 ignored, 0 measured, 1730 filtered out; 4.20 seconds. |
| `CARGO_BUILD_JOBS=1 cargo fmt --check` | Exit 0; no output. |
| `CARGO_BUILD_JOBS=1 cargo build --quiet` | Exit 0; rebuilt the combined binary for the Python suite. |
| `python3 src/multiplexer/herdr/integration/guest_rpc_checks.py -v` | Exit 0: 5 passed in 10.244 seconds; no failures or skips. |

The Rust filter does not run ignored live-server tests or the full Rust test
suite. The Python suite uses private live Herdr instances and the controlled
launcher described above. Broad pre-merge hooks are intentionally skipped
for the authorized final Workmux merge. No full-suite pass is claimed.

## Remaining gaps and proposed table cells

Real container/VM execution of these four operations remains untested. This
suite uses a controlled launcher by design. No claim is made that a real
runtime or SSH is unavailable on this host; neither was used. Exec filesystem
sandbox enforcement is outside this suite. Merge conflicts, alternate merge
options, and self-close/self-merge are not covered here.

Proposed table cells:

- **Tested:** Existing heartbeat, status, title, clipboard and authentication
  evidence; guest-mode spawn, exec, merge and close through real TCP RPC to
  private live Herdr. Spawn placement, background focus and shell execution;
  exec streams, exit code and allowlist; merge Git effects and cleanup; close
  retains the worktree; rejected token. Container launcher is controlled.
- **Untested:** These four operations from a real container or VM, including
  runtime networking and filesystem boundaries; self-close/self-merge and
  alternate merge paths.
- **Percentage:** **75% (estimate)**. Retain partial credit because all four
  new operations use a local guest-mode process, not a real sandbox guest.
  This is not a measured code-coverage percentage.
- **Coverage marker:** **◐**, CSS class `unknown` (partial).

No live container/VM acceptance claim or 100% claim is proposed.
