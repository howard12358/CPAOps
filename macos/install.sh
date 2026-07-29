#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"
. "$DEPLOY_REPO_ROOT/macos/lib/config.sh"
. "$DEPLOY_REPO_ROOT/macos/lib/proxy.sh"
. "$DEPLOY_REPO_ROOT/macos/lib/releases.sh"
. "$DEPLOY_REPO_ROOT/macos/lib/lifecycle.sh"
. "$DEPLOY_REPO_ROOT/macos/lib/launchd.sh"

[ "$#" -eq 0 ] || { printf 'Usage: %s\n' "$0" >&2; exit 2; }
require_macos_arm64
require_commands curl tar shasum plutil launchctl
ensure_runtime_layout
root=$(runtime_root)
if [ ! -f "$root/config/config.yaml" ] || [ ! -f "$root/config/keeper.env" ]; then
  management_key=''
  if [ ! -f "$root/config/config.yaml" ] || [ ! -f "$root/config/keeper.env" ]; then
    printf 'CPA management key: ' >&2; stty -echo; trap 'stty echo' EXIT HUP INT TERM; IFS= read -r management_key; stty echo; trap - EXIT HUP INT TERM; printf '\n' >&2
  fi
  if [ ! -f "$root/config/config.yaml" ]; then
    sed "s|__REQUIRED__|$management_key|g" "$DEPLOY_REPO_ROOT/config/cpa.config.yaml.example" > "$root/config/config.yaml"
  fi
  if [ ! -f "$root/config/keeper.env" ]; then
    printf 'Keeper login password: ' >&2; stty -echo; trap 'stty echo' EXIT HUP INT TERM; IFS= read -r login_password; stty echo; trap - EXIT HUP INT TERM; printf '\n' >&2
    sed -e "s|CPA_MANAGEMENT_KEY=__REQUIRED__|CPA_MANAGEMENT_KEY=$management_key|" -e "s|LOGIN_PASSWORD=__REQUIRED__|LOGIN_PASSWORD=$login_password|" "$DEPLOY_REPO_ROOT/config/keeper.env.example" > "$root/config/keeper.env"
  fi
  chmod 600 "$root/config/config.yaml" "$root/config/keeper.env"
fi
ensure_keeper_defaults
validate_config
load_proxy || prompt_proxy
render_plists
for target in cli keeper; do
  set -- $(fetch_latest_release "$target")
  asset=$1 sums=$2
  archive="$root/downloads/$(basename "$asset")"
  checksum_file="$root/downloads/$(service_name "$target").checksums.txt"
  github_curl --fail --location --silent --show-error "$asset" -o "$archive"
  github_curl --fail --location --silent --show-error "$sums" -o "$checksum_file"
  version=$(basename "$archive" | sed -E 's/.*_v?([0-9]+(\.[0-9]+)+)_.*/\1/')
  install_release "$target" "$version" "$archive" "$checksum_file"
  if is_registered "$target"; then kickstart_service "$target"; else bootstrap_service "$target"; fi
done
printf 'Installed CPA and Keeper. Run sh macos/status.sh for status.\n'
