# Extension browser examples

A standalone Wasm app for `xgpui-shader`, `xgpui-parley`, and `xgpui-svg`. All use the existing
GPUI web platform and wgpu renderer. The gallery offers automatic selection
(WebGPU with WebGL2 fallback), forced WebGPU, and forced WebGL2. A forced backend
fails explicitly if unavailable; check the browser console for adapter details.

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
HTML, JavaScript and Wasm as this server. No Trunk, nightly toolchain, threaded worker pool, or shared Wasm
memory configuration is required. Hybrid SVG zoom uses an ordinary module Web
Worker with its own non-atomic Wasm instance. The app opts into wgpu's
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
- **Comparison:** defaults to Hybrid: GPU-transform the last viewport raster
  while moving, wait 160 ms after the last wheel/pan event, then render sharp
  visible pixels in a module Web Worker. The sharp result replaces the preview
  only if its document, camera and viewport generation are still current.
  Sharp (UI thread) and Texture only modes remain available for comparison.

The ten-tile gallery still uses bounded elements and the shared scheduler.
The comparison raster is outside that pool and reports its own size and latency.
Check clipping, gradients, shadows and curve detail as the bounded preview settles.

Diagnostics expose raster latency, phase and accounted pool reservations, not
total browser/GPU memory. Each bounded element caps output at 2 MiB, with one admitted
job in the pool. The bounded left view and gallery still run raster work on the
browser thread. Hybrid moves the right view's parse/raster/conversion work to a
worker; pixel transfer and GPU upload still occur on the browser thread. Native
latency/memory benchmarks do not establish browser results.

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
local browser check in Sharp (UI thread) mode; that synchronous mode can pause
input. Hybrid performs the same sharp rendering in a worker. Its output is capped
at 32 MiB and 8192 pixels per edge, but filter intermediates remain outside that
cap. The worker duplicates the app's Wasm instance; this is not a total-memory bound.
The main thread retains a base texture plus the latest detail raster (up to two
outputs), outside the bounded scheduler's accounting.

Viewport comparison validated locally on 2026-10-24: WebGPU and WebGL2, synchronized
wheel zoom and drag pan, and sharp leaf detail through 1600%. Native tests compare
a viewport crop against a full raster (including curves, gradients, clipping and
shadow), verify fixed output dimensions at deep zoom, and reject invalid regions.

## Hybrid worker behavior

Use **Hybrid · worker** (default), **Sharp · UI thread**, and **Texture only** to
compare quality and interaction cost. Hybrid retains a full-artwork base texture
for gestures so zoom-out and pan do not expose holes in a cropped detail raster.
It first obtains that base from the worker, then refines the settled viewport. Texture
only renders a base raster at 100% once and performs no right-view camera
rerasterization. Source changes and resize can rebuild that base.

`svg-worker-client.mjs` owns one worker, one active render and at most one latest
queued request. Gesture changes discard queued work and invalidate in-flight
results. A running render can finish during movement, but cannot replace a newer
camera. If it is still busy when the next settled request arrives, the client
terminates/recreates the obsolete worker rather than waiting behind it. Normally
the worker/document stay warm. Switching modes or dropping the demo rejects
outstanding jobs. Worker errors/timeouts retain the preview and show an error;
there is no synchronous-render fallback. Change the camera or reselect Hybrid to
retry. The parsed document is cached inside the worker; transferable pixel buffers
carry the result back. GPU/window objects never enter the worker.

The worker imports the same Wasm module as the app; `main` exits before GPUI
platform initialization in a worker context. `rasterize_svg_region` rejects calls
on the browser thread. The build stages both worker modules beside the Wasm in
standalone and docs previews. Module workers need HTTP(S), but no shared-memory
cross-origin-isolation setup. The bounded left view/gallery can still contribute
UI-thread work, so this is not a guarantee about all frame or input latency.

Queue regression tests (no browser required):

```sh
node --test apps/extensions-web/test-svg-worker.mjs
```

Interactive acceptance: rapidly zoom/pan at 1600%, change direction while a
worker render is active, and verify the preview follows input immediately and
only the current camera becomes sharp after settling. Switch source, modes and
backdrops while rendering, resize, and repeat on WebGPU/WebGL2. The worker timing
includes parse on a source change and pixel conversion, but excludes transfer,
GPU upload and presentation. Existing native benchmark numbers are not worker
responsiveness measurements.

Validated locally on 2026-10-25 with forced WebGPU and WebGL2, including the
port-8000 docs preview: leaf/camera source changes, zoom/pan, texture-to-sharp
settling, cancellation and mode changes. During a 1600% worker render, backdrop
changes and page scrolling still responded. This is an interactive smoke check,
not an input-latency benchmark or a claim that workers reduce raster CPU time.
