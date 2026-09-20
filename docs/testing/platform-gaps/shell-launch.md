# Shell launch, readiness, and cancellation

## Scope and files

Assigned row: Table 2, **Shell launch, readiness, and cancellation**.

- Tests: `src/multiplexer/herdr/shell_launch_tests.rs`.
- Registration: two lines in `src/multiplexer/herdr/mod.rs`.
- No production behavior changes.

The tests use the `Launch` and `PaneHandshake` interfaces. Each launcher runs
in a new process session and process group. Each test owns its files and child
processes. The child environment does not contain the inherited Herdr socket.
No Herdr server is contacted. No user tabs or global configuration are changed.

## Cases added

1. **Startup failure:** a test-owned shell executable exits with status 37.
   Readiness must fail, the launcher must retain status 37, and later command
   delivery must fail. The command marker must not exist. This covers all eight
   accepted shell names: sh, bash, zsh, dash, ksh, ash, fish, and nu.
2. **Delayed readiness:** a test-owned startup gate blocks the shell. The
   readiness wait must remain pending. After gate release, the real shell must
   report readiness, accept a command, create its marker, and exit successfully.
3. **Cancellation:** dropping an unused handshake during startup must stop its
   launcher. Explicit cancellation must unblock a pending readiness wait.
   Cancellation after readiness must also stop the launcher. Cancelled launches
   must reject delivery and must not create the command marker.

The delayed-readiness and cancellation cases use real sh, bash, zsh, dash,
ksh, fish, and nu runtimes. The fixture disables user configuration where the
shell provides a command-line option. It uses a temporary HOME and configuration
directory. File paths contain spaces and apostrophes.

Ash startup-failure injection needs no ash runtime. The two real-ash tests are
explicitly ignored because ash is not installed on this host. To run them on a
host with ash:

```sh
CARGO_BUILD_JOBS=1 cargo test shell_launch_tests::ash -- --ignored --nocapture --test-threads=1
```

For the other shells, an unavailable runtime emits `NOT RUN` under `--nocapture`.
A runner success alone is not evidence for such a runtime.

## Validation

The previous build was interrupted when the agent tabs closed. Its log was
absent after resume. It provides no validation evidence. All results below are
from fresh commands after resume, with one Cargo build job.

| Command | Result |
| --- | --- |
| `CARGO_BUILD_JOBS=1 cargo test shell_launch_tests -- --nocapture --test-threads=1` | Exit 0. 22 passed; 0 failed; 2 ignored; 0 measured; 1775 filtered out. Test time: 10.64s. |
| Same command, second run | Exit 0. 22 passed; 0 failed; 2 ignored; 0 measured; 1775 filtered out. Test time: 10.21s. |
| `CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr::pane_launch::tests -- --nocapture --test-threads=1` | Exit 0. 1 passed; 0 failed; 0 ignored; 0 measured; 1798 filtered out. Test time: 0.23s. |
| `CARGO_BUILD_JOBS=1 cargo fmt --all -- --check` | Exit 0. No formatting differences. |
| `git diff --check` | Exit 0. No whitespace errors. |

Neither new-test run emitted `NOT RUN`. Both ignored cases were the explicit
ash runtime tests. The shell runtimes found after resume were:

- sh: `/bin/sh`
- bash: `/opt/homebrew/bin/bash`
- zsh: `/opt/homebrew/bin/zsh`
- dash: `/bin/dash`
- ksh: `/bin/ksh`
- fish: `/opt/homebrew/bin/fish`
- nu: `/Users/kenbanks/.local/share/mise/installs/aqua-nushell-nushell/latest/nu-0.115.1-aarch64-apple-darwin/nu`

The full repository test suite and live Herdr tests were not run.

## Evidence limits and remaining gaps

- **Controlled-process evidence:** all new tests execute generated launch scripts
  and the real file handshake. Startup failure and startup delay are injected
  at the shell executable boundary. They are not failures in user login files.
- **Controlled-server evidence:** none added. These tests do not use a fake server.
- **Live-server evidence:** none added or rerun. These child processes have no
  terminal PTY. This change does not prove live Herdr terminal behavior.
- The real ash runtime is unavailable. Its delayed-readiness and cancellation
  cases remain untested on this host.
- A live isolated Herdr shell matrix, shell-specific login-file failures, and
  startup delays past the production timeout remain untested by these additions.
- These tests target controlled command launch. They do not add coverage for
  initial-shell release or move-gate behavior.

## Proposed Table 2 cells

- **Tested:** Readiness handshake; controlled launch; cancellation; cleanup after
  setup failure. Added isolated process tests for startup failure across eight
  shell launch names; delayed readiness, command delivery, and cancellation
  before, during, and after readiness with sh, bash, zsh, dash, ksh, fish, and nu.
- **Untested:** Live Herdr shell matrix; ash runtime delay and cancellation;
  shell login-file failures; startup delays past the production timeout.
- **Percentage:** 85% (estimate, not a measured coverage ratio).
- **Coverage marker:** `◐`, class `unknown`. Do not mark complete.
