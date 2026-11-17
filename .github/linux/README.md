# Linux window probes

These scripts run in the Linux job of `.github/workflows/epic-284.yml` on every
pull request to `main`, and can be run locally:

- `window-smoke.sh`: real windows under Weston (nested in Xvfb) and directly under
  Xvfb/Openbox. It opens default and requested client-decorated windows, completes
  rendered frames, checks per-window titles, decoration insets and color fallback,
  then exits.
- `mutter-smoke.sh`: real windows under headless Mutter, which offers
  `xdg_activation_v1`.
- `wayland-protocol-check.sh`: checks the Wayland client protocol the smoke runs
  recorded.

Extension suites, lab tests, Vulkan captures and native DMA-BUF/video checks run
locally with `just test` on Linux.

## Run the window probe locally

The window probe is opt-in (`linux-window-tests`) and is not part of normal
headless `lab-test`. Build it before a bounded display-server run:

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --features linux-window-tests,runtime-shaders,portable-shaders,wgpu \
  --test linux_windows --no-run
```

With Weston, Xvfb, Openbox and X11 utilities installed, run the same orchestration:

```sh
export CARGO_TARGET_DIR="$PWD/target/linux-ci"
export LINUX_CI_RESULTS="$PWD/target/linux-ci-results"
export XDG_RUNTIME_DIR="$(mktemp -d)"
export WGPU_BACKEND=vulkan
dbus-run-session -- bash .github/linux/window-smoke.sh
rmdir "$XDG_RUNTIME_DIR"
```

Weston uses its X11 backend on Xvfb so it provides the input seat required by
GPUI; Ubuntu 24.04's headless backend does not advertise `wl_seat`. The Wayland
probe clears `DISPLAY` to ensure it connects through Wayland.

The script checks server readiness, imposes a timeout on each probe and cleans up
its display-server processes on success or failure. Use a private runtime directory.

Hosted Vulkan may use a software Mesa adapter. These results do not establish
real DMA-BUF hardware support, GNOME gestures, input methods, accessibility,
physical display coverage or sustained playback performance.
