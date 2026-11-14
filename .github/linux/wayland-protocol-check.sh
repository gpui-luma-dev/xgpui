#!/usr/bin/env bash
# Checks the Wayland client protocol recorded by window-smoke.sh (WAYLAND_DEBUG=client)
# for #284 package 08. Usage: wayland-protocol-check.sh RESULTS_DIR
#
# - No protocol error from the compositor.
# - If the compositor offers xdg_activation_v1: every activation token that came
#   back was used, and none is left unpaired (the bug that kept GNOME's launch
#   indicator up). Without the global, the result says so.
# - The opaque-region requests are counted for evidence.
set -euo pipefail
results=${1:?results directory}
log="$results/wayland-smoke.log"
[[ -s "$log" ]] || { echo "missing Wayland smoke log: $log"; exit 1; }
summary="$results/wayland-protocol-check.txt"
: > "$summary"
say() { printf '%s\n' "$*" | tee -a "$summary"; }

count() { grep -cE "$1" "$log" || true; }

requests=$(count '(->|-> ).*[@#][0-9]+\.')
[[ "$requests" -gt 0 ]] || { say "FAIL: no client protocol recorded (is WAYLAND_DEBUG supported?)"; exit 1; }
say "client requests recorded: $requests"

errors=$(count 'wl_display[@#]1\.error\(')
if [[ "$errors" -gt 0 ]]; then
    say "FAIL: compositor sent $errors protocol error(s):"
    grep -E 'wl_display[@#]1\.error\(' "$log" | head -5 | tee -a "$summary"
    exit 1
fi
say "protocol errors: 0"

if grep -qE 'global\([0-9]+, "xdg_activation_v1"' "$log"; then
    asked=$(count 'xdg_activation_v1[@#][0-9]+\.get_activation_token\(')
    done_events=$(count 'xdg_activation_token_v1[@#][0-9]+\.done\(')
    activated=$(count 'xdg_activation_v1[@#][0-9]+\.activate\(')
    say "activation: offered; tokens requested $asked, returned $done_events, used $activated"
    if [[ "$activated" -ne "$done_events" ]]; then
        say "FAIL: $done_events token(s) returned but $activated used (unpaired activation token)"
        exit 1
    fi
    if [[ "$asked" -lt "$done_events" ]]; then
        say "FAIL: more tokens returned than requested"
        exit 1
    fi
else
    say "activation: compositor does not offer xdg_activation_v1; pairing not exercised"
fi

say "opaque-region requests: $(count 'wl_surface[@#][0-9]+\.set_opaque_region\(')"
say "Wayland protocol check passed"
