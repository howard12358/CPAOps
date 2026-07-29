#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"; . "$DEPLOY_REPO_ROOT/macos/lib/releases.sh"; . "$DEPLOY_REPO_ROOT/macos/lib/lifecycle.sh"
target=${1:-all}; root=$(runtime_root)
services=$(service_targets "$target") || exit 2
for service in $services; do
  [ "$service" != keeper ] || { command -v sqlite3 >/dev/null && sqlite3 "$root/keeper/app.db" ".backup '$root/keeper/pre-update-$(date +%Y%m%d%H%M%S).db'" || true; }
  set -- $(fetch_latest_release "$service"); asset=$1; sums=$2; archive="$root/downloads/$(basename "$asset")"; checksum_file="$root/downloads/$(service_name "$service").checksums.txt"
  github_curl --fail --location --silent --show-error "$asset" -o "$archive"; github_curl --fail --location --silent --show-error "$sums" -o "$checksum_file"
  version=$(basename "$archive" | sed -E 's/.*_v?([0-9]+(\.[0-9]+)+)_.*/\1/'); install_release "$service" "$version" "$archive" "$checksum_file"; kickstart_service "$service"
done
