# Review retained Herdr work

## Review result

Useful task-05 safety fixes have been adapted inside
`src/multiplexer/herdr/`. No core workflow, command, configuration, state, or
sandbox changes were imported.

Integrated:

- Immediate cleanup closes captured, verified terminals with `pane.close`,
  not whole tabs or workspaces. Late foreign occupants remain alive and cause
  a partial-cleanup error.
- The same rule now applies to the current adapter's deferred Python helper.
- Launch ownership comes from the live destination terminal after a native
  move, not an old tab record.
- A replacement launch retains primary ownership. An added split is not primary.
- Adapter-local tests reuse the task-05 close-barrier test approach. Six
  private-server cases cover immediate/deferred cleanup and late insertion
  into existing/new tabs. Another probe checks moved-terminal launch ownership.

Verification: `cargo test` and
`python3 src/multiplexer/herdr/integration/run.py` pass. See
`src/multiplexer/herdr/SUPPORT.md` for supported functions and restrictions.

## task-05 — retain remaining reference work

Worktree:
`/private/var/folders/8x/70__y82n71dbb9q971h35d0h0000gn/T/.ctx-mode-I3pasx/workmux-herdr-lanes-_vf9c4mp/task-05`

Detached HEAD: `5055fe0230c995cd31c8a54d3a449e894af545c0`, with uncommitted changes.

The safety fixes above are integrated. The old cleanup, launch, and workflow
tests depend on shared interfaces and fixtures absent from the current adapter;
relevant checks were adapted rather than copied wholesale. Session ownership,
core workflow changes, and the old deferred-operation runtime were not imported.
Keep this worktree for those remaining references.

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
