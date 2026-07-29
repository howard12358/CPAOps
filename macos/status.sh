#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"; . "$DEPLOY_REPO_ROOT/macos/lib/lifecycle.sh"
root=$(runtime_root)
for service in cli keeper; do
  name=$(service_name "$service")
  if is_disabled "$service"; then state=disabled; elif is_registered "$service"; then state=loaded; else state=stopped; fi
  version=$(readlink "$root/current/$name" 2>/dev/null | awk -F/ '{print $NF}')
  printf '%s: %s%s\n' "$service" "$state" "${version:+ ($version)}"
done
for port in 8317 18080; do lsof -nP -iTCP:"$port" -sTCP:LISTEN 2>/dev/null || true; done
