#!/usr/bin/env bash
# Real windows under headless Mutter (GNOME's compositor), which offers
# xdg_activation_v1, so #284 package 08's activation pairing is exercised.
# Records the client protocol for wayland-protocol-check.sh.
# Usage: mutter-smoke.sh (needs XDG_RUNTIME_DIR, LINUX_CI_RESULTS, a D-Bus session)
set -euo pipefail
: "${XDG_RUNTIME_DIR:?set XDG_RUNTIME_DIR to a private runtime directory}"
: "${LINUX_CI_RESULTS:?set LINUX_CI_RESULTS for display logs}"
results="$LINUX_CI_RESULTS/mutter"
mkdir -p "$XDG_RUNTIME_DIR" "$results"
chmod 700 "$XDG_RUNTIME_DIR"
socket=wayland-mutter
mutter_pid=''
cleanup() {
    if [[ -n "$mutter_pid" ]]; then kill "$mutter_pid" 2>/dev/null || true; wait "$mutter_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT

flags=(--headless --wayland --virtual-monitor 1280x800 --wayland-display "$socket")
# Skip Xwayland where this Mutter supports it; the windows under test are native Wayland.
if mutter --help 2>&1 | grep -q -- '--no-x11'; then flags+=(--no-x11); fi
mutter --version 2>&1 | tee "$results/mutter-version.log"
env -u DISPLAY -u WAYLAND_DISPLAY mutter "${flags[@]}" > "$results/mutter.log" 2>&1 &
mutter_pid=$!
for ((attempt = 0; attempt < 200; attempt++)); do
    kill -0 "$mutter_pid" 2>/dev/null || { echo "Mutter exited:"; cat "$results/mutter.log"; exit 1; }
    [[ -S "$XDG_RUNTIME_DIR/$socket" ]] && break
    sleep 0.1
done
[[ -S "$XDG_RUNTIME_DIR/$socket" ]] || { echo 'Mutter socket did not become ready'; cat "$results/mutter.log"; exit 1; }

common=(test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml
    --features linux-window-tests,runtime-shaders,portable-shaders,wgpu --test linux_windows)
env -u DISPLAY WAYLAND_DISPLAY="$socket" WAYLAND_DEBUG=client timeout 90s cargo "${common[@]}" 2>&1 |
    tee "$results/wayland-smoke.log"
