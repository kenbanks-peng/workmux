# Development container

Run these tasks from the repository root:

```sh
mise run dev-container:build
mise run dev-container:config:install
```

The image tasks use Apple `container` and build `workmux-dev:latest` from
`dev-container/docker/`, with `dev-container/` as the build context.
