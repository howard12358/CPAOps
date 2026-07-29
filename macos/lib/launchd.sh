#!/bin/sh

script_dir=${DEPLOY_REPO_ROOT:?DEPLOY_REPO_ROOT must be set by the entry script}/macos

render_plist() {
  service=$(service_name "$1") || return 1
  root=$(runtime_root) label=$(service_label "$service")
  mkdir -p "$HOME/Library/LaunchAgents"
  sed "s|__ROOT__|$root|g" "$script_dir/launchd/$label.plist.in" > "$(service_plist "$service")"
  plutil -lint "$(service_plist "$service")" >/dev/null
}

write_wrappers() {
  root=$(runtime_root)
  cat > "$root/bin/run-cli-proxy-api" <<EOF
#!/bin/sh
[ -f "$root/state/cli-proxy-api.disabled" ] && exit 0
exec "$root/current/cli-proxy-api/cli-proxy-api" -config "$root/config/config.yaml"
EOF
  cat > "$root/bin/run-cpa-usage-keeper" <<EOF
#!/bin/sh
[ -f "$root/state/cpa-usage-keeper.disabled" ] && exit 0
exec "$root/current/cpa-usage-keeper/cpa-usage-keeper" -env "$root/config/keeper.env"
EOF
  chmod 700 "$root/bin/run-cli-proxy-api" "$root/bin/run-cpa-usage-keeper"
}

render_plists() { render_plist cli && render_plist keeper && write_wrappers; }
