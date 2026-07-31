#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"; . "$DEPLOY_REPO_ROOT/macos/lib/lifecycle.sh"
purge=${1:-}; [ "$purge" = "" ] || [ "$purge" = --purge ] || exit 2
for service in keeper cli; do is_registered "$service" && launchctl_cmd bootout "$(service_domain)/$(service_label "$service")" || true; rm -f "$(service_plist "$service")"; done
if [ "$purge" = --purge ]; then printf 'Type DELETE to remove %s: ' "$(runtime_root)"; IFS= read -r confirmation; [ "$confirmation" = DELETE ] || { printf 'Cancelled.\n'; exit 1; }; rm -rf "$(runtime_root)"; fi
