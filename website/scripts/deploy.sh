#!/usr/bin/env bash
# Build the website and publish it atomically for Caddy.
#
#   scripts/deploy.sh             build + publish
#   scripts/deploy.sh --rollback  point "current" back to the previous build
#   scripts/deploy.sh --list      show published builds
#
# Caddy serves ./.deploy/current (mounted read-only at /srv/website). Each build is copied to
# .deploy/releases/<UTC timestamp> and "current" is switched with an atomic rename, so visitors
# never see a half-written site and Caddy needs no reload or restart. The last 5 builds are kept.
set -euo pipefail

cd "$(dirname "$0")/.."
DEPLOY=.deploy
KEEP=5

list() { ls -1 "$DEPLOY/releases" 2>/dev/null | sort; }
current() { readlink "$DEPLOY/current" 2>/dev/null | sed 's|^releases/||'; }

switch_to() {
  ln -sfn "releases/$1" "$DEPLOY/current.tmp"
  mv -T "$DEPLOY/current.tmp" "$DEPLOY/current"
  echo "website: now serving $1"
}

case "${1:-}" in
  --list)
    cur=$(current)
    for r in $(list); do [[ $r == "$cur" ]] && echo "* $r" || echo "  $r"; done
    exit 0
    ;;
  --rollback)
    cur=$(current)
    prev=$(list | awk -v c="$cur" '$0 < c' | tail -n 1)
    [[ -n $prev ]] || { echo "website: no older build to roll back to" >&2; exit 1; }
    switch_to "$prev"
    exit 0
    ;;
  "") ;;
  *) echo "usage: $0 [--list|--rollback]" >&2; exit 2 ;;
esac

docker compose run --rm build
[[ -f dist/index.html ]] || { echo "website: build produced no dist/index.html" >&2; exit 1; }

ts=$(date -u +%Y%m%d-%H%M%S)
mkdir -p "$DEPLOY/releases"
cp -a dist "$DEPLOY/releases/$ts"
chmod -R a+rX "$DEPLOY/releases/$ts"
switch_to "$ts"

# Prune old builds, never the one being served.
cur=$(current)
list | head -n -"$KEEP" | while read -r old; do
  [[ $old == "$cur" ]] || rm -rf "${DEPLOY:?}/releases/$old"
done
