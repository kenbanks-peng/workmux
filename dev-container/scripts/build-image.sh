#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
RUNTIME="${RUNTIME:-container}"
case "$RUNTIME" in container|docker|podman) ;; *) echo "Unsupported runtime: $RUNTIME" >&2; exit 1 ;; esac
"$RUNTIME" build --file Dockerfile.base --tag workmux-dev:base .
"$RUNTIME" build --file Dockerfile.pi --build-arg BASE=workmux-dev:base \
  --tag "${IMAGE:-workmux-dev:latest}" .
