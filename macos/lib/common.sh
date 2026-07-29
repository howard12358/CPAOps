#!/bin/sh

runtime_root() {
  printf '%s\n' "${CPA_STACK_ROOT:-$HOME/Library/Application Support/cpa-stack}"
}

require_macos_arm64() {
  os=${UNAME_S:-$(uname -s)}
  arch=${UNAME_M:-$(uname -m)}
  [ "$os" = Darwin ] && [ "$arch" = arm64 ] || {
    printf '%s\n' 'This installer supports only macOS Apple Silicon (Darwin arm64).' >&2
    return 1
  }
}

require_commands() {
  for command_name in "$@"; do
    command -v "$command_name" >/dev/null 2>&1 || {
      printf 'Required command not found: %s\n' "$command_name" >&2
      return 1
    }
  done
}

ensure_runtime_layout() {
  root=$(runtime_root)
  umask 077
  mkdir -p "$root/config" "$root/auths" "$root/keeper" "$root/releases" \
    "$root/current" "$root/downloads" "$root/logs" "$root/state" "$root/bin"
  chmod 700 "$root/auths" "$root/keeper" "$root/releases" "$root/current" \
    "$root/downloads" "$root/logs" "$root/state" "$root/bin"
}

service_name() {
  case "$1" in
    cli|cli-proxy-api) printf '%s\n' cli-proxy-api ;;
    keeper|cpa-usage-keeper) printf '%s\n' cpa-usage-keeper ;;
    *) printf 'Unknown service: %s\n' "$1" >&2; return 1 ;;
  esac
}

service_label() {
  case "$(service_name "$1")" in
    cli-proxy-api) printf '%s\n' io.cpa-local.cli-proxy-api ;;
    cpa-usage-keeper) printf '%s\n' io.cpa-local.usage-keeper ;;
  esac
}

service_targets() {
  case "${1:-all}" in
    all) printf '%s\n' cli keeper ;;
    cli|cli-proxy-api|keeper|cpa-usage-keeper) service_name "$1" ;;
    *) printf 'Unknown service: %s\n' "$1" >&2; return 1 ;;
  esac
}
