# Herdr adapter

The adapter targets unmodified Herdr 0.9.0, protocol 22, on macOS and Linux.
Use `WORKMUX_BACKEND=herdr` to select it. Inside Herdr, `HERDR_SOCKET_PATH`
selects the server automatically if no nested multiplexer signal is present.
The adapter does not start or stop the user's Herdr server.

Workmux windows map to Herdr tabs. The existing session methods map to
workspaces, not named Herdr servers. From outside a pane, use an explicit
workspace, for example:

```sh
WORKMUX_BACKEND=herdr HERDR_SOCKET_PATH=/absolute/path/herdr.sock \
  workmux add feature --parent-session workspace-name
```

## Implementation boundary

All platform code and integration tests are in this directory. Shared changes
cover backend registration, two multiplexer launch lifecycle hooks, and the
explicit status-target allowlist. The allowlist accepts Herdr; the adapter still
validates the endpoint and server-lifetime-qualified terminal identity. The shared
resurrect command also prints the full error chain for failed worktrees. Workflow,
state, sandbox, sidebar, and other backend files are unchanged. These constraints
supersede the earlier shared-interface design in `docs/design/herdr-support.md`
for this implementation.

The adapter supplies tab creation and placement, pane splits, controlled command
launch, input, capture, focus, zoom, names, ownership checks, native agent reports,
and immediate or deferred cleanup. Splits outside Herdr's native 10–90% range
fail before pane allocation. Only fresh workmux panes can be replaced for launch.
Live layout replacement is not used. Launches read ownership from the live
destination terminal, not a cached tab record. A replacement retains the primary
ownership flag; an added split does not inherit that flag.

Immediate and deferred cleanup match tmux: window cleanup uses `tab.close`,
and session cleanup uses `workspace.close`. All panes in the target close,
including panes you added and panes inserted after the ownership check.
Tab cleanup requires a verified Workmux terminal. Workspace cleanup requires
its Workmux ownership record. Labels alone are not proof of ownership.
Cleanup does not follow terminals moved outside the target.

Deferred operations use the same workmux executable through the private
`_herdr-deferred` command. The Rust helper shares the adapter's transport and
process identity checks. It checks the captured server lifetime on each connection
and terminal identities before cleanup. Scheduled cleanup runs in a detached
process, not a thread that dies with the caller. Python is not required at runtime.

## Core restrictions

This is not full feature parity with tmux:

- The unchanged workflows reject `--session` and session-mode configuration.
- The unchanged sidebar is tmux-only. No separate Herdr sidebar is installed.
- Automatic focus acknowledgement is unsupported. Protocol 22 does not expose
  the attached-client visibility needed for tmux-equivalent acknowledgement.
  Native status and Workmux state remain unchanged on focus. See the capability
  evidence below; no observer is installed.
- Herdr-specific sandbox identity routing is not verified. Shared agent reaping
  passed with a cooperative stub: dry-run, Ctrl-C exit, and state removal.
  Ctrl-D fallback and unresponsive agents remain unverified.
- Popup opening remains subject to the existing command's backend restrictions.
  The adapter can resolve the caller of a native Herdr popup.

No tmux emulation, plugin, configuration rewrite, or core bypass is used.

## Verification

```sh
cargo test
python3 src/multiplexer/herdr/integration/run.py
python3 src/multiplexer/herdr/integration/support_checks.py
```

The integration runner starts private servers with temporary HOME and XDG
paths, then stops those servers. It requires Herdr 0.9.0 and Python 3. Live Rust
probes are explicitly ignored in ordinary unit runs; they are not counted as
passed without a private server. The earlier full-feature matrix is not an
acceptance claim for this adapter.

The runner also tests six cleanup races: immediate and deferred tab cleanup,
and immediate and deferred workspace cleanup with insertion into an existing
or new tab. A protocol proxy inserts a foreign terminal just before the first
close request. Each case checks that `tab.close` or `workspace.close` was sent
and that the target and the inserted terminal were removed. A separate probe checks launch
ownership after a native move into a tab with stale ownership metadata.

On 2026-09-15, the private-server runner and all 15 support probes passed on
macOS without the status workaround. An ordinary agent stub launch registers
with its original `WORKMUX_STATUS_*` variables. Detached hooks work without
native Herdr variables or process ancestry. Working, waiting, done, and clear
agree with Workmux JSON state and native reports: waiting maps to `blocked`,
done maps to `idle`, and clear maps to `unknown` in protocol 22. Native agent
recognition can display idle as done. No label change is needed for routing.

Status tests cover moved-terminal registration and later hooks, closed terminals,
invalid and partial targets, endpoint separation, and server restart. Rejected
hooks do not change native status or stored records and do not fall back to the
caller's pane. Hook commands retain their existing best-effort exit behavior;
command success alone is not evidence of an update.

Wait releases on done; run retains output, exit status 7, and background output.
The dashboard renders done and focuses the agent on Enter. Multiple-agent and
continue/fork stub launches register and report working. These are not real
agent replay or sandbox checks. Linux was not retested for this change.

### Focus acknowledgement: blocked on protocol 22

