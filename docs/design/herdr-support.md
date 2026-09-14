# Full workmux support on herdr

Status: full-feature architecture approved; an adapter-only subset is implemented.
Full feature support is not verified.

For the current implementation and restrictions, see the
[Herdr adapter support record](../../src/multiplexer/herdr/SUPPORT.md).
The adapter includes verified-terminal cleanup and live-terminal launch
ownership. Session-mode workflows, sidebar integration, and the other shared
changes described below remain outside the current implementation.

This design adds herdr support without copying workmux workflows or reducing their feature set. It extends the existing backend interface and separates the remaining direct tmux operations from shared code.

- [Domain terms](../../CONTEXT.md)
- [Decision record](../adr/0001-herdr-backend.md)
- [Feature and acceptance matrix](herdr-feature-matrix.md)

## 1. Agreed requirements

1. All workmux commands, options, and configuration features must work on herdr.
2. Use unmodified herdr **0.9.0**. Both the installed client and server reported version 0.9.0 and protocol 22 during inspection.
3. Do not require a herdr plugin, a herdr core change, or tmux inside herdr.
4. Preserve tmux behavior where herdr permits it. Do not silently replace sessions with tabs or windows with panes.
5. Most added implementation should be new backend code. Existing changes should be small and concentrated, but correctness takes precedence over a small diff.
6. Workmux may automatically start and manage its own background process. This process may remain active when the sidebar is closed.
7. Workmux may use a temporary tab during sidebar layout changes. Intermediate layouts or that tab may be visible. Running programs must remain intact, and workmux must restore the final layout and focus.
8. User-written tmux commands are not part of the requirement. There are no such commands to translate.
9. A missing feature is a blocker, not a successful no-op or an undocumented fallback. Any further unavoidable behavior difference requires a decision before it is accepted.

The architecture is approved. Full feature support is **not** yet verified.

## 2. Object mapping

| Workmux concept | tmux | herdr |
| --- | --- | --- |
| Backend instance | Server/socket | Named herdr session/server/socket |
| Multiplexer session | Session | Workspace |
| Terminal window | Window | Tab |
| Terminal pane | Pane | Pane |
| Window mode | Worktree window in a parent session | Worktree tab in a parent workspace |
| Session mode | Worktree session with windows | Worktree workspace with tabs |

A herdr named session is not the target of `workmux add --session`. That command must create a workspace. `--parent-session` must select a workspace for window mode. Sidebar and dashboard session filters must also use the workspace mapping.

Workmux remains responsible for Git worktree creation, branch selection, file operations, hooks, merge, and removal. Do not use herdr's `worktree.create` or `worktree.remove` as substitutes for these workflows.

## 3. Existing code and required changes

The inspected workmux baseline is commit `ae85d52e9686ac9324bdad2000355e06d8dbfe20`.

| Area | Existing situation | Required direction |
| --- | --- | --- |
| `src/multiplexer/mod.rs` | `Multiplexer` already covers most terminal operations | Add backend registration and only the missing semantic operations |
| `src/multiplexer/types.rs` | Backend selection and common target types | Add herdr and define identity/observation data where needed |
| `src/workflow/create.rs`, `open.rs` | Session mode explicitly requires the name `tmux` | Check session support instead; herdr must supply it |
| `src/command/sidebar/` | Direct tmux commands, hooks, options, layout parsing, and daemon observations | Route host operations through a sidebar host interface |
| `src/state/` | Some identity and process-lifetime checks are tmux-specific | Preserve those safeguards for herdr through explicit identity contracts |
| `src/command/set_window_status.rs` | Backend environment and ownership resolution have backend-specific paths | Add herdr caller resolution and safe status routing |
| `src/config.rs` and backend callers | Additional tmux assumptions require an audit | Replace functional assumptions, not every mention of the word tmux |
| `tests/conftest.py` | Shared test interface plus backend-specific environments | Add isolated herdr fixtures and port behavioral scenarios |

