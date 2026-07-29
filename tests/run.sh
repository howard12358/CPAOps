#!/bin/sh

set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
status=0
for test_file in "$root"/tests/test_*.sh; do
  [ -f "$test_file" ] || continue
  [ "$(basename "$test_file")" = test_helper.sh ] && continue
  printf 'RUN %s\n' "$(basename "$test_file")"
  sh "$test_file" || status=1
done
exit "$status"
