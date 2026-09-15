# Herdr parity tasks

## Goal and baseline

Support as much Workmux-on-tmux functionality as possible with Workmux-on-Herdr. Keep almost all implementation changes in `src/multiplexer/herdr/`. Permit small shared changes where a command check or interface prevents adapter support. Do not report a shared restriction as an adapter-only task.

Baseline: [`STATUS.html`](STATUS.html), including its 2026-09-14 test scope, and [`SUPPORT.md`](src/multiplexer/herdr/SUPPORT.md). Target: unmodified Herdr 0.9.0 / protocol 22. This list comes from the status report and code inspection; no new live tests were run to prepare it.

Priority: P0 fixes a defect that affects many workflows; P1 restores core behavior; P2 extends coverage or needs a larger shared change. Tasks are ordered within each priority.

For each task:

- Keep server-lifetime checks, process identity checks, and ownership checks. Do not accept an inherited pane ID or matching label as proof of ownership.
- Do not claim tmux identity, bypass command checks, replace occupied layouts, or change user configuration to obtain support.
- Add adapter unit tests and private-server integration checks. Where a support probe currently expects failure, change it to require successful behavior after the fix.
- Update `STATUS.html` and `SUPPORT.md` only for behavior that was verified. A passing restriction probe is not feature support.

## P0 — Reliable status and agent tracking

### 1. [ ] Fix default status target routing

**Gap:** Normal Herdr status updates return success without applying the update. The workaround requires removal of `WORKMUX_STATUS_*` and explicit backend selection. This affects native status, registration, dashboard, wait, run, reaping, and agent hooks.

**Evidence:** `src/multiplexer/mod.rs::setup_panes` supplies Herdr status targets. `src/command/set_window_status.rs::StatusTarget::from_values` accepts only tmux and Zellij; `run_for_status_target` logs the rejection and returns success. The adapter already implements live target validation and native reports.

**Work:** Accept Herdr in explicit status target validation. Check the adapter's endpoint and boot-qualified terminal identity through launch, registration, and subsequent hooks. Retain fail-closed handling for stale or invalid targets. Do not remove target variables as the permanent fix: explicit targets are needed when hook ancestry is unavailable.

**Scope:** Small shared change in `src/command/set_window_status.rs`, with validation tests there; adapter changes and live tests in `src/multiplexer/herdr/`.

**Acceptance:** An ordinary agent launch, without the workaround, registers and reports working, waiting, done, and clear. Native reports and Workmux state agree. Wait releases on done; run retains output and exit status. Targets from another endpoint or server lifetime cannot update a different pane. Test moved terminals, closed terminals, and hooks without native environment variables.

## P1 — Core behavior

### 2. [ ] Restore worktrees after a Herdr server restart

**Gap:** Closed-tab restore works, but restore after stop/start fails with “Failed to create window in session”, even when a replacement parent workspace exists.

**Evidence:** `integration/support_checks.py::resurrect` reproduces the failure. The relevant adapter path is `session_exists` → `create_window_in_session` → `new_tab`; shared setup adds the outer error. The underlying cause is not established by that message.

**Work:** Capture the full error on the restart probe. Check workspace resolution, new server identity, tab allocation, launch guards, and ownership records. Use the replacement workspace only as a creation destination; never treat old pane or ownership records as valid in the new server. If saved parent selection requires a shared recovery change, keep it small and explicit.

**Scope:** Adapter-first diagnosis and fix; a shared recovery change is conditional on the cause. Inspect `src/workflow/setup.rs`, `src/workflow/open.rs`, and `src/workflow/resurrect.rs` without assuming that they need edits.

**Depends on:** Task 1 for recovery records created without a status workaround.

**Acceptance:** Restore succeeds after restart at the same socket with a replacement parent workspace. Test missing and ambiguous parents, repeat restore, and partial failure. Failed restore retains recovery data; successful restore produces fresh pane identities. Old cleanup requests cannot close new resources.

### 3. [ ] Resolve native popup-script callers

**Gap:** A native popup script cannot run `add` without explicit `--parent-session`, although popup caller resolution exists in the adapter.

**Evidence:** `integration/support_checks.py::popup`; `HerdrBackend::caller` and `popup_caller`. The latter checks the server ancestor, launcher process identity, endpoint, and `HERDR_ACTIVE_PANE_ID`.

**Work:** Reproduce direct popup commands and script/shell wrappers. Correct caller resolution for the verified launcher chain. Preserve endpoint and process checks; do not use the globally focused pane as caller identity. Use the resolved caller for workspace selection and navigation context.

