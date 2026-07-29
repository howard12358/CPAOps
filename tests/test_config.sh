#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"
. "$ROOT/macos/lib/common.sh"
. "$ROOT/macos/lib/config.sh"

CPA_STACK_ROOT="$TMP_ROOT" ensure_runtime_layout
printf 'port: 8317\nremote-management:\n  secret-key: __REQUIRED__\n' > "$TMP_ROOT/config/config.yaml"
printf 'CPA_MANAGEMENT_KEY=__REQUIRED__\nAPP_PORT=18080\n' > "$TMP_ROOT/config/keeper.env"
chmod 600 "$TMP_ROOT/config/config.yaml" "$TMP_ROOT/config/keeper.env"
assert_fails validate_config
sed -i '' 's/__REQUIRED__/secret/g' "$TMP_ROOT/config/config.yaml" "$TMP_ROOT/config/keeper.env"
validate_config
chmod 644 "$TMP_ROOT/config/keeper.env"
assert_fails validate_config
chmod 600 "$TMP_ROOT/config/keeper.env"
ensure_keeper_defaults
assert_contains 'CPA_PUBLIC_URL=http://127.0.0.1:8317' "$(cat "$TMP_ROOT/config/keeper.env")"
printf 'PASS config\n'
