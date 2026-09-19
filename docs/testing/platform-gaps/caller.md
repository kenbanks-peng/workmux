# Current caller and target discovery

## Cases added

Six live cases test the backend discovery methods through a dedicated Rust probe:

- Pane caller with native caller environment values removed.
- Pane caller with stale pane addresses from a test-owned workspace that was closed.
- Pane caller with addresses for another live pane and an invalid Workmux pane hint.
- External caller with each of those three environment states. Each must return no caller, not the last focused pane.

The pane cases check terminal, window, and session IDs; window and session names; active pane; and caller working directory. The external cases check absent IDs and names and an error for caller working directory.

Two real Herdr UI clients run in private PTYs. The second client changes focus seven times in each pane case, for 21 confirmed changes. The probe continuously resolves the caller during these changes. A file handshake also requires a successful discovery check after each confirmed focus state. Each case ends with focus on the other pane.

## Test files

- `src/multiplexer/herdr/caller_tests.rs`: ignored Rust integration probe. Missing fixture variables cause an error, not a silent pass.
- `src/multiplexer/herdr/integration/caller_checks.py`: standalone private-server driver.
- `src/multiplexer/herdr/mod.rs`: two-line test module registration.

No production behavior changed. `STATUS.html` and `README.md` were not changed.

## Exact commands and results

Run from the worktree root. All completed Cargo commands after the coordinator resumed this agent used one build job.

```sh
CARGO_BUILD_JOBS=1 cargo test --no-run --message-format=json > /tmp/workmux-caller-build.jsonl 2>/tmp/workmux-caller-build.err
```

Passed, exit 0. Final Rust probe build completed in 8.66 seconds.

```sh
CARGO_BUILD_JOBS=1 cargo fmt --check
ruff check src/multiplexer/herdr/integration/caller_checks.py
ruff format --check src/multiplexer/herdr/integration/caller_checks.py
git diff --check
herdr --version
```

All checks passed, exit 0. Ruff reported `All checks passed!` and `1 file already formatted`. Herdr reported `herdr 0.9.0`.

```sh
CARGO_BUILD_JOBS=1 python3 src/multiplexer/herdr/integration/caller_checks.py /Users/kenbanks/Software/Repos/forks/workmux__worktrees/test-gap-caller/target/debug/deps/workmux-3c4143229c1a27ae
```

Passed, exit 0. Final output:

```text
PASS external/missing
PASS pane/missing: 7 second-client focus changes; CALLER_FOCUS_CHECKS=8
PASS external/stale
PASS pane/stale: 7 second-client focus changes; CALLER_FOCUS_CHECKS=9
PASS external/conflicting
PASS pane/conflicting: 7 second-client focus changes; CALLER_FOCUS_CHECKS=9
```

The loop counts can vary. The seven acknowledged focus-state checks per pane case are required independently of these counts. All six Rust probe invocations passed. Two earlier live runs also passed; the first preceded the per-focus-state handshake. An initial Ruff check found nine lint errors in the new driver. These were corrected before final validation. Earlier two-job builds were interrupted before completion and are not pass evidence.

For a fresh build, the standalone entry point is:

```sh
CARGO_BUILD_JOBS=1 python3 src/multiplexer/herdr/integration/caller_checks.py
```

This no-argument entry point uses the existing integration build helper. Final validation used the explicit test executable above, not this build entry point.

## Evidence type and safety

**Live evidence:** Herdr 0.9.0, protocol 22, a private backend instance, a native shell caller, external processes, and two real UI clients. The existing `HerdrServer` fixture owns the socket, HOME, XDG directories, configuration, PTYs, and server process. All closed workspaces and focus targets belonged to that fixture. The driver closes only its private instance.

**Controlled-server evidence:** None added. These results do not depend on a fake server.

The inherited main server and other agents' targets were not used. No software was installed and no global configuration was changed.

## Remaining gaps and blockers

- Missing, stale, or conflicting popup launcher information was not tested here. Existing ordinary popup evidence is not proof of these failure cases.
- The concurrent live focus test changes panes within one window. Concurrent focus changes across windows or workspaces remain untested.
- Caller exit or removal during discovery, and conflicting server process metadata, remain outside this new live matrix.
- No runtime blocker remains for the six cases above. The full integration suite was not run.

## Proposed Table 2 cells

- **Tested:** Pane, popup, and external callers; no last-focus fallback. Live pane and external callers with missing, stale, and conflicting environment hints; caller identity and target discovery during 21 confirmed focus changes from a second real UI client.
- **Untested:** Popup launcher information faults; caller discovery during cross-window or cross-workspace focus changes; caller removal during discovery.
- **Percentage:** 95% (estimate, not measured code coverage).
- **Coverage marker:** `◐` (`class="unknown"`). Keep partial coverage; do not claim 100%.
