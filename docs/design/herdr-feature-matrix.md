# Herdr feature and acceptance matrix

This is the planning matrix for [full herdr support](herdr-support.md). It is not a support declaration.

Baseline: workmux commit `ae85d52e9686ac9324bdad2000355e06d8dbfe20`; unmodified herdr 0.9.0, protocol 22.

**Full-feature acceptance remains pending.** The current adapter has a verified
subset. See the [adapter support record](../../src/multiplexer/herdr/SUPPORT.md)
for its restrictions. The results below do not mark a complete feature row as
passed.

## Integrated adapter evidence

Run `python3 src/multiplexer/herdr/integration/run.py` against Herdr 0.9.0,
protocol 22. The runner uses private servers, not the user's session.

| Area | Verified subset | Evidence under `src/multiplexer/herdr/` |
| --- | --- | --- |
| C01, C03–C05, C10 | CLI add/list/close/open/remove with an explicit parent workspace; session mode is still rejected | `integration/run.py::cli_smoke` |
| C04, C05 | Immediate and deferred cleanup preserve foreign terminals inserted just before a close request; a retained container produces an error | `cleanup_tests.rs::isolated_cleanup_race`, `integration/cleanup_proxy.py`; six cases |
| W02, W06 | Launch uses the live terminal owner after a native move, retains primary ownership on replacement, and does not copy the primary flag to a split | `cleanup_tests.rs::isolated_launch_ownership` |
| C04, C05 | Deferred cleanup rejects changed contents and stale server identity; scheduled cleanup survives caller exit | `deferred_tests.rs::isolated_deferred_cleanup` |
| W02, W05, W06 | Core pane operations, launch, identity checks, and native split-size limits | `tests.rs` private-server probes |

Task-06 sidebar layout, recovery, locking, and spacing-calibration work remains
reference code in the retained worktree. C21, C28, and U07–U10 are not integrated.

## Coverage rules

1. Expand each row into test cases for all applicable options, aliases, configuration settings, and interaction paths. Use `src/cli.rs`, flattened argument structs, subcommand definitions, `src/config.rs`, and existing tests as the inventory sources.
2. Include successful outcomes, refusal/error cases, exit status, output contracts, and final state. Backend names and backend-native addresses may differ according to the agreed mapping.
3. Run the same behavioral scenarios against tmux and herdr where possible. Keep backend-specific fixture mechanics separate from the assertions.
4. For each relevant `tmux_only` test, add an equivalent herdr case. Do not count a skipped test as supported behavior.
5. Cover current/explicit targets, foreground/background operation, and pane/popup/external invocation where applicable.
6. Add newly discovered cases to this matrix. This initial grouping is not a reason to omit a command option or a hidden command used by a feature.
7. Record the test path or verification evidence and result for every case before completion. Missing test infrastructure leaves the case pending.

## Commands

