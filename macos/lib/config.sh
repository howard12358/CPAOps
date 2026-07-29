#!/bin/sh

private_file_ok() {
  file=$1
  [ -f "$file" ] || return 1
  [ "$(stat -f '%Lp' "$file")" = 600 ]
}

require_no_placeholder() {
  ! grep -q '__REQUIRED__' "$1"
}

validate_config() {
  root=$(runtime_root)
  cpa_config="$root/config/config.yaml"
  keeper_env="$root/config/keeper.env"
  private_file_ok "$cpa_config" || { printf '%s\n' "Invalid private config: $cpa_config" >&2; return 1; }
  private_file_ok "$keeper_env" || { printf '%s\n' "Invalid private config: $keeper_env" >&2; return 1; }
  require_no_placeholder "$cpa_config" || { printf '%s\n' 'CPA config contains required placeholders.' >&2; return 1; }
  require_no_placeholder "$keeper_env" || { printf '%s\n' 'Keeper env contains required placeholders.' >&2; return 1; }
  grep -Eq '^[[:space:]]*port:[[:space:]]*[1-9][0-9]{0,4}[[:space:]]*$' "$cpa_config" || {
    printf '%s\n' 'CPA config must contain a valid port.' >&2; return 1;
  }
  grep -Eq '^APP_PORT=[1-9][0-9]{0,4}$' "$keeper_env" || {
    printf '%s\n' 'Keeper env must contain APP_PORT.' >&2; return 1;
  }
}