Retain shared Git, agent selection, prompt, sandbox, dashboard, diff, and sidebar rendering code. Do not rewrite those modules solely to add herdr. Retain existing tmux behavior; move or wrap its host operations with narrow changes.

## 4. Proposed modules

Names below are proposed implementation locations, not existing files.

### Herdr adapter

Put new backend implementation under `src/multiplexer/herdr/`:

- `mod.rs`: implement `Multiplexer` and the sidebar host interface.
- `client.rs`: socket requests, response decoding, errors, bounded timeouts, and event subscriptions.
- `identity.rs`: backend instance, server lifetime, terminal identity, and caller resolution.
- `layout.rs`: live-pane layout changes and restoration.
- `pane_launch.rs`: controlled command launch and handshake support.
- `runtime.rs`: background event handling, operation recovery, and lifecycle control.

Keep these internal details behind the adapter. Shared workflows must not construct herdr JSON, interpret herdr identifiers, or know about temporary tabs.

Use the installed protocol schema as the initial contract. Check the connected server, not only the CLI version. Do not upgrade or stop a user's server to resolve an unsupported protocol.

### Sidebar host interface

Add a focused interface, provisionally `src/multiplexer/sidebar.rs`, for operations currently embedded in sidebar modules:

- Observe live windows, panes, focus, dimensions, roles, and sidebar state.
- Make a window's sidebar match the requested scope, position, and size.
- Receive change notifications and recover a complete observation after a gap.
- Read and update backend-scoped sidebar settings and control state.
- Resolve the sidebar's host and navigate to an explicitly identified target.

Prefer semantic operations such as ensuring a sidebar layout over a generic command-string executor. Hide native layout syntax, hook installation, temporary tabs, and recovery inside the host implementation.

Tmux retains its native hooks and layout operations. Herdr uses socket events and live-pane moves. Shared sidebar rendering and agent/Git/PR processing remain common.

### Workmux background process

Extend or compose the existing daemon infrastructure instead of adding a second independent sidebar implementation. The exact process split can be selected during implementation, but it must not duplicate the Git/PR polling and rendering pipelines.

For herdr, the runtime must:

- Start automatically when persistent integration work is first required.
- Be unique per backend instance, with race-safe startup and ownership checks.
- Continue required status/event work without an open sidebar UI.
- Reconnect and obtain a fresh snapshot after event-stream failure.
- Reconcile rather than replay destructive operations blindly.
- Recover incomplete layout and deferred-cleanup operations.
- Stop cleanly when its server lifetime ends; never stop the herdr server.

Closing the sidebar must not disable focus acknowledgement or unrelated deferred cleanup. Preserve existing tmux daemon behavior unless a change is required and tested.

## 5. State and identity

Workmux state remains authoritative for workmux ownership, agent status, run records, and recovery. Herdr state describes the live terminal system. Native herdr agent/status displays may reflect workmux state, but they must not replace workmux's status model.

Herdr reports `working`, `blocked`, `idle`, and `unknown` through its agent-report request. A direct `done` report is not part of that request. Completion display also depends on whether a result has been seen. Preserve workmux's working/waiting/done semantics explicitly; do not infer them only from a native icon.

State must distinguish:

1. Backend kind and explicit server endpoint.
2. The server lifetime or another verified non-reuse identity.
3. A live terminal and its process identity.
4. Its current workspace, tab, and pane address.
5. Its workmux owner and role.

The herdr snapshot exposes `terminal_id`; the process query exposes `shell_pid`. These are useful evidence, not proof that every identifier survives every lifecycle. Verify rename, reorder, move, disconnect, and cold restart behavior before selecting persistence keys.

Do not trust labels as ownership. Do not treat inherited `HERDR_TAB_ID` or `HERDR_WORKSPACE_ID` as current after a pane moves. Resolve current location from verified live identity. Protect against stale status hooks, reused addresses, and multiple servers with identical public IDs.

