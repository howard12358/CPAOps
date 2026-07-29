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

activate_release() {
  service=$(service_name "$1") || return 1
  version=$2
  root=$(runtime_root)
  target="$root/releases/$service/$version"
  [ -x "$target/$(release_binary "$service")" ] || return 1
  temporary="$root/current/$service.next"
  rm -f "$temporary"
  ln -s "../releases/$service/$version" "$temporary"
  mv -f "$temporary" "$root/current/$service"
}

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
  curl --fail --location --silent --show-error "https://api.github.com/repos/$repository/releases/latest" -o "$metadata" || return 1
  case "$service" in
    cli-proxy-api) pattern='CLIProxyAPI_[0-9.]+_darwin_aarch64\.tar\.gz' ;;
    cpa-usage-keeper) pattern='cpa-usage-keeper_v[0-9.]+_darwin_arm64\.tar\.gz' ;;
  esac
  asset=$(grep -Eo '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+' "$metadata" | sed 's/.*"//' | grep -E "/$pattern$" | head -n 1)
  sums=$(grep -Eo '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+' "$metadata" | sed 's/.*"//' | grep '/checksums\.txt$' | head -n 1)
  [ -n "$asset" ] && [ -n "$sums" ] || return 1
  printf '%s\n%s\n' "$asset" "$sums"
}
