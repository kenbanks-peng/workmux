# Demos

| Demo | What to show |
|---|---|
| Worktree lifecycle | Create a worktree with its own tab, split panes, launch commands, close it, then reopen it. |
| Parallel agents | Create multiple agent worktrees with `--count`, multiple `--agent` options, or `--foreach`. Show background creation preserving your current focus. |
| Live agent status | Show working → waiting → done updating in Herdr’s native display, without changing pane labels or adding workarounds. |
| Dashboard navigation | Show a registered agent marked done; press Enter to close the dashboard and focus its pane. |
| Wait and command tracking | Wait for an agent to finish; run a command and show captured output and propagated exit status. |
| Diff and patch review | Open a worktree diff, stage a change, then undo it. |
| Merge and cleanup | Make a simple conflict-free change in a worktree, merge it, and clean up its tab and worktree. |
| Resurrect a closed tab | Close a worktree tab, then restore its layout and launch a fresh agent. |
| Lifecycle hooks | Show a post-create hook, then a pre-remove hook that prevents removal when a check fails. |
| File sync | Demonstrate copied files, symlinks, and `sync-files --all` across worktrees. |
