# WebAssembly demo index and extension examples

A standalone Wasm app for `xgpui-shader`, `xgpui-parley`, and `xgpui-svg`. All use the existing
GPUI web platform and wgpu renderer. The gallery offers automatic selection
(WebGPU with WebGL2 fallback), forced WebGPU, and forced WebGL2. A forced backend
fails explicitly if unavailable; check the browser console for adapter details.

The landing page also lists all seven interactive examples from the older
`gpui-web/examples/hello_web` gallery and its three browser-test Wasm modules.
These entries link to source and are explicitly not hosted. The captured gallery
still uses pre-normalization crate paths; porting/building it for publication is
separate from maintaining this index. Native-only labs are not listed as Wasm.

From the repository root:

```sh
rustup target add wasm32-unknown-unknown
# Match the version recorded for wasm-bindgen in this app's Cargo.lock.
cargo install wasm-bindgen-cli --version 0.2.129 --locked
bash apps/extensions-web/build.sh
python3 apps/extensions-web/serve.py
```

Open <http://localhost:8081/>. Add `--release` to the build command for an optimized
build. The server accepts `--port 8082` if needed. Stop it with Ctrl-C; rebuild and
reload after changes. The build also refreshes `docs/examples/extensions/` for
local documentation previews, so `/examples/extensions/?demo=svg` uses the same
HTML, JavaScript and Wasm as this server. No Trunk, nightly toolchain, worker pool, or shared Wasm
memory configuration is required. The app opts into wgpu's
`fragile-send-sync-non-atomic-wasm` compatibility feature because GPUI paint
producers require `Send + Sync`; GPU handles stay on the browser thread. A
compile-time guard rejects atomic Wasm builds. The build script clears global
host Rust flags so native linker options cannot leak into the Wasm link. Serve on localhost (or HTTPS) for WebGPU and
clipboard access. The supplied server also sends cross-origin isolation headers.

## Local checks

- Shader: run both forced backends. All eleven cards should show the same paint
  in a rounded background, border and diamond path.
  Animate mesh/WGSL, pause, change group opacity, scroll, and resize. Check rounded
  edges, transparent stops, path coverage, scene order, and console errors.
- Parley: run both forced backends. Cycle the three samples; switch narrow/wide
  and resize. Verify Arabic shaping and Hebrew/Latin visual order. Click/drag,
  use arrows and Shift, type, delete grapheme clusters, copy/paste, and try IME
  composition/commit/cancel with the candidate window near the caret.
- The Parley view and input handler are shared directly with `apps/text-lab`.
  Timings use a browser clock; reported seed timings measure CPU layout only.
  Bundled IBM Plex Sans and Noto fonts need no system-font access or download.
  Their licenses remain beside the source font assets.

The demos exercise SDR output. `xgpui-gamut` currently has Metal and native D3D11
presentation adapters; a browser wide-gamut presentation path is separate work.
Compile/link checks do not establish browser GPU, clipboard, or IME correctness;
the checklist above is intended for local interactive acceptance.

## SVG demo

Open `?demo=svg` (or force `&backend=webgpu` / `&backend=webgl`). Select one of
five samples, wheel to zoom around the pointer, drag to pan, or use preset zooms.
Toggle the backdrop, mount ten gallery tiles, and switch the shared 8/32 MiB
pool. Two side-by-side views share wheel zoom, preset zooms and drag pan:

- **Bounded:** the reusable progressive element, with a 2 MiB output cap and
  optional shared pool. Deep zoom eventually enlarges a limited-resolution image.
- **Rerasterized:** renders the visible rectangle at device-pixel resolution on
  camera or viewport changes. Output dimensions depend on the viewport, rather
  than zoom, so curves stay sharp through 1600%. The parsed document is retained;
  unchanged redraws reuse the raster. Replaced atlas images are released.

The ten-tile gallery still uses bounded elements and the shared scheduler.
The comparison raster is outside that pool and reports its own size and latency.
Check clipping, gradients, shadows and curve detail as the bounded preview settles.

Diagnostics expose raster latency, phase and accounted pool reservations, not
total browser/GPU memory. Each bounded element caps output at 2 MiB, with one admitted
job in the pool. This non-atomic Wasm platform runs raster work on the browser
thread; complex filters can still block interaction. Worker-based rendering is
a follow-up. Native latency/memory benchmarks do not establish browser results.

The artwork is embedded unchanged from `apps/svg-lab/samples`. The build copies
the original SVGs, notices and `svg-attribution.html` beside the app, preserving
AJ Ashton, rg1024 and Ebaychatter0 credits and each artwork's separate license.
Dragon remains CC BY-SA 3.0; original lab artwork is Apache-2.0.

Validated on 2026-10-24 in the in-app Chromium browser on Apple M4 Max:
automatic, forced WebGPU and forced WebGL2 rendered; wheel zoom, drag pan,
reset, sample selection, gallery mounting and scrolling worked without console
errors. The 8 MiB pool with 11 mounted elements displayed a 7.98 MiB accounted
peak. This is an interactive smoke check, not a browser memory benchmark.

The viewport comparison uses `SvgSize::Viewport` in the GPUI renderer. Each output
edge must be 1..=8192 device pixels; oversized outputs fail visibly instead of
silently reducing detail. Visible-region rendering does not bound all resvg
filter intermediates. A 1600% leaf shadow render took about 2.5 seconds in one
local browser check; synchronous rasterization can pause input. This is a
quality comparison, not a worker-based responsiveness improvement.

Viewport comparison validated locally on 2026-10-24: WebGPU and WebGL2, synchronized
wheel zoom and drag pan, and sharp leaf detail through 1600%. Native tests compare
a viewport crop against a full raster (including curves, gradients, clipping and
shadow), verify fixed output dimensions at deep zoom, and reject invalid regions.

Use **Sharp SVG zoom: ON/OFF** to compare CPU rerasterization with GPU texture
zoom. ON preserves crisp detail by rebuilding visible pixels on camera changes.
OFF renders a base raster at 100% once, then applies synchronized zoom/pan while
keeping the right raster count unchanged. Selecting artwork or resizing still
rebuilds the base raster. The left bounded element/gallery retain progressive
refinement, so this switch does not claim to disable all CPU work in the app.
The notice and right-hand raster counter show the active mode and its cost.

**Bounded view: ON/OFF** and **Comparison view: ON/OFF** independently unmount
each panel's renderer while preserving panel dimensions and the shared camera.
An off panel performs no new raster work; switching off the comparison also
releases its cached raster. Re-enabling a panel renders the current camera.
Unmount the separate gallery as well when comparing a single renderer's cost.
