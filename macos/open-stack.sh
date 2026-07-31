#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"

root=$(runtime_root)
[ -d "$root" ] || { printf 'CPAStack runtime directory does not exist: %s\n' "$root" >&2; exit 1; }
open -a Terminal "$root"
