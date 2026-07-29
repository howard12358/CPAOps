#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"
. "$ROOT/macos/lib/common.sh"

assert_fails env UNAME_S=Linux UNAME_M=x86_64 CPA_STACK_ROOT="$TMP_ROOT" sh -c '. "$1"; require_macos_arm64' sh "$ROOT/macos/lib/common.sh"
CPA_STACK_ROOT="$TMP_ROOT" ensure_runtime_layout
assert_dir "$TMP_ROOT/releases"
assert_eq 700 "$(stat -f '%Lp' "$TMP_ROOT/keeper")"
printf 'PASS common\n'
