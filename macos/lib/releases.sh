#!/bin/sh

release_binary() {
  case "$(service_name "$1")" in
    cli-proxy-api) printf '%s\n' cli-proxy-api ;;
    cpa-usage-keeper) printf '%s\n' cpa-usage-keeper ;;
  esac
}

release_repository() {
  case "$(service_name "$1")" in
    cli-proxy-api) printf '%s\n' router-for-me/CLIProxyAPI ;;
    cpa-usage-keeper) printf '%s\n' Willxup/cpa-usage-keeper ;;
  esac
}

github_token() {
  token_file="$(runtime_root)/config/github-token"
  [ -r "$token_file" ] || return 0
  tr -d '\r\n' < "$token_file"
}

save_github_token() {
  token_file="$(runtime_root)/config/github-token"
  umask 077
  printf '%s\n' "$1" > "$token_file"
  chmod 600 "$token_file"
}

prompt_github_token() {
  [ -t 0 ] || { printf '%s\n' 'GitHub denied the request (401/403). Re-run interactively to provide a GitHub token.' >&2; return 1; }
  printf 'GitHub token (input hidden): ' >&2
  stty -echo
  trap 'stty echo' EXIT HUP INT TERM
  IFS= read -r token
  stty echo
  trap - EXIT HUP INT TERM
  printf '\n' >&2
  [ -n "$token" ] || return 1
  save_github_token "$token"
  printf '%s\n' "$token"
}

github_curl() {
  http_code=$(curl "$@" --write-out '%{http_code}')
  curl_result=$?
  [ "$curl_result" -eq 0 ] && return 0
  case "$http_code" in
    401|403) ;;
    *) return "$curl_result" ;;
  esac
  token=$(github_token)
  if [ -n "$token" ]; then
    http_code=$(curl -H "Authorization: Bearer $token" "$@" --write-out '%{http_code}')
    curl_result=$?
    [ "$curl_result" -eq 0 ] && return 0
    case "$http_code" in 401|403) ;; *) return "$curl_result" ;; esac
  fi
  token=$(prompt_github_token) || return 1
  curl -H "Authorization: Bearer $token" "$@"
}

activate_release() (
  service=$(service_name "$1") || return 1
  version=$2
  root=$(runtime_root)
  target="$root/releases/$service/$version"
  [ -x "$target/$(release_binary "$service")" ] || return 1
  temporary="$root/current/$service.next"
  rm -f "$temporary"
  ln -s "../releases/$service/$version" "$temporary"
  mv -f "$temporary" "$root/current/$service"
)

install_release() {
  service=$(service_name "$1") || return 1
  version=$2 archive=$3 sums=$4
  root=$(runtime_root) binary=$(release_binary "$service")
  expected=$(awk -v file="$(basename "$archive")" '$2 == file { print $1; exit }' "$sums")
  [ -n "$expected" ] || { printf '%s\n' 'No matching checksum found.' >&2; return 1; }
  actual=$(shasum -a 256 "$archive" | awk '{print $1}')
  [ "$expected" = "$actual" ] || { printf '%s\n' 'Checksum verification failed.' >&2; return 1; }
  destination="$root/releases/$service/$version"
  [ -d "$destination" ] || {
    temporary="$destination.tmp.$$"
    rm -rf "$temporary"
    mkdir -p "$temporary" || return 1
    tar -xzf "$archive" -C "$temporary" || { rm -rf "$temporary"; return 1; }
    if [ ! -x "$temporary/$binary" ]; then
      candidate=$(find "$temporary" -type f -name "$binary" -print | head -n 1)
      [ -n "$candidate" ] && mv "$candidate" "$temporary/$binary"
    fi
    chmod 700 "$temporary/$binary" 2>/dev/null || { rm -rf "$temporary"; return 1; }
    "$temporary/$binary" --help >/dev/null 2>&1 || { rm -rf "$temporary"; return 1; }
    mv "$temporary" "$destination"
  }
  activate_release "$service" "$version"
}

fetch_latest_release() {
  service=$(service_name "$1") || return 1
  root=$(runtime_root) repository=$(release_repository "$service")
  metadata="$root/downloads/$service-release.json"
  github_curl --fail --location --silent --show-error "https://api.github.com/repos/$repository/releases/latest" -o "$metadata" || return 1
  case "$service" in
    cli-proxy-api) pattern='CLIProxyAPI_[0-9.]+_darwin_aarch64\.tar\.gz' ;;
    cpa-usage-keeper) pattern='cpa-usage-keeper_v[0-9.]+_darwin_arm64\.tar\.gz' ;;
  esac
  asset=$(grep -Eo '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+' "$metadata" | sed 's/.*"//' | grep -E "/$pattern$" | head -n 1)
  sums=$(grep -Eo '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+' "$metadata" | sed 's/.*"//' | grep '/checksums\.txt$' | head -n 1)
  [ -n "$asset" ] && [ -n "$sums" ] || return 1
  printf '%s\n%s\n' "$asset" "$sums"
}
