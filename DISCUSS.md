# Discussion points

- **Native agent status:** The Herdr adapter maps Workmux waiting to Herdr blocked, and Workmux done to Herdr idle. Herdr's protocol has no distinct done state or Workmux icon field; the adapter sends the icon as message text. Workmux retains the exact state in its own state store. Investigate how to preserve the tmux status distinctions in Herdr's native display.

- Investigate functionality differences between Workmux's tmux sidebar and Herdr's native sidebar. Identify confirmed gaps and determine whether the adapter can address them through Herdr's native sidebar.

- **Clear status on focus:** tmux clears waiting/done indicators when the relevant pane receives focus, or immediately if it already has focus. The Herdr adapter does not implement this behavior. Add focus detection and status acknowledgement to match tmux. This is an adapter gap, not a confirmed Herdr limitation.

- When a create/open worktree command runs from outside Herdr, explicitly specify the destination workspace with `--parent-session`.

- Workmux currently blocks session mode for Herdr. The shared create and open workflows explicitly reject session mode when the backend name is not `tmux`:

  ```rust
  if mode == MuxMode::Session && context.mux.name() != "tmux"
  ```

  These checks are in `src/workflow/create.rs` and `src/workflow/open.rs`. The create workflow uses `options.mode` instead of `mode`.

- Investigate these Workmux features with Herdr. Test each feature and identify functionality gaps compared with tmux:
  - Merge
  - Rebase
  - Rename
  - Resurrect
  - Popup support, including command restrictions
  - Dashboard
  - Diff actions
  - Patch actions
  - PR / MR creation
  - Multiple agents
  - Continue / fork
  - Status
  - Wait
  - Last-agent navigation
  - Run commands with output and exit tracking
  - Agent reaping, including missing Herdr-specific process targeting
  - Sandbox support, including missing Herdr-specific sandbox identity routing
  - Host RPC
  - Clipboard
  - Hooks
  - File sync
  - Headless workflows

