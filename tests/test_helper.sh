#!/bin/sh

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/cpa-deploy-test.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
assert_eq() { [ "$1" = "$2" ] || fail "expected [$1], got [$2]"; }
assert_file() { [ -f "$1" ] || fail "expected file: $1"; }
assert_dir() { [ -d "$1" ] || fail "expected directory: $1"; }
assert_not_file() { [ ! -e "$1" ] || fail "did not expect path: $1"; }
assert_contains() { case "$2" in *"$1"*) ;; *) fail "expected [$2] to contain [$1]" ;; esac; }
assert_fails() { if ( "$@" ) >/dev/null 2>&1; then fail "expected command to fail: $*"; fi; }