Respect explicit backend targeting. Test mixed and nested multiplexer environments so inherited variables do not direct workmux to the wrong server.

## 6. Live sidebar layout changes

Herdr 0.9.0 `layout.apply` creates a new tab. When given an existing tab ID, it creates a replacement and closes the old tab. It does **not** preserve live terminal processes or scrollback. Never use that operation to rearrange occupied workmux tabs.

The verified alternative for a left sidebar is:

1. Observe the original layout, pane identities, focus, and zoom state.
2. Create a sidebar pane and a temporary tab in the same workspace.
3. Move the original panes into the temporary tab, leaving the sidebar in the original tab.
4. Move the original panes back in the order required to reconstruct the original content tree beside the sidebar.
5. Remove the temporary tab.
6. Restore focus, zoom, and the required final geometry.

The experiment verified steps 2–5 and layout restoration after sidebar removal. Attached-client focus and zoom restoration are still test requirements.

The final sidebar must span the window's full height on the left, or full width at the top, as applicable. Preserve content split proportions when the available content area changes. Test top position, repositioning, small terminal sizes, absolute and proportional sizing, and manual pane changes separately.

### Failure handling

Before the first move, persist a recovery record with verified ownership, original topology, target topology, and temporary resources. Serialize workmux layout operations for the affected tab and detect external changes.

On restart or failure:

- Re-read the live layout and identities before each recovery decision.
- Resume or restore from verified state.
- Do not close a temporary tab until it contains no user terminal that must survive.
- Do not close or recreate original user terminals to make cleanup easier.
- If safe recovery cannot be established, preserve the terminals and report the failure and remaining resources.

The journal format, external-change detection, and rollback behavior need tests. The successful layout experiment did not prove crash safety or atomicity.

## 7. Pane launch and input

Herdr has no direct equivalent of tmux `respawn-pane` in the inspected socket schema. `pane.split` does not accept an argv command, while a new `layout.apply` pane does.

A verified primitive is to create a fresh command tab with `layout.apply`, then move the running pane into an existing tab. The test preserved the process and terminal identity; the empty source tab closed automatically.

Use that primitive where appropriate. A workmux-controlled initial launcher is a candidate for the existing first-pane handshake flow. Do not copy the shared agent/sandbox setup pipeline into the herdr adapter. Finalize the launcher only after shell-initialization, input, and identity tests pass.

Keep command text separate from shell syntax where possible. Do not assume that sending text to a newly created interactive shell is a reliable replacement for controlled argv launch. Test slow and non-POSIX shell initialization, multiline input, bracketed paste, Unicode, large prompts, timeout, and cancellation.

## 8. Navigation and presentation

Retain workmux's dashboard and sidebar UI. Do not substitute herdr's native agent list for a workmux feature.

Herdr supports configured popup commands. Its socket schema exposes `popup.close`, but not a general `popup.open` request. Inspect existing popup launch paths before choosing the workmux integration; do not claim that one socket method replaces tmux `display-popup`.

Tests must cover a command invoked from a pane, from a popup, and without a current pane. Determine the invoking context safely, not merely the server's most recently focused pane. Verify multi-client behavior with real attached clients, including close/remove navigation and dashboard peek/jump.

Do not overwrite a user's herdr key bindings or theme as a hidden side effect. Any required configuration setup must be explicit and limited to workmux integration. No herdr plugin is permitted.

## 9. Implementation sequence and gates

### Phase 1 — Inventory and feasibility tests

- Expand the feature matrix into command, option, configuration, and interaction cases from the CLI definitions and tests.
- Add an isolated herdr test environment. Never use a user's active server as a fixture.
- Convert the temporary layout and argv probes into repeatable tests.
- Verify popup/caller focus, multiple clients, status acknowledgement, process lifetime, and safe launch before relying on them in the implementation.
- Report any further platform limit. Do not reduce the requirement to pass this gate.

