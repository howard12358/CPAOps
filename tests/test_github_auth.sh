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
stub_dir="$TMP_ROOT/stub"
mkdir "$stub_dir"
cat > "$stub_dir/curl" <<'EOF'
#!/bin/sh
case " $* " in
  *' Authorization: Bearer test-token '*) printf '200'; exit 0 ;;
  *) printf 'simulated anonymous denial\n' >&2; printf '403'; exit 22 ;;
esac
EOF
chmod 700 "$stub_dir/curl"
if output=$(PATH="$stub_dir:$PATH" github_curl --fail --silent -o "$TMP_ROOT/output" https://example.invalid 2>&1); then :; else fail 'token retry should succeed'; fi
assert_eq '' "$output"
printf 'PASS github auth\n'
