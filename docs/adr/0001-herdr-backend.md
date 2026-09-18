---
status: accepted
---

# Extend the backend interface for unmodified herdr

Add a herdr adapter to the existing workmux multiplexer interface and a focused sidebar host interface, while retaining shared workflows and UI. Use a workmux-managed background process and recoverable live-pane moves because the target is unmodified herdr 0.9.0 without plugins, and its layout replacement operation terminates the old terminals.

Tmux sessions map to herdr workspaces; tmux windows map to herdr tabs. A herdr named session is a backend instance. Workmux retains ownership of Git worktree operations and agent state.

## Considered options

- **Tmux command emulation:** rejected. It would require interpreting tmux command syntax and behavior, although only workmux commands need support.
- **Herdr plugins or core changes:** excluded by the user.
- **A separate herdr sidebar and workflow implementation:** rejected. It would duplicate behavior and make full feature support harder to maintain.
- **A backend adapter with no shared-code changes:** insufficient. Sidebar operations and session checks currently depend directly on tmux.

## Consequences

A workmux background process may run without an open sidebar. Sidebar layout changes may briefly expose a temporary tab or intermediate layout; this trade-off is approved. Running programs must survive, and final layout and focus must be restored.

Recovery and identity checks are required, not optional cleanup. Full support remains a release requirement and must be established by the [feature matrix](../design/herdr-feature-matrix.md), not inferred from adapter registration.

See the [design and implementation sequence](../design/herdr-support.md).
