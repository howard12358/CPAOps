#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"
. "$ROOT/macos/lib/common.sh"
. "$ROOT/macos/lib/releases.sh"

token_file="$TMP_ROOT/token"
printf 'test-token\n' > "$token_file"
token=$(github_token "$token_file")
assert_eq test-token "$token"
printf 'PASS github auth\n'
