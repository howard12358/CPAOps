#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"
[ "$#" -eq 1 ] || { printf 'Usage: %s cli|keeper\n' "$0" >&2; exit 2; }
case "$(service_name "$1")" in cli-proxy-api) prefix=cli-proxy-api ;; cpa-usage-keeper) prefix=cpa-usage-keeper ;; esac
root=$(runtime_root); mkdir -p "$root/logs"; touch "$root/logs/$prefix.out.log" "$root/logs/$prefix.err.log"
tail -n 200 -f "$root/logs/$prefix.out.log" "$root/logs/$prefix.err.log"
