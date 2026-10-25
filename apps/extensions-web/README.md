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
reload after changes. No Trunk, nightly toolchain, worker pool, or shared Wasm
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
pool. The gallery shares zoom and the scheduler; pan applies to the large viewer.
Check clipping, gradients, shadows and curve detail as the preview settles.

Diagnostics expose raster latency, phase and accounted pool reservations, not
total browser/GPU memory. Each element caps output at 2 MiB, with one admitted
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
