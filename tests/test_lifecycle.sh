#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"
. "$ROOT/macos/lib/common.sh"
. "$ROOT/macos/lib/lifecycle.sh"

CPA_STACK_ROOT="$TMP_ROOT" ensure_runtime_layout
set_disabled cli
assert_file "$TMP_ROOT/state/cli-proxy-api.disabled"
clear_disabled cli
assert_not_file "$TMP_ROOT/state/cli-proxy-api.disabled"
printf 'PASS lifecycle\n'
