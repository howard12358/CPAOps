#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"
. "$ROOT/macos/lib/common.sh"
. "$ROOT/macos/lib/releases.sh"

CPA_STACK_ROOT="$TMP_ROOT/runtime"
export CPA_STACK_ROOT
ensure_runtime_layout
assert_eq '' "$(github_token)"
save_github_token test-token
assert_eq test-token "$(github_token)"
assert_eq 600 "$(stat -f '%Lp' "$TMP_ROOT/runtime/config/github-token")"
printf 'PASS github auth\n'