| ID | Commands or family | Required acceptance behavior |
| --- | --- | --- |
| C01 | `add` | Same worktree/branch result; window mode creates a tab; session mode creates a workspace; background mode preserves the intended client view; names and placement are correct |
| C02 | `add` variants | PR/MR and remote refs, forge selection, base selection, automatic names, explicit target/parent, prompts, layouts, multiple/named agents, rescue, limits, continue/fork, wait, alternate config, dry run, headless and JSON behavior |
| C03 | `open` | Existing/new worktree opening, explicit parent/target, both modes, duplicate handling, additional windows, and already-open targets |
| C04 | `close` | Close only owned terminal targets; preserve worktree, branch, and untracked files; navigate safely from the target or another location |
| C05 | `remove` and aliases | Apply existing refusal/force/branch rules; remove the correct worktree and owned targets; preserve unrelated terminals; complete deferred cleanup |
| C06 | `merge` | Preserve pre-merge hooks, merge policy, failures, cleanup, and client navigation |
| C07 | `rebase`, `set-base` | Preserve branch/base behavior, conflicts, metadata, and subsequent cleanup behavior |
| C08 | `rename` | Rename worktree and mapped tab/workspace as required; preserve live programs and ownership; follow branch-option behavior |
| C09 | `resurrect` | Recreate expected targets after server/computer loss without duplicate ownership or use of stale live identities |
| C10 | `list`, `path` | Preserve filtering, output/JSON contracts, paths, and active-target reporting |
| C11 | `send` | Correct target across projects; argument/file/stdin input; complete literal and multiline delivery; no unintended focus change or extra submission |
| C12 | `capture` | Correct target, line limit, output ordering, terminal content, and missing-target errors |
| C13 | `status` | Correct workmux lifecycle status; explicit/all-project targets; JSON and Git detail; no stale process attribution |
| C14 | `wait` | Correct requested state; any/all semantics; timeout, completion, missing/dead targets, and exit behavior |
| C15 | `run`, `_exec` | Correct cwd/argv; new execution pane; streamed output and exit status; background, timeout, artifacts, and cleanup |
| C16 | `reap-agents` | Same age/dry-run/force policy; stop only the verified tracked process, not a reused pane or unrelated shell |
| C17 | `sync-files` | Same file copy/symlink behavior and current/all-worktree scope |
| C18 | `init`, `config` | Same configuration defaults, validation, precedence, and subcommands; herdr selection must not imply reduced feature support |
| C19 | `setup`, `uninstall` | Agent hooks and skills work on herdr; targeted setup and dry-run removal are correct; no herdr plugin or hidden user-config overwrite |
| C20 | `dashboard` | Preserve all dashboard options, actions, views, prompts, keyboard/mouse behavior, and settings; see UI matrix |
| C21 | `sidebar` | Toggle/on/off, global/workspace scope, next/previous/jump/filter/prune, appearance, and layout changes; see UI matrix |
| C22 | `last-done`, `last-agent` | Correct selection and previous-target tracking across tabs, workspaces, and projects; no jump to a stale address |
| C23 | `set-window-status`, `register-agent` | Correct caller, owner, and process; workmux status persistence and display; late hooks cannot attach to a replacement terminal |
| C24 | `claude` subcommands | All existing integration and conversation behavior remains available; terminal-specific actions use the selected backend |
| C25 | `sandbox` subcommands, `host-exec`, `clipboard-read` | Same sandbox policy and host RPC/clipboard results; correct host pane identity; no unsafe unsandboxed fallback |
| C26 | `completions`, `_complete-*` | All supported shells, dynamic worktree/branch/agent targets, and cross-project handles; no tmux-only discovery assumption |
| C27 | `docs`, `changelog`, `update`, `_check-update` | Existing behavior remains available from herdr, including safe update failure behavior |
| C28 | `_sidebar-run`, `_sidebar-sync`, `_sidebar-reflow`, `_sidebar-reflow-all`, `_sidebar-daemon` | Backend-correct host resolution, observation, layout, lifecycle, and error behavior; no accidental tmux access |

## Configuration and workflows

| ID | Area | Required acceptance behavior |
| --- | --- | --- |
| W01 | Object mapping | Session → workspace, window → tab, pane → pane; herdr named session → backend instance |
| W02 | Target discovery | Identical names in different projects/workspaces; exact targets; explicit parent; renamed and moved targets; ownership cannot be inferred from a label alone |
| W03 | Configuration | Global/project/nested/alternate configuration, monorepos, directory resolution, environment substitution, and frozen/recovery configuration |
| W04 | Worktree creation | Local/remote branches, forge APIs, prompt loading, naming, rescue, orphan cleanup, and concurrency limits |
| W05 | Pane layouts | Target indices, all supported split directions, fixed/proportional dimensions, focus, zoom, named layouts, and no-command panes |
| W06 | Shell startup | Default/custom shells, login policy, slow init, shell-specific commands, initial-pane launch, handshake readiness, environment, cwd, and terminal echo |
| W07 | Agents | Every supported agent profile, named/multiple agents, prompt input, continue/fork, status hooks, conversation identity, and cancellation |
| W08 | User hooks | Preserve workmux pre/post creation, merge, and removal hooks and failure policy; native tmux command compatibility is not required |
| W09 | Headless | Provisioning without a multiplexer target remains headless; no unnecessary controller/server creation or terminal mutation |
| W10 | Dry run | No persistent state or terminal changes, including no unnecessary background runtime startup |
| W11 | Sandbox | Container and Lima paths where supported, guest shims, host RPC, identity transfer, clipboard, and fail-closed behavior |
| W12 | Remote operation | Workmux running on the intended SSH host uses that host's explicit herdr instance; inherited context cannot select another server |
| W13 | Existing backends | Tmux behavior remains unchanged; shared changes retain working behavior for WezTerm, Kitty, and Zellij |

