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

Pi config is managed directly in `~/.config/workmux/agents/pi`.
Select that directory in the global workmux config:

```yaml
sandbox:
  agent_config_dir: ~/.config/workmux/agents/{agent}
```

Workmux mounts that profile at `/tmp/.pi/agent` when it starts Pi. The image
build does not copy Pi config.
