#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"
. "$ROOT/macos/lib/common.sh"
. "$ROOT/macos/lib/releases.sh"

CPA_STACK_ROOT="$TMP_ROOT" ensure_runtime_layout
mkdir -p "$TMP_ROOT/releases/cli-proxy-api/1.0.0"
printf '#!/bin/sh\nexit 0\n' > "$TMP_ROOT/releases/cli-proxy-api/1.0.0/cli-proxy-api"
chmod 700 "$TMP_ROOT/releases/cli-proxy-api/1.0.0/cli-proxy-api"
ln -s ../releases/cli-proxy-api/1.0.0 "$TMP_ROOT/current/cli-proxy-api"
old_link=$(readlink "$TMP_ROOT/current/cli-proxy-api")
printf 'bad archive' > "$TMP_ROOT/bad.tar.gz"
printf '0000  bad.tar.gz\n' > "$TMP_ROOT/checksums.txt"
assert_fails install_release cli 1.2.3 "$TMP_ROOT/bad.tar.gz" "$TMP_ROOT/checksums.txt"
assert_eq "$old_link" "$(readlink "$TMP_ROOT/current/cli-proxy-api")"
target=cli
activate_release cli 1.0.0
assert_eq cli "$target"
assert_eq ../releases/cli-proxy-api/1.0.0 "$(readlink "$TMP_ROOT/current/cli-proxy-api")"
printf 'PASS releases\n'
