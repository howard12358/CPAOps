#!/bin/sh

set -eu
. "$(dirname "$0")/test_helper.sh"
. "$ROOT/macos/lib/common.sh"
. "$ROOT/macos/lib/proxy.sh"

CPA_STACK_ROOT="$TMP_ROOT/runtime"
export CPA_STACK_ROOT
ensure_runtime_layout
assert_fails parse_proxy_line 'export https_proxy=not-a-url'
parse_proxy_line 'export https_proxy=http://127.0.0.1:7897 http_proxy=http://127.0.0.1:7897 all_proxy=socks5://127.0.0.1:7897'
save_proxy
unset https_proxy http_proxy all_proxy
load_proxy
assert_eq http://127.0.0.1:7897 "$https_proxy"
assert_eq socks5://127.0.0.1:7897 "$all_proxy"
assert_eq 600 "$(stat -f '%Lp' "$TMP_ROOT/runtime/config/proxy.env")"
printf 'PASS proxy\n'