### Phase 2 — Small shared interfaces

- Add herdr backend selection and registration.
- Replace tmux-name session checks with explicit session support.
- Introduce the sidebar host interface and identity contracts.
- Route tmux through the interface without changing its behavior.
- Run existing tmux tests before adding herdr workflow behavior.

### Phase 3 — Core herdr operations

- Implement workspace, tab, pane, naming, placement, launch, input, capture, and liveness operations.
- Reuse shared worktree, hook, prompt, agent, and sandbox workflows.
- Verify foreground/background behavior, headless mode, run/wait/send/capture, and close/remove/merge navigation.

### Phase 4 — Persistent integration and UI

- Implement the managed background runtime, event recovery, and ownership state.
- Connect the shared sidebar to herdr observations and safe live layouts.
- Implement workmux status acknowledgement and native display integration.
- Verify dashboard, popup, filtering, preview, diff, and navigation behavior.

### Phase 5 — Failure, regression, and release checks

- Test interrupted launch, runtime crash, event loss, resize, concurrent commands, manual terminal changes, and server restart.
- Test sandbox RPC and clipboard behavior in supported environments.
- Run the feature matrix against tmux and herdr. Run regression checks for other backends affected by shared changes.
- Check normal herdr operation with tmux unavailable to the tested processes.
- Update relevant command and configuration documentation. Keep README changes high level, if needed.

Phases are development steps, not permission to release partial support as complete.

## 10. Definition of complete

Every matrix case must link to a passing test or an explicit, reviewed verification result. Relevant tmux-only tests must gain equivalent herdr scenarios; a skip does not establish support. Tests should compare observable behavior rather than native command spelling.

The agreed temporary-tab behavior is the only accepted visible layout exception at this point. Any further difference must be documented and approved. Required tests that cannot run remain pending, not passed.

Do not claim “100% support” until the matrix is complete, the new failure tests pass, and existing tmux behavior is preserved.

## 11. Evidence and limits

Inspection date: 2026-09-13.

Herdr source: version 0.9.0, commit `b99002ac99b09e00b4ca692436cb15a6b0d676f1`.

Primary sources:

- [Versioned socket interface documentation](https://github.com/herdrdev/herdr/blob/b99002ac99b09e00b4ca692436cb15a6b0d676f1/docs/next/website/src/content/docs/socket-api.mdx)
- [Layout implementation](https://github.com/herdrdev/herdr/blob/b99002ac99b09e00b4ca692436cb15a6b0d676f1/src/app/api/layouts.rs)
- [Popup implementation](https://github.com/herdrdev/herdr/blob/b99002ac99b09e00b4ca692436cb15a6b0d676f1/src/app/popup.rs)
- [Versioned configuration documentation](https://github.com/herdrdev/herdr/blob/b99002ac99b09e00b4ca692436cb15a6b0d676f1/docs/next/website/src/content/docs/configuration.mdx)
- [Multi-client concepts](https://github.com/herdrdev/herdr/blob/b99002ac99b09e00b4ca692436cb15a6b0d676f1/docs/next/website/src/content/docs/concepts.mdx)
- Installed schema obtained with `herdr api schema --output PATH`; live client/server versions checked with `herdr status --json`.

The successful temporary Python probe started a separate named server with temporary configuration/state directories and `/bin/sh` panes. It checked:

- Three original panes in a mixed horizontal/vertical layout.
- A full-height left sidebar added through a same-workspace temporary tab.
- Unchanged shell PIDs, terminal IDs, pane IDs, and original tab ID.
- Exact restoration of the exported content layout after sidebar removal.
- Retention of terminal output.
- Explicit argv launch in a fresh tab and live transfer into an existing tab.
- Removal of temporary tabs and command panes without losing the original layout.

All probe servers were stopped. The final probe passed. It was not a workmux integration test, did not run real coding agents, and did not attach UI clients. No workmux implementation change was made during this design session.
