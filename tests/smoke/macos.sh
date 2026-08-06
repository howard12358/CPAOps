#!/bin/sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/cpactl-smoke.XXXXXX")
trap 'rm -rf "$TEMP_DIR"' EXIT HUP INT TERM
RUNTIME_ROOT="$TEMP_DIR/cpa-stack"
SMOKE_HOME="$TEMP_DIR/home"
BINARY="$ROOT_DIR/target/debug/cpactl"

cd "$ROOT_DIR"
test "$(rustc -vV | sed -n 's/^host: //p')" = 'aarch64-apple-darwin'
cargo build --offline --quiet
mkdir -p "$SMOKE_HOME"

cpactl() {
    HOME="$SMOKE_HOME" CPACTL_SMOKE_NO_PLATFORM_COMMANDS=1 "$BINARY" "$@"
}

cpactl --help | grep -F 'rollback' >/dev/null
test "$(cpactl path --root "$RUNTIME_ROOT")" = "$RUNTIME_ROOT"

# The reserved TCP destination port zero makes install stop after safe
# configuration initialization, before it can contact GitHub or start a service.
HTTPS_PROXY='http://smoke-user:smoke-secret@127.0.0.1:0' \
    cpactl --root "$RUNTIME_ROOT" proxy set >/dev/null
proxy_json=$(cpactl --root "$RUNTIME_ROOT" --json proxy show)
printf '%s' "$proxy_json" | grep -F '"configured":true' >/dev/null
case "$proxy_json" in
    *smoke-secret*)
        echo '代理密钥泄露到 JSON 输出' >&2
        exit 1
        ;;
esac

set +e
CPA_MANAGEMENT_KEY='smoke-management-key' \
    KEEPER_LOGIN_PASSWORD='smoke-keeper-password' \
    cpactl --root "$RUNTIME_ROOT" install >/dev/null 2>&1
install_status=$?
set -e
test "$install_status" -eq 5
test -f "$RUNTIME_ROOT/config/config.yaml"
test -f "$RUNTIME_ROOT/config/keeper.env"
! grep -F '__REQUIRED__' "$RUNTIME_ROOT/config/config.yaml" >/dev/null
! grep -F '__REQUIRED__' "$RUNTIME_ROOT/config/keeper.env" >/dev/null
test ! -e "$RUNTIME_ROOT/downloads/cli-proxy-api"

status_json=$(cpactl --root "$RUNTIME_ROOT" --json status)
printf '%s' "$status_json" | grep -F '"ok":true' >/dev/null
printf '%s' "$status_json" | grep -F '"services"' >/dev/null

mkdir -p "$RUNTIME_ROOT/logs"
printf 'old-line\nsmoke-out\n' >"$RUNTIME_ROOT/logs/cli-proxy-api.out.log"
printf 'smoke-error\n' >"$RUNTIME_ROOT/logs/cli-proxy-api.err.log"
logs_json=$(cpactl --root "$RUNTIME_ROOT" --json logs cli -n 1)
printf '%s' "$logs_json" | grep -F 'smoke-out' >/dev/null
printf '%s' "$logs_json" | grep -F 'smoke-error' >/dev/null
case "$logs_json" in
    *old-line*)
        echo '日志行数限制未生效' >&2
        exit 1
        ;;
esac

set +e
cpactl --root "$RUNTIME_ROOT" stop cli >/dev/null 2>&1
stop_status=$?
set -e
# The temporary root has no LaunchAgent, so launchctl may return the service
# error code after the marker is safely written.
test "$stop_status" -eq 0 || test "$stop_status" -eq 7
test -f "$RUNTIME_ROOT/state/cli-proxy-api.disabled"

set +e
cpactl --root "$RUNTIME_ROOT" uninstall --purge </dev/null >/dev/null 2>&1
purge_status=$?
set -e
test "$purge_status" -eq 2
test -d "$RUNTIME_ROOT"
