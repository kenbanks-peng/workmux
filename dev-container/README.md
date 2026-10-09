# Development container

Run these tasks from the repository root:

```sh
mise run dev-container:build
```

The image tasks use Apple `container` and build `workmux-dev:latest` from
`dev-container/Dockerfile.base` and `dev-container/Dockerfile.pi`, with
`dev-container/` as the build context.

Workmux uses the host's global config at `~/.config/workmux/config.yaml`,
plus the repository's `.workmux.yaml`. No separate dev-container workmux
config is needed.

Interactive Bash shells use Oh My Posh with `dev-container/oh-my-posh.toml`,
based on the local Herdr prompt: matching colors, diamond segments, Git status,
and a two-line layout. The directory segment shows only the current folder,
and the prompt does not look up the runtime UID's username. This is cosmetic;
it does not create a matching `/etc/passwd` entry.

Use a Nerd Font in your host terminal for the prompt icons. No fonts need to
be installed in the container. Edit the TOML and rebuild to customize it.
The Oh My Posh version is pinned in `Dockerfile.pi` and can be overridden with
the `OH_MY_POSH_VERSION` build argument.

Run `f` to open Yazi and change the shell to its last directory on exit.
Arguments are forwarded to Yazi (for example, `f /path/to/directory`).
Launching `yazi` directly does not change the parent shell's directory.

Pi config is managed directly in `~/.config/workmux/pi/agent`.
Select that directory in the global workmux config:

```yaml
sandbox:
  agent_config_dir: ~/.config/workmux/pi/agent
```

Workmux mounts that profile at `/tmp/.pi/agent` when it starts Pi. The image
build does not copy Pi config.
