# Development container

Run these tasks from the repository root:

```sh
mise run dev-container:build
mise run dev-container:config:install
```

The image tasks use Apple `container` and build `workmux-dev:latest` from
`dev-container/docker/`, with `dev-container/` as the build context.
The local `.workmux.yaml` selects this image when working in this directory.

For another runtime or image tag, use the standalone build script:

```sh
RUNTIME=podman IMAGE=workmux-dev:latest ./dev-container/scripts/build-image.sh
```

The config task installs the managed `pi/` profile into
`~/.config/workmux/agents/pi`, without copying credentials or session state.
To choose a different destination:

```sh
./dev-container/scripts/install-config.sh /path/to/profile
```

Ignore rules for local state live in the root `.gitignore`.
