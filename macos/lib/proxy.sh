#!/bin/sh

proxy_file() { printf '%s/config/proxy.env\n' "$(runtime_root)"; }

valid_proxy_url() {
  case "$1" in http://*|https://*|socks5://*) case "$1" in *[![:print:]]*|*[[:space:]]*) return 1 ;; *) return 0 ;; esac ;; *) return 1 ;; esac
}

parse_proxy_assignment() {
  assignment=$1
  key=${assignment%%=*}
  value=${assignment#*=}
  [ "$key" != "$assignment" ] && valid_proxy_url "$value" || return 1
  case "$key" in
    https_proxy) proxy_https=$value ;;
    http_proxy) proxy_http=$value ;;
    all_proxy) proxy_all=$value ;;
    *) return 1 ;;
  esac
}

parse_proxy_line() {
  proxy_https='' proxy_http='' proxy_all=''
  set -f
  set -- $1
  set +f
  [ "${1:-}" != export ] || shift
  [ "$#" -gt 0 ] || return 1
  for assignment in "$@"; do parse_proxy_assignment "$assignment" || return 1; done
  [ -n "$proxy_https$proxy_http$proxy_all" ]
}

save_proxy() {
  file=$(proxy_file)
  umask 077
  : > "$file"
  [ -z "$proxy_https" ] || printf 'https_proxy=%s\n' "$proxy_https" >> "$file"
  [ -z "$proxy_http" ] || printf 'http_proxy=%s\n' "$proxy_http" >> "$file"
  [ -z "$proxy_all" ] || printf 'all_proxy=%s\n' "$proxy_all" >> "$file"
  chmod 600 "$file"
  export https_proxy="$proxy_https" http_proxy="$proxy_http" all_proxy="$proxy_all"
}

load_proxy() {
  [ -z "${https_proxy:-}${http_proxy:-}${all_proxy:-}" ] || return 0
  file=$(proxy_file)
  [ -f "$file" ] || return 1
  proxy_https='' proxy_http='' proxy_all=''
  while IFS= read -r assignment || [ -n "$assignment" ]; do
    parse_proxy_assignment "$assignment" || { printf 'Invalid saved proxy configuration.\n' >&2; return 1; }
  done < "$file"
  [ -n "$proxy_https$proxy_http$proxy_all" ] || return 1
  export https_proxy="$proxy_https" http_proxy="$proxy_http" all_proxy="$proxy_all"
}

prompt_proxy() {
  printf 'Optional proxy (paste export ... line, or press Enter to skip): ' >&2
  IFS= read -r proxy_line
  [ -n "$proxy_line" ] || return 0
  parse_proxy_line "$proxy_line" || { printf 'Invalid proxy format; expected http(s):// or socks5:// URLs.\n' >&2; return 1; }
  save_proxy
}