On 2026-09-15, private macOS servers verified the following limits on unmodified
Herdr 0.9.0. These are capability and restriction checks, not feature support:

- `events.subscribe` accepts `pane.focused`, `tab.focused`, and
  `workspace.focused`. An API `pane.focus` emits all three events even when
  there is no attached UI. With a UI attached, the same call emits the same
  payload. The payload contains destination IDs, not client or caller identity.
- Manual Ctrl-B h/l navigation from two real PTY-backed UI clients changes
  `session.snapshot` focus fields but produces no focus events during the
  probe's observation interval. A later API focus confirms the subscription
  is still live. An event-only observer would miss these manual changes.
- `SessionSnapshot` exposes `focused_workspace_id`, `focused_tab_id`,
  `focused_pane_id`, and per-resource `focused` flags. It has no attached-client
  list/count or per-client focus map. The focus fields remain set after both
  UI clients detach. Polling these fields cannot prove that a pane is visible
  to an attached UI. Layout dimensions are not proof of attachment either.
- Ordinary registered agent updates to an already-focused pane retain waiting
  (`blocked`) and done (`idle`). Focus away followed by `workmux open feature`
  also retains them. Working remains working. Native reports and Workmux JSON
  tracking agree throughout this restriction check.

The tmux reference acknowledges waiting/done immediately when the pane is active
in an active window in an attached session. It otherwise waits for pane focus.
Protocol 22's retained server selection is not equivalent to that condition.
A snapshot poller or focus-event observer would risk clearing an unseen status.
No partial observer is installed; there are no observer processes to duplicate
or orphan. Existing endpoint, lifetime, process, and ownership checks are unchanged.
Newer-status race, move, restart, and observer-lifecycle acceptance checks remain
blocked, not passed. Linux was not tested for this investigation.

Repeat the evidence checks with:

```sh
python3 src/multiplexer/herdr/integration/support_checks.py focus-protocol focus
```

They print `BLOCKED`, not a feature-support claim. Set `WORKMUX_HERDR_LOG_DIR`
to retain `focus-protocol.json`, API request logs, server logs, and client PTY
output under each private server's artifact directory. Full acknowledgement
requires a supported way to establish attached-client visibility and focus;
do not infer it from labels, inherited pane IDs, geometry, or event arrival.

### Session-mode gate: restart recovery is blocked

On 2026-09-15, an experimental add/open support change was tested on private
macOS Herdr 0.9.0 / protocol 22 servers, then removed. The session-mode restriction
remains in place. These results are diagnosis, not enabled feature support.

The experiment passed add/open with `--session`, `--mode session`, and configuration
mode, background creation, foreground open, workspace/tab mapping, close, and
remove. Ordinary agent-stub registration, working status, and resurrect after
workspace close also passed without a status workaround.

Same-socket restart failed the session recovery gate. Herdr restored the workspace
label `wm-feature` with an unknown agent status. `workmux resurrect` returned success
but printed `skipping (already open)` and `Nothing to restore`; it did not launch a
fresh agent. In `workflow::resurrect::plan`, session-mode skip detection tests only
membership in the live session-name set. That is not proof that a restored workspace
contains a current Workmux agent or has current-lifetime cleanup ownership.

A recovery destination and ownership rule must be resolved before session mode
can be enabled. Preserve the occupied restored layout. Do not treat its label or
old workspace ownership file as authority to close it. Multi-window setup, mode
conversion, rename, merge cleanup, and in-pane navigation were not verified in this
experiment. The complete lifecycle gate remains open. Linux was not tested.

### Restore after restart

On 2026-09-15, window-mode restore passed on private macOS servers at the same
socket after stop/start, using ordinary agent-stub registration. Restored agents
received fresh server-lifetime-qualified pane identities and could report status.
The replacement parent was only a creation destination. Native restored panes
were retained; their labels and old ownership files did not authorize cleanup.
Old tab and workspace cleanup commands failed against the new server lifetime.
Linux and real agent conversation replay were not tested for this change.

The earlier restart failure was a probe defect, not a stale-parent recovery defect.
Herdr restores workspace labels on restart. The probe then created a second
workspace named `parent`. The full error was `Failed to create window in session:
Herdr workspace 'parent' is missing or ambiguous`. Resolution failed before tab
allocation, controlled launch, or ownership writes. The adapter now reports missing
and ambiguous names separately, including the match count. No shared parent
recovery change was needed. Use a unique parent name; do not create a second
workspace with a name that Herdr has already restored.

The probes cover duplicate parents without allocation or recovery-data loss,
repeated successful restore without duplicate tabs, and missing or stale parents
at the adapter boundary. The unchanged shared setup creates a missing named parent
rather than choosing the focused workspace. A partial pane-setup failure retains
recovery data and can leave a usable shell. Repeated restore skips that open layout
and does not consume the recovery data. After `workmux close <handle>` and correction
of the pane configuration, retry succeeds. The probe verifies this path with a
split-size failure after tab allocation; it does not claim automatic rollback of
already-created layouts.

The retained task-06 layout trees, journals, locks, spacing calibration, and
sidebar recovery tests remain reference work. They are not integrated or
verified against this adapter. Full sidebar support still requires shared
interface and command changes.
