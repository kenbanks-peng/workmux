# Live pane moves

## Scope and test file

Table 2 row: **Live pane moves**. Previous estimate: 50%.

New executable test file:
`src/multiplexer/herdr/integration/pane_move_checks.py`

The file uses the existing `HerdrServer` fixture and Python `unittest`. Run it directly. It needs no Cargo build or extra Python packages.

## Cases added

1. Move a pane from a split source tab into a split destination tab in another workspace. Check terminal identity, destination, source peer placement, pane count, and all four running shells.
2. Request a move within a nested split in the same tab. Check the explicit `changed: false`, `reason: same_tab` result, unchanged layout, and all three shells. This is a native limit, not successful relocation.
3. Relocate a pane in a nested split through a temporary staging tab. Resolve the current terminal address between moves. Check the final layout geometry and all four shells.
4. Close the source pane after target resolution but before the move request. Check rejection, unchanged remaining layout, and surviving shells.
5. Close the destination pane after resolution, with another pane still in its tab. Check rejection without source loss, then retry against the live peer.
6. Close the destination tab after resolution. Check rejection without changes to the source split or its shells.
7. Submit a move without reading its response. Use a new connection and the stable terminal identity to find the completed move. Check the new address and all shells, then move back using the current address.

Shell checks send a command to each live terminal before and after the move. They compare the shell PID (`$$`) and a retained shell variable, and require new output. Snapshot identity alone is not the shell-liveness assertion.

## Exact validation commands and results

Environment: macOS; `herdr --version` returned `herdr 0.9.0`; `python3 --version` returned `Python 3.14.7`. The fixture requires protocol 22.

```sh
CARGO_BUILD_JOBS=2 python3 src/multiplexer/herdr/integration/pane_move_checks.py
```

Final suite: **7 passed**, `Ran 7 tests in 13.423s`, `OK`.
Independent repeat: **7 passed**, `Ran 7 tests in 13.756s`, `OK`.
Both runs exited 0. Output was redirected to `/tmp/workmux-pane-moves-test.log` and `/tmp/workmux-pane-moves-repeat.log` during validation.

```sh
ruff check src/multiplexer/herdr/integration/pane_move_checks.py
ruff format --check src/multiplexer/herdr/integration/pane_move_checks.py
git diff --check
```

Results: `All checks passed!`; `1 file already formatted`; no whitespace errors. All exited 0.

Initial test development found invalid test direction names and use of `pane_id` instead of `target_pane_id` for `pane.split`. These test errors were corrected. A test that expected direct same-tab relocation also failed: the native response explicitly reports `same_tab`. The final tests preserve this finding and separately test relocation through a staging tab. No production files were changed.

## Live versus controlled evidence

**Live:** All seven tests use the installed Herdr binary, its real private server, and real shell processes. Each test gets separate temporary HOME, XDG paths, configuration, socket, workspaces, and tabs. Cleanup stops only the server process owned by that test. No inherited server, user tab, or other agent target is used. No UI client is attached, so these results do not prove visual focus behavior.

**Controlled:** No fake server is used. The lost-response test controls the client connection: it shuts down its write side, never reads the response, waits for the committed move through another connection, then closes the original socket. This proves reconciliation after an unread acknowledgement. It does not prove interruption inside the server or an arbitrary disconnect before commit.

The recovery steps are performed by the test through the native API. They are not proof of automatic Workmux recovery.

## Remaining gaps and blockers

- Direct same-tab relocation is a native no-op in Herdr 0.9.0. The staging-tab path works in these tests.
- No server crash during mutation or client disconnect before commit is tested.
- No Workmux process crash and restart during a multi-step move is tested. Persisted automatic recovery and ownership restoration remain unproven here.
- Disappearance tests cover the deterministic boundary between resolution and mutation, not a concurrent removal inside the native move operation.
- No SSH or other remote runtime evidence was added. No local runtime requirement blocked these tests.

## Proposed Table 2 cells

- **Tested:** Terminal identity, PID, shell state, and input retained across split-tab moves; nested split relocation through staging; same-tab no-op; missing source pane, destination pane, and destination tab rejected safely; retry and identity-based reconciliation after a lost response on an isolated live server.
- **Untested:** Automatic Workmux recovery after process interruption; server crash or disconnect before commit; concurrent disappearance inside mutation. Direct same-tab relocation is unsupported (native no-op).
- **Percentage:** 80% (estimate, not measured code coverage).
- **Coverage marker:** `◐` with class `unknown`; keep partial coverage.
