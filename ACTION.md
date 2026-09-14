# Review retained Herdr work

Keep these worktrees until the useful changes have been reviewed. Both use
detached HEAD at `5055fe0230c995cd31c8a54d3a449e894af545c0` and contain
uncommitted changes. Their tests have not been rerun against the current adapter.

## task-05 — adapter safety fixes

Worktree:
`/private/var/folders/8x/70__y82n71dbb9q971h35d0h0000gn/T/.ctx-mode-I3pasx/workmux-herdr-lanes-_vf9c4mp/task-05`

Review `src/multiplexer/herdr/mod.rs` and `tests/test_herdr_cleanup_races.py` for:

- Close only verified panes, not the whole tab. Preserve foreign panes inserted
  after the ownership check. The current adapter still has this race.
- Read ownership from the live destination pane after a move, not an old tab record.
- Preserve the primary ownership flag when replacing a launch pane.
- Reuse relevant cleanup, launch, and workflow tests.

Adapt these fixes inside `src/multiplexer/herdr/`. Do not merge the worktree's
core workflow, command, or configuration changes.

## task-06 — layout recovery and tests

Worktree:
`/private/var/folders/8x/70__y82n71dbb9q971h35d0h0000gn/T/.ctx-mode-I3pasx/workmux-herdr-lanes-_vf9c4mp/task-06`

Potentially useful files under `src/multiplexer/herdr/`:

- `layout.rs`: layout trees, operation journals, and exclusive locks.
- `sidebar_layout.rs`: live-pane moves and recovery after interrupted operations.
- `calibration.rs`: measure native pane spacing for fixed-size layouts.
- `sidebar.rs`: backend-local settings and sidebar operations.

Review `tests/test_herdr_sidebar_layout.py`, `tests/test_herdr_sidebar_review.py`,
and `tests/support/herdr_response_proxy.py` for failure and recovery tests.

Full sidebar integration requires core changes and is outside the current
scope. Retain this code as reference; do not merge it wholesale.
