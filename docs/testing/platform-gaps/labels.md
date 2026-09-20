# Session, window, and pane labels

## Cases added

### Controlled server

`src/multiplexer/herdr/label_tests.rs` tests the backend interface:

- Session rename forwards an empty label and a long Unicode label exactly.
- Window rename, by name and by pane identity, forwards those labels exactly.
- Both operations return the server's `invalid_label` error for each label.
- The long label is `"界🙂"` repeated 4,096 times: 8,192 Unicode characters and 28,672 UTF-8 bytes.
- The request script also checks that there are no extra requests or retries.
- Session ownership records use an isolated child process and private state directory.

Small registration changes are in `src/multiplexer/herdr/mod.rs`. The tests reuse the controlled-server helper in `src/multiplexer/herdr/platform_tests.rs`. Only helper visibility changed in that file. No production behavior changed.

### Live server

`src/multiplexer/herdr/integration/label_checks.py` runs four executable tests against Herdr 0.9.0, protocol 22:

- Empty and long Unicode session labels survive a snapshot read exactly.
- Empty and long Unicode window labels survive a snapshot read exactly.
- Pane labels preserve the same long Unicode value, 65,536 ASCII bytes, and 262,144 ASCII bytes without truncation.
- An empty pane label clears the label. A later rename succeeds.
- For sessions, windows, and panes, a 1,048,576-byte ASCII label request fails at the transport layer. The previous label remains unchanged. A later rename on a new connection succeeds.

Each test starts and closes its own `HerdrServer` instance. That fixture owns a private HOME, XDG directories, configuration, socket, process, and targets. The tests do not attach to the inherited server or change existing tabs. They do not create agent tabs.

These are live native-protocol state tests. They do not prove visual rendering in an attached terminal client. The controlled tests prove backend forwarding and structured error propagation, not live server acceptance.

## Validation

Commands run from the worktree root after the host-load pause:

| Command | Exact result |
| --- | --- |
| `CARGO_BUILD_JOBS=1 cargo test label_tests -- --nocapture` | 2 passed; 0 failed; 0 ignored; 1,775 filtered out; test time 0.04 s; exit 0. |
| `CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr::platform_tests -- --nocapture` | 5 passed; 0 failed; 0 ignored; 1,772 filtered out; test time 0.08 s; exit 0. |
| `python3 src/multiplexer/herdr/integration/label_checks.py` | 4 tests passed; 3.739 s; exit 0. |
| `rustfmt --edition 2024 --check src/multiplexer/herdr/label_tests.rs` | Passed; exit 0. |
| `ruff check src/multiplexer/herdr/integration/label_checks.py` | All checks passed; exit 0. |
| `ruff format --check src/multiplexer/herdr/integration/label_checks.py` | 1 file already formatted; exit 0. |
| `git diff --check` | Passed; exit 0. |
| `herdr --version` | `herdr 0.9.0`; exit 0. |

Earlier Cargo attempts used `CARGO_BUILD_JOBS=2`, as originally instructed. They were interrupted during compilation and are not test evidence. The first completed controlled run found an extra snapshot in the window-by-name test script (1 passed, 1 failed). The script was corrected; the backend needed no fix.

The first live run had 2 errors: the oversized request could time out during send, rather than disconnect. The test now accepts either bounded transport outcome and requires unchanged state and successful recovery. A later run before the pause passed all 4 tests in 34.877 s. The post-pause run above is the final live evidence. Python formatting was then applied; no test logic changed.

## Remaining gaps and blockers

- The exact largest accepted label and request sizes are not established. The tests establish accepted sample sizes and non-acceptance at 1 MiB, not an exact boundary.
- A 1 MiB request can disconnect or time out. This is not evidence of a structured live `invalid_label` response. Structured rejection propagation is tested only with a controlled server.
- Visual clipping and rendering of long labels in an attached terminal client are not tested.
- No runtime, SSH, or shell blocker prevented the focused live tests. Earlier build interruptions were caused by host load; focused Cargo validation is now complete.

## Proposed Table 2 cells

- **Tested:** Rename; Unicode; shell syntax treated as data; exact empty/long Unicode label forwarding and structured rejection propagation for sessions, windows, and panes (controlled server); empty/long session and window labels; pane label clearing and exact values through 262,144 bytes; unchanged labels and recovery after 1 MiB requests fail (isolated live Herdr 0.9.0).
- **Untested:** Exact label/request size boundary; structured live label rejection; visual rendering of long labels.
- **Percentage:** 95% (estimate, not a measured coverage statistic).
- **Coverage marker:** `◐` with class `unknown`.

`STATUS.html` and `README.md` were not changed.
