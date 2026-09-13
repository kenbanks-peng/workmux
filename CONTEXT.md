# Workmux

Workmux connects Git worktrees with terminal sessions, windows, panes, and coding agents.

## Language

**Worktree**:
A Git working copy with its own checked-out state. Several worktrees can share one repository.

**Multiplexer session**:
A named group of terminal windows. A tmux session and a herdr workspace represent this concept.
_Avoid_: Herdr named session, when referring to a group of workmux windows.

**Terminal window**:
A group of terminal panes within a multiplexer session. A tmux window and a herdr tab represent this concept.
_Avoid_: Workspace, when referring to one tab.

**Terminal pane**:
A terminal area with its own running program and terminal output.

**Backend instance**:
One multiplexer server with its own sessions, windows, and panes. A herdr named session represents a backend instance, not a workmux multiplexer session.

**Window mode**:
The arrangement in which a worktree uses a terminal window within a parent multiplexer session.

**Session mode**:
The arrangement in which a worktree uses its own multiplexer session, which contains its terminal windows.
