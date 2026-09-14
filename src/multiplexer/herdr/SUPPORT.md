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

All platform code and integration tests are in this directory. The only shared
changes are backend registration and two multiplexer launch lifecycle hooks.
The command, workflow, state, sandbox, sidebar, and other backend files are
unchanged. These constraints supersede the earlier shared-interface design in
`docs/design/herdr-support.md` for this implementation.

The adapter supplies tab creation and placement, pane splits, controlled command
launch, input, capture, focus, zoom, names, ownership checks, native agent reports,
and immediate or deferred cleanup. Splits outside Herdr's native 10–90% range
fail before pane allocation. Only fresh workmux panes can be replaced for launch.
Live layout replacement is not used.

Deferred operations require `python3` on PATH. The embedded standard-library
helper checks the server lifetime on each connection and checks terminal
identities before cleanup. This avoids a new workmux CLI command. Scheduled
cleanup runs in a detached process, not a thread that dies with the caller.

## Core restrictions

This is not full feature parity with tmux:

- The unchanged workflows reject `--session` and session-mode configuration.
- The unchanged sidebar is tmux-only. No separate Herdr sidebar is installed.
- No automatic focus acknowledgement runtime is installed. Native status does
  not automatically clear on focus. Workmux retains its existing agent state.
- Herdr-specific sandbox identity routing and process-directed agent reaping
  are not added. These paths must not be treated as verified Herdr support.
- Popup opening remains subject to the existing command's backend restrictions.
  The adapter can resolve the caller of a native Herdr popup.

No tmux emulation, plugin, configuration rewrite, or core bypass is used.

## Verification

```sh
cargo test
python3 src/multiplexer/herdr/integration/run.py
```

The integration runner starts private servers with temporary HOME and XDG
paths, then stops those servers. It requires Herdr 0.9.0 and Python 3. Live Rust
probes are explicitly ignored in ordinary unit runs; they are not counted as
passed without a private server. The earlier full-feature matrix is not an
acceptance claim for this adapter.
