#!/bin/sh

disabled_file() { printf '%s/state/%s.disabled\n' "$(runtime_root)" "$(service_name "$1")"; }
set_disabled() { : > "$(disabled_file "$1")"; }
clear_disabled() { rm -f "$(disabled_file "$1")"; }
is_disabled() { [ -f "$(disabled_file "$1")" ]; }

service_plist() { printf '%s/Library/LaunchAgents/%s.plist\n' "$HOME" "$(service_label "$1")"; }
service_domain() { printf 'gui/%s\n' "$(id -u)"; }
launchctl_cmd() { "${LAUNCHCTL:-launchctl}" "$@"; }
is_registered() { launchctl_cmd print "$(service_domain)/$(service_label "$1")" >/dev/null 2>&1; }
bootstrap_service() { launchctl_cmd bootstrap "$(service_domain)" "$(service_plist "$1")"; }
kickstart_service() { launchctl_cmd kickstart -k "$(service_domain)/$(service_label "$1")"; }
stop_service() { set_disabled "$1"; launchctl_cmd kill SIGTERM "$(service_domain)/$(service_label "$1")" 2>/dev/null || true; }
