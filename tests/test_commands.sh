#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"

assert_fails sh "$ROOT/macos/start.sh" unknown
output=$(CPA_STACK_ROOT="$TMP_ROOT" sh "$ROOT/macos/start.sh" cli 2>&1 || true)
assert_contains 'first run install.sh' "$output"
printf 'PASS commands\n'