## UI and persistent behavior

| ID | Area | Required acceptance behavior |
| --- | --- | --- |
| U01 | Dashboard inventory | Correct agents/worktrees, sorting, filtering, session/workspace scope, stale entries, settings, and project selection |
| U02 | Dashboard preview | Correct live output, ANSI handling, scrolling, preview size, and target changes |
| U03 | Diff and patch modes | Same diff content, navigation, staging/revert operations, confirmation, and failure handling |
| U04 | Dashboard actions | Send, jump, peek, kill, sweep, create/open/close, and other registered actions retain their effects |
| U05 | Popup launch and exit | Verified herdr popup launch path, correct cwd/caller, no false pane ownership, correct close and return behavior |
| U06 | Popup navigation | Jump versus peek; closing/removing current or other targets from popup and pane; retain attached clients where tmux does |
| U07 | Sidebar rendering | Same workmux compact/tile views, templates, styling, icons, state labels, timers, Git/PR/check data, and live configuration updates |
| U08 | Sidebar scope | Global and workspace-only scope, per-workspace opt-out, future tab/workspace creation, and isolated backend instances |
| U09 | Sidebar layout | Left/top placement, absolute/proportional size, full-edge coverage, content split proportions, resize, hidden tabs, zoom, and manual pane changes |
| U10 | Sidebar navigation | Keyboard/mouse, next/previous/index jump, correct host identity, filter/layout toggles, and sleeping/prune controls |
| U11 | Status lifecycle | Working/waiting/done, focus acknowledgement, no-sidebar operation, multiple agents in a tab, custom icons, and native display consistency |
| U12 | Multiple clients | Independent client views where supported, shared-tab sizing, invoking-client navigation, popup context, and safe removal of the current target |
| U13 | Runtime lifecycle | Automatic single-instance startup, no duplicate observers, continued operation without sidebar UI, restart/reconnect, and no herdr-server shutdown |

The temporary-tab/intermediate-layout behavior during sidebar changes is approved. It does not permit loss of running programs, incorrect final focus, or permanent extra tabs.

## Failure and safety tests

| ID | Scenario | Required result |
| --- | --- | --- |
| F01 | Runtime stops during each layout step | Restart can reconcile and restore/resume safely; user terminals remain intact |
| F02 | Another command or user changes the layout | Detect conflict; do not remove terminals or overwrite unrelated changes based on stale observations |
| F03 | Herdr socket disconnects or events are lost | Fresh snapshot reconciles state; no duplicated tabs/panes or unsafe replay |
| F04 | Server restarts and public IDs repeat | Old ownership/status records cannot attach to new terminal processes |
| F05 | Agent exits or late status hook arrives | Stale process state is rejected; another program is not reaped or marked as that agent |
| F06 | Names or prompts contain shell syntax | Values remain data; no unintended shell command execution |
| F07 | Cleanup originates inside the target | Deferred work survives target termination and completes or reports a recoverable failure |
| F08 | Temporary tab contains moved user panes | Cleanup cannot close it until all required user panes are safely accounted for |
| F09 | Two servers use identical public IDs | Commands, state, events, and background runtime remain isolated |
| F10 | Tmux is unavailable to herdr test processes | Every required herdr feature works without a hidden tmux dependency |
| F11 | Unsupported or mismatched protocol | Clear error without mutation, server replacement, implicit upgrade, or false success |
| F12 | Required external test infrastructure is absent | Case remains pending; absence cannot be recorded as a passing test |

## Verified primitives, not completed features

| Probe | Observed result | Still unverified |
| --- | --- | --- |
| Three-pane layout with left sidebar through a temporary tab | Original shell PIDs, terminal IDs, pane IDs, and tab ID preserved | Real agents, top sidebar, arbitrary layouts, concurrent changes, attached-client focus/zoom, and recovery |
| Sidebar removal | Original exported content layout restored; output retained; temporary tab removed | Failure at each intermediate step and external/manual changes |
| Explicit argv launch in a fresh tab, then live move | Command ran; process and terminal identity survived transfer; empty source tab closed | Complete workmux shell handshake, slow/non-POSIX shells, sandboxed agents, and initial-pane replacement contract |

See the design document for pinned sources and experiment conditions.