**Scope:** Adapter and adapter tests. This does not install tmux popup bindings or enable shared commands that reject Herdr.

**Acceptance:** Popup-launched `add` and `open` select the correct workspace without an explicit parent. Test shell wrappers, spaces in paths, moved source panes, closed source panes, stale inherited variables, and multiple workspaces. An unrelated external process must not become a verified caller.

### 4. [ ] Clear waiting and done status on focus

**Gap:** Waiting and done remain after focus moves away and returns. `set_status_state` ignores its auto-clear argument; no focus acknowledgement runtime is installed.

**Evidence:** `HerdrBackend::set_status_state`, `set_status`, and `clear_status`; `integration/support_checks.py::focus`. The tmux reference is `src/multiplexer/tmux.rs::set_status` and its focus hook.

**Work:** Add an adapter-owned focus observer using supported protocol facilities. First confirm which events or snapshot fields can identify real focus transitions. If a helper must outlive the command, keep its implementation in the adapter and bound it to the endpoint and server lifetime. Acknowledge only the status version observed on focus; do not clear a newer working update. Check tmux behavior for an update to an already focused pane.

**Scope:** Adapter runtime, tests, and existing state APIs. A private helper dispatch entry may require a small shared registration change. Do not change Herdr itself or install a user plugin.

**Depends on:** Task 1.

**Acceptance:** With an attached UI, waiting and done clear according to tmux acknowledgement behavior; working does not. Native display and Workmux tracking remain consistent. Test manual focus, Workmux navigation, multiple clients, concurrent updates, pane moves, and restart. No duplicate or orphan observer remains.

### 5. [ ] Enable session mode through the workspace adapter

**Gap:** Workspace operations pass adapter tests, but `add` and `open` reject `--session`, `--mode session`, and session-mode configuration.

**Evidence:** tmux-only checks in `src/workflow/create.rs` and `src/workflow/open.rs`. Herdr already implements workspace create, switch, rename, close, and window creation within a workspace.

**Work:** Replace the shared backend-name restrictions with a narrow capability check, or an equally small explicit support change. Keep workspace behavior in the adapter. Verify the complete session lifecycle before enabling the capability; direct workspace tests alone are insufficient.

**Scope:** Adapter plus small shared workflow/interface changes. This cannot be completed in the adapter alone.

**Depends on:** Tasks 1 and 2 for complete status and recovery coverage.

**Acceptance:** Exercise all three mode selectors; multi-window configuration; foreground/background launch; open and mode conversion; rename; close; remove; merge cleanup; and resurrect. In-pane cleanup must navigate safely before closing the workspace. Workmux sessions map to workspaces, not Herdr named servers.

### 6. [ ] Support explicit parents for external multi-worktree creation

**Gap:** `add --count 2` works from a native pane, but external calls cannot supply the parent workspace that the adapter requires. Multiple-worktree `open` has a related restriction.

**Evidence:** `src/command/add.rs` rejects `--parent-session` together with multi-worktree generation; `src/command/open.rs` rejects it when opening multiple worktrees. `HerdrBackend::create_window` requires a verified caller unless shared setup supplies a parent.

**Work:** Separate parent selection from the single-worktree `--target-name` restriction. Pass one explicit parent to each generated worktree. Retain unique target naming and existing validation for unrelated options. Do not infer a parent from arbitrary server focus to evade the command restriction.

**Scope:** Small shared command/option propagation changes; workspace placement and ownership remain in the adapter.

**Depends on:** Task 1 for agent tracking; task 5 only for session-mode variants.

**Acceptance:** External `--count`, multiple `--agent`, `--foreach`, supported stdin generation, and multi-worktree `open` use the requested workspace. Test duplicate labels, naming collisions, background behavior, partial launch failure, and ownership-safe cleanup.

## P2 — Navigation, coverage, and larger extensions

### 7. [ ] Provide safe active-pane context for external navigation

**Gap:** External `last-agent` switches once but cannot record the return pane. Native-pane two-way navigation already works.

**Evidence:** `src/command/last_agent.rs` reads `active_pane_id` before switching. Herdr's `active_pane_id` currently delegates to ancestry-based `current_pane_id`.

**Work:** Keep caller identity separate from UI focus. Determine whether protocol 22 can select one unambiguous attached client's active pane for an external navigation request. Implement that read-only context in the adapter if supported. Otherwise record the exact protocol or small interface change needed; do not select an arbitrary client. Share focus observation with task 4 where useful.

