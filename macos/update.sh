#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"; . "$DEPLOY_REPO_ROOT/macos/lib/proxy.sh"; . "$DEPLOY_REPO_ROOT/macos/lib/releases.sh"; . "$DEPLOY_REPO_ROOT/macos/lib/lifecycle.sh"
target=${1:-all}; root=$(runtime_root)
load_proxy || true
services=$(service_targets "$target") || exit 2
for service in $services; do
  printf 'Checking latest %s release...\n' "$(service_name "$service")"
  [ "$service" != keeper ] || { command -v sqlite3 >/dev/null && sqlite3 "$root/keeper/app.db" ".backup '$root/keeper/pre-update-$(date +%Y%m%d%H%M%S).db'" || true; }
  set -- $(fetch_latest_release "$service"); asset=$1; sums=$2; archive="$root/downloads/$(basename "$asset")"; checksum_file="$root/downloads/$(service_name "$service").checksums.txt"
  github_curl --fail --location --silent --show-error "$asset" -o "$archive"; github_curl --fail --location --silent --show-error "$sums" -o "$checksum_file"
  version=$(basename "$archive" | sed -E 's/.*_v?([0-9]+(\.[0-9]+)+)_.*/\1/')
  printf 'Activating %s %s...\n' "$(service_name "$service")" "$version"
  install_release "$service" "$version" "$archive" "$checksum_file"
  kickstart_service "$service"
  printf '%s %s is active.\n' "$(service_name "$service")" "$version"
done
