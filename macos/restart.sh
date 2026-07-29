#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"; . "$DEPLOY_REPO_ROOT/macos/lib/lifecycle.sh"
target=${1:-all}; [ "$#" -le 1 ] || exit 2
services=$(service_targets "$target") || exit 2
for service in $services; do clear_disabled "$service"; kickstart_service "$service"; done
