#!/bin/sh
set -eu
DEPLOY_REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$DEPLOY_REPO_ROOT/macos/lib/common.sh"
. "$DEPLOY_REPO_ROOT/macos/lib/proxy.sh"
ensure_runtime_layout
case "${1:-}" in
  set) prompt_proxy ;;
  clear) rm -f "$(proxy_file)"; printf 'Saved proxy removed.\n' ;;
  show) if load_proxy; then printf 'Proxy is configured.\n'; else printf 'No proxy configured.\n'; fi ;;
  *) printf 'Usage: %s set|clear|show\n' "$0" >&2; exit 2 ;;
esac
