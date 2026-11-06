# Linux extension validation

`linux-extensions.yml` runs on relevant pull requests, pushes to main and manual
dispatch. Its path filters include all apps, shared lab code, extensions, core
backends and Cargo configuration. The existing paint and native-library/font
workflows remain independent.

## Hosted job

The Ubuntu job uses committed lockfiles and a shared Cargo target/cache. It runs:

- All seven extension library suites, color/ICC integration tests and the native
  Parley construction/interop tests, with the relevant Linux features enabled.
- Experimental `xgpui-shell` appearance, capability and title-lifecycle tests.
- All native lab test binaries, the shared lab library and compatibility tests.
- Vulkan image and path captures, saved with per-suite logs and adapter details.
- A dedicated real-window probe under Weston nested in Xvfb, and directly under
  Xvfb/Openbox. It opens default and requested client-decorated windows, completes
  rendered frames,
  checks per-window titles, decoration insets and color fallback, then exits.

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
GPUI; Ubuntu 24.04’s headless backend does not advertise `wl_seat`. The Wayland
probe clears `DISPLAY` to ensure it connects through Wayland.

The script checks server readiness, imposes a timeout on each probe and cleans up
its display-server processes on success or failure. Use a private runtime directory.

Hosted Vulkan may use a software Mesa adapter. These results do not establish
real DMA-BUF hardware support, GNOME gestures, input methods, accessibility,
physical display coverage or sustained playback performance.

## Native GPU job

Enable the `native_dma_buf` input in a manual workflow dispatch to request the
separate native job. Register a Linux x64 runner with the additional label
`xgpui-vulkan`. That runner needs Rust/build prerequisites, accessible Vulkan GPU
render devices, `vulkaninfo`, and the Khronos validation layer. On Fedora, the
additional diagnostics packages are `vulkan-tools` and `vulkan-validation-layers`.
The GPU must support the current immutable linear BGRA DMA-BUF export/import
contract; a software adapter or unavailable validation layer must fail this job,
not silently count as native acceptance.

The job runs the core DMA-BUF test, the synthetic-video pixel/lifetime test and
the actual headless video capture command. Vulkan synchronization validation is
enabled and its errors are counted by the native tests. Native logs, adapter
information and the video capture are uploaded separately from hosted results.
This job is dispatch-only and does not run untrusted pull requests automatically.
No self-hosted runner is provisioned by adding the workflow.