**Scope:** Adapter-first; external client selection may require a small shared interface change or remain blocked by protocol support.

**Acceptance:** External two-way toggle preserves the return pane with one attached UI. Test multiple clients, no UI, closed targets, pane moves, and server restart. Navigation context must not authorize status updates or destructive operations.

### 8. [ ] Verify sandbox-to-Herdr identity and status routing

**Gap:** Real guest launch, guest-to-Herdr RPC, and guest clipboard transfer are unverified. Shared sandbox/RPC test counts do not establish Herdr support.

**Evidence:** `STATUS.html`; `src/command/sandbox_run.rs` selects the backend and reads `current_pane_id`; `SUPPORT.md` explicitly excludes verified Herdr sandbox identity routing.

**Work:** Add a private-server test path with a real supported sandbox runtime when one is available. Trace the host supervisor's endpoint and pane identity through guest registration, status, input, and exit. Fix demonstrated adapter defects first. Treat any missing shared identity transport as a separate, narrow change rather than weakening identity validation.

**Scope:** Adapter integration tests and conditional adapter fixes; shared sandbox changes only if a test proves they are needed. Docker, Podman, and Lima were unavailable in the recorded assessment.

**Depends on:** Task 1.

**Acceptance:** A real guest updates only its host Herdr pane; wait/run observe completion; guest exit and stale targets are safe. Test guest RPC and clipboard transfer separately. Use a controlled clipboard fixture, not user clipboard contents.

### 9. [ ] Close the remaining adapter integration coverage gaps

**Gap:** Several supported or partial rows cover only basic paths or agent stubs. These are verification gaps, not confirmed adapter defects.

**Work and acceptance:** Extend `integration/run.py` and `integration/support_checks.py`, in this order:

1. After task 1, rerun status-dependent hooks, dashboard, wait, run, multiple agents, continue/fork, and reaping without the workaround.
2. Test reaping with Ctrl-D fallback and an unresponsive agent. A surviving process must not be reported as successfully removed.
3. Test merge conflicts, squash, in-pane merge cleanup, and rebase conflicts. Failed Git operations must leave panes and worktrees available.
4. Test dashboard actions beyond Enter, diff send/commit/merge, and patch split/comments. Check the destination pane, input content, focus, output capture, and resulting Git state.
5. Test real supported agent continuation and fork, including Codex fork, when credentials and fixtures are available. Keep agent CLI/history failures separate from adapter failures.

**Scope:** Adapter-local integration tests; fix only demonstrated adapter gaps. Shared tests may be reused, but their previous success is not a Herdr end-to-end result.

### 10. [ ] Assess and implement the minimum shared interface for Workmux sidebar

**Gap:** The Workmux sidebar explicitly rejects Herdr. Herdr's native sidebar is not equivalent.

**Evidence:** `src/command/sidebar/mod.rs`; `SUPPORT.md` states that full sidebar support requires shared interface and command changes. Retained layout/recovery work is not integrated or verified support.

**Work:** First inventory the sidebar's direct tmux operations: pane layout and geometry, client attachment, hooks/refresh, runtime state, and recovery. Define the smallest backend interface that supports the required behavior. Implement Herdr operations under the adapter. Reuse retained work only after validation against protocol 22. Do not remove the backend check before the dependencies work.

**Scope:** Larger shared change, unlike the tasks above. Keep this last because it conflicts most with the adapter-local goal. If the required refactor is too large, record the boundary and leave the feature explicitly unsupported.

**Depends on:** Tasks 1 and 4; task 7 where client-specific focus is needed.

**Acceptance:** Show, hide, refresh, resize, and recover the Workmux sidebar without terminating or replacing user panes. Test multiple workspaces/clients, native layout changes, reconnect, restart, and cleanup. Verify Workmux content and actions, not only Herdr's native display.

## Not adapter feature tasks

- **Remote PR/MR creation:** External tool and remote-service verification remains open. No current evidence identifies an adapter defect.
- **Host clipboard implementation:** Uses OS tools, not Herdr. Guest transfer verification belongs to task 8.
- **Rename messages that say “tmux”:** Small shared wording cleanup, not an adapter capability gap.
- **Headless restrictions shared with tmux:** No parity gap unless the desired behavior changes.
- **Already verified core operations:** Keep regression coverage for create/open/list/close/remove, deferred cleanup, splits, input/capture, focus/zoom, file sync, and basic Git workflows. Do not rebuild them merely because other rows remain partial.
