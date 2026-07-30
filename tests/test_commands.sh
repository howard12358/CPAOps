#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"

assert_fails sh "$ROOT/macos/start.sh" unknown
fake_launchctl="$TMP_ROOT/launchctl"
printf '#!/bin/sh\nexit 1\n' > "$fake_launchctl"
chmod 700 "$fake_launchctl"
output=$(CPA_STACK_ROOT="$TMP_ROOT" LAUNCHCTL="$fake_launchctl" sh "$ROOT/macos/start.sh" cli 2>&1 || true)
assert_contains 'first run install.sh' "$output"
assert_fails sh "$ROOT/macos/logs.sh"
assert_fails sh "$ROOT/macos/logs.sh" unknown
printf 'PASS commands\n'
