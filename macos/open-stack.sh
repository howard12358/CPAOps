#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"

root=$(runtime_root)
[ -d "$root" ] || { printf 'CPAStack runtime directory does not exist: %s\n' "$root" >&2; exit 1; }
osascript - "$root" <<'APPLESCRIPT'
on run argv
  set stackRoot to item 1 of argv
  tell application "Terminal"
    activate
    if (count of windows) is 0 then
      do script "cd " & quoted form of stackRoot
    else
      do script "cd " & quoted form of stackRoot in front window
    end if
  end tell
end run
APPLESCRIPT
