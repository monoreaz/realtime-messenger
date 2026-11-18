#!/usr/bin/env bash
# Run cargo for desktop/ inside a Rust container on the Linux build host.
# Usage: scripts/remote-cargo.sh <cargo args...>     e.g. scripts/remote-cargo.sh test
#        BUILD_ENV="-e FOO=bar" scripts/remote-cargo.sh run --bin x
# The source is rsynced to $BUILD_HOST:desktop-build; registry, toolchain and target
# dirs live in Docker volumes (desktop_cargo, desktop_rustup, desktop_target).
# CPU/memory are capped so builds don't starve services running on the host.
set -euo pipefail
HOST=${BUILD_HOST:-messenger-01}
here=$(cd "$(dirname "$0")/.." && pwd)
rsync -a --delete --exclude target/ --exclude shots/ --exclude '*.swp' "$here/" "$HOST:desktop-build/"
args=$(printf '%q ' "$@")
status=0
ssh -o BatchMode=yes "$HOST" "docker run --rm --init --cpus=3 --memory=5g \
  -u \$(id -u):\$(id -g) --network ${BUILD_NETWORK:-bridge} ${BUILD_ENV:-} \
  -v \$HOME/desktop-build:/src -v desktop_cargo:/cargo -v desktop_rustup:/rustup -v desktop_target:/target \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR=/target -e CARGO_TERM_COLOR=never \
  -w /src ${BUILD_IMAGE:-rust:1-bookworm} cargo $args" || status=$?
rsync -a "$HOST:desktop-build/Cargo.lock" "$here/Cargo.lock" 2>/dev/null || true
exit $status
