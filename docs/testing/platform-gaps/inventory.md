# Platform gap: session and window inventory

## Row

Session and window inventory, creation, and placement.

## Cases added

Test files:

- `src/multiplexer/herdr/inventory_tests.rs`
- `src/multiplexer/herdr/integration/inventory_checks.py`

The only registration change is in `src/multiplexer/herdr/mod.rs`.
No production behavior was changed.

### Controlled-server adapter evidence

- `parent_removed_after_lookup_does_not_retry_or_allocate_in_a_replacement`:
  the lookup returns a parent ID, but allocation returns `workspace_not_found`.
  The adapter returns that error. It sends exactly one allocation request, uses
  the original parent ID, and does not retry by label, change focus, or send a
  cleanup mutation. The server controls the error; this is not live evidence.
- `removed_placement_target_is_not_replaced_by_its_label`:
  discovery returns a placement target. The next snapshot replaces that target
  with a different ID and the same label. Creation fails before allocation.
  The test permits only the two snapshot requests; no allocation or move is sent.

### Live-server evidence

The Python runner uses the existing `HerdrServer` fixture. Each test has its own
server process, socket, HOME, XDG directories, shell, and targets. The fixture
stops only its own server. No main-server endpoint or existing agent target is
used. The Rust live test also requires the explicit inventory socket to be
inside its disposable working directory.

Environment: Darwin arm64, Herdr 0.9.0, protocol 22, `/bin/sh`.
No UI client is attached. These tests use real server processes and terminals;
they do not prove visual tab behavior or SSH behavior.

- `isolated_simultaneous_same_name_creation` runs through the workmux adapter.
  Two callers, each with a separate backend client, start at a barrier. Both
  create a window with the same name under one explicit parent. Returned terminal
  IDs and tab IDs must differ. Inventory must contain exactly two new tabs.
  Name-only lookup must fail as ambiguous. Background focus must not change.
- The same Rust test starts two session creators at a barrier. The current
  adapter uses a preflight name check, not an atomic name reservation. The test
  permits either two successful creations or one success and one `already exists`
  error. Every successful result must identify a distinct workspace present in
  inventory. Two successes require ambiguous name lookup. No other error is
  accepted. All three validated live runs reported `sessions=2`.
- Two Python tests call the native API concurrently for same-name workspaces and
  same-name tabs. They check distinct IDs, exact inventory count changes, parent
  placement for tabs, and unchanged focus. These are native API tests, not adapter
  tests. A barrier aligns caller starts; it does not force a server execution order.
- The native parent-removal test discovers a parent, closes that test-owned
  parent, and creates a same-label replacement. Allocation using the old ID must
  fail. Tabs, panes, and focus must remain unchanged. This verifies the live API
  behavior behind the controlled adapter test; it does not inject removal into
  a running adapter call.

## Commands and exact results

All final validation below was run after the session resumed. Earlier terminated
Cargo builds are not counted. Every resumed Cargo command used one build job.

```sh
CARGO_BUILD_JOBS=1 cargo test inventory_tests -- --nocapture
```

Result: **2 passed; 0 failed; 1 ignored; 1775 filtered out**. The ignored test is
explicitly run by the live fixture below. Final build time: 6.05 s; test time:
0.01 s.

```sh
CARGO_BUILD_JOBS=1 cargo test multiplexer::herdr::session_tests -- --nocapture
```

Result: **1 passed; 0 failed; 0 ignored; 1777 filtered out**, 0.01 s.

```sh
python3 src/multiplexer/herdr/integration/inventory_checks.py
```

Result: **4 tests run; 3 passed; 1 skipped**, 1.094 s. The skipped test requires
an explicit adapter test binary. This command alone is not adapter evidence.

```sh
python3 src/multiplexer/herdr/integration/inventory_checks.py \
  --adapter-test-binary target/debug/deps/workmux-3c4143229c1a27ae
```

Final result: **4 passed; 0 failed; 0 skipped**, 1.780 s. The nested Rust test
reported **1 passed; 0 failed; 0 ignored; 1777 filtered out**, 0.67 s, with
`HERDR_INVENTORY_CONCURRENT_PASSED sessions=2`. Two earlier resumed runs also
passed all four tests in 1.961 s and 1.824 s and reported `sessions=2`.
The executable hash is build-specific; use the executable path printed by Cargo
when repeating this command on another build.

```sh
ruff check src/multiplexer/herdr/integration/inventory_checks.py
ruff format --check src/multiplexer/herdr/integration/inventory_checks.py
rustfmt --edition 2024 --check src/multiplexer/herdr/inventory_tests.rs
git diff --check
```

Results: Ruff checks passed; one Python file already formatted; rustfmt and
Git whitespace checks passed.

## Remaining gaps and blockers

- Placement target removal **after** the adapter resolves its insertion index,
  including removal during allocation or before `tab.move`, remains untested.
  The added stale-target test covers removal before that validation, not after it.
- Parent removal after successful allocation, during ownership or shell setup,
  remains untested.
- The live adapter test does not force both session preflight snapshots to finish
  before either allocation. It covers concurrent callers and validates either
  allowed outcome, not every possible request order.
- No environment blocker remains for these tests. The remaining cases need
  deterministic request scheduling and further assertions. They are test gaps,
  not claims of unavailable SSH or runtime support.

## Proposed Table 2 cells

- **Tested:** Names; explicit parent; tab order; duplicate and ambiguous targets;
  background creation; simultaneous same-name creation (live adapter and API);
  removed-parent rejection (controlled adapter and live API); stale placement ID
  rejection before allocation (controlled adapter).
- **Untested:** Placement target removal after index resolution or during
  allocation/move; parent removal after allocation; forced session preflight race
  schedules.
- **Percentage:** 90% (estimate, not measured code coverage).
- **Coverage marker:** `◐` (`class="unknown"`).

`STATUS.html` and `README.md` were not changed.
