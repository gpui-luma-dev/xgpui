#!/usr/bin/env bash
# Real protocol/paint smoke, not a claim about GNOME gestures or physical output.
set -euo pipefail
: "${XDG_RUNTIME_DIR:?set XDG_RUNTIME_DIR to a private runtime directory}"
: "${LINUX_CI_RESULTS:?set LINUX_CI_RESULTS for display logs}"
mkdir -p "$XDG_RUNTIME_DIR" "$LINUX_CI_RESULTS"
chmod 700 "$XDG_RUNTIME_DIR"
weston_pid=''
xvfb_pid=''
wm_pid=''
cleanup() {
    for pid in "$wm_pid" "$xvfb_pid" "$weston_pid"; do
        if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
    done
}
trap cleanup EXIT
common=(test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --features linux-window-tests,runtime-shaders,portable-shaders,wgpu --test linux_windows)
weston --backend=headless-backend.so --renderer=pixman --socket=wayland-ci --idle-time=0 --no-config --log="$LINUX_CI_RESULTS/weston.log" > "$LINUX_CI_RESULTS/weston-stdout.log" 2>&1 &
weston_pid=$!
for ((attempt=0; attempt<100; attempt++)); do
    kill -0 "$weston_pid" 2>/dev/null || { cat "$LINUX_CI_RESULTS/weston.log"; exit 1; }
    [[ -S "$XDG_RUNTIME_DIR/wayland-ci" ]] && break
    sleep 0.1
done
[[ -S "$XDG_RUNTIME_DIR/wayland-ci" ]] || { echo 'Weston socket did not become ready'; exit 1; }
env -u DISPLAY WAYLAND_DISPLAY=wayland-ci timeout 60s cargo "${common[@]}" 2>&1 | tee "$LINUX_CI_RESULTS/wayland-smoke.log"
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp > "$LINUX_CI_RESULTS/xvfb.log" 2>&1 &
xvfb_pid=$!
for ((attempt=0; attempt<100; attempt++)); do
    kill -0 "$xvfb_pid" 2>/dev/null || { cat "$LINUX_CI_RESULTS/xvfb.log"; exit 1; }
    xdpyinfo -display :99 >/dev/null 2>&1 && break
    sleep 0.1
done
xdpyinfo -display :99 >/dev/null 2>&1 || { echo 'Xvfb did not become ready'; exit 1; }
DISPLAY=:99 openbox --sm-disable > "$LINUX_CI_RESULTS/openbox.log" 2>&1 &
wm_pid=$!
for ((attempt=0; attempt<100; attempt++)); do
    kill -0 "$wm_pid" 2>/dev/null || { cat "$LINUX_CI_RESULTS/openbox.log"; exit 1; }
    if xprop -display :99 -root _NET_SUPPORTING_WM_CHECK | grep -q 'window id'; then break; fi
    sleep 0.1
done
xprop -display :99 -root _NET_SUPPORTING_WM_CHECK | grep -q 'window id'
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET DISPLAY=:99 timeout 60s cargo "${common[@]}" 2>&1 | tee "$LINUX_CI_RESULTS/x11-smoke.log"
