#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"
DEPLOY_REPO_ROOT=$ROOT
export DEPLOY_REPO_ROOT
. "$ROOT/macos/lib/common.sh"
. "$ROOT/macos/lib/lifecycle.sh"
. "$ROOT/macos/lib/launchd.sh"

HOME="$TMP_ROOT/home"
CPA_STACK_ROOT="$TMP_ROOT/runtime"
export HOME CPA_STACK_ROOT
ensure_runtime_layout
render_plists
plutil -lint "$HOME/Library/LaunchAgents/io.cpa-local.cli-proxy-api.plist" >/dev/null
assert_file "$TMP_ROOT/runtime/bin/run-cli-proxy-api"
printf 'PASS launchd\n'
