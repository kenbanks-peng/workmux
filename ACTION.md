# Review retained Herdr work

## task-05 — retain remaining reference work

Worktree:
`/private/var/folders/8x/70__y82n71dbb9q971h35d0h0000gn/T/.ctx-mode-I3pasx/workmux-herdr-lanes-_vf9c4mp/task-05`

Detached HEAD: `5055fe0230c995cd31c8a54d3a449e894af545c0`, with uncommitted changes.

## task-06 — retain sidebar recovery reference

Worktree:
`/private/var/folders/8x/70__y82n71dbb9q971h35d0h0000gn/T/.ctx-mode-I3pasx/workmux-herdr-lanes-_vf9c4mp/task-06`

Detached HEAD: `5055fe0230c995cd31c8a54d3a449e894af545c0`, with uncommitted changes.

Reviewed reference areas under `src/multiplexer/herdr/`:

- `layout.rs`: identity-based trees, operation journals, and exclusive locks.
- `sidebar_layout.rs`: live-pane moves, before/after topology checks, and
  recovery without blind replay after a lost response.
- `calibration.rs`: native spacing probes and ownership-qualified probe cleanup.
- `sidebar.rs`: backend-local settings, scope, and sidebar operations.

The layout and calibration modules depend on each other and on sidebar
ownership roles and methods absent from the current adapter. Adding them alone
would add unused code, not working layout recovery. Full sidebar integration
requires core changes and remains outside this scope. Nothing was imported.

Retain `tests/test_herdr_sidebar_layout.py`,
`tests/test_herdr_sidebar_review.py`, and
`tests/support/herdr_response_proxy.py` as failure/recovery test references.
These sidebar tests have not been rerun against the current adapter. Do not
claim sidebar support from these files.

## Other retained worktrees

`task-09-fixes` contains state and sandbox RPC changes plus identity tests.
These are outside the adapter-only boundary and were not imported.
`task-09-review` has no local changes. Both worktrees remain untouched.
