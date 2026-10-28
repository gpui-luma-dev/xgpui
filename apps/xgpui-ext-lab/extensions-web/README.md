# All demos index and browser extension examples

A standalone Wasm app for `xgpui-shader`, `xgpui-parley`, `xgpui-svg`, `xgpui-media`, and `xgpui-color`. All use the existing
GPUI web platform and wgpu renderer. The gallery offers automatic selection
(WebGPU with WebGL2 fallback), forced WebGPU, and forced WebGL2. A forced backend
fails explicitly if unavailable; check the browser console for adapter details.

## Extension demos

| Crate | URL query | What it demonstrates |
| --- | --- | --- |
| `xgpui-color` | `?demo=color` | Display P3 endpoints, encoded sRGB / linear RGB / OkLab / OkLCh interpolation, gamut mapping and alpha. Runtime diagnostics distinguish the sRGB canvas from P3 display capability. |
| `xgpui-shader` | `?demo=typography` | Gradient/glow/drop-shadow styling plus typewriter, wave and scramble effects; source inspection and reduced-motion preview. |
| `xgpui-shader` | `?demo=shader` | Existing gradients, meshes and portable WGSL paints. |
| `xgpui-shader` | `?demo=brushes` | Universal solid/linear/radial fills and borders, 16-point freeform, regular and tangent-controlled Hermite mesh, Simplex/Worley/dither modulation, glyph masks and direct/native paths. |
| `xgpui-shader` | `?demo=layers` | Subtree opacity, grayscale/saturation/brightness/contrast/inversion, adaptive/full-resolution blur, seven destination blends and backdrop blur. |
| `xgpui-parley` | `?demo=parley` | Paragraph shaping, bidirectional text and editing. |
| `xgpui-svg` | `?demo=svg` | Progressive SVG rendering and viewport rasterization. |

The Color / Gamut demo uses the standalone `xgpui-color` math crate directly.
Its browser output is mapped to sRGB. For actual native Display P3 comparison,
see the [color-lab desktop companion](../gamut-lab/README.md#native-color-comparison).

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
bash apps/xgpui-ext-lab/extensions-web/build.sh
python3 apps/xgpui-ext-lab/extensions-web/serve.py
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

- Color / Gamut: open `?demo=color` with each forced backend. Cycle endpoint
  presets, mapping policy and ending alpha. Check all four interpolation rows
  and the amber out-of-sRGB indicators. Confirm the runtime panel reports the
  canvas color space separately from browser-reported P3 display support.

- Shader: run both forced backends. All eleven cards should show the same paint
  in a rounded background, border and diamond path.
  Animate mesh/WGSL, pause, change group opacity, scroll, and resize. Check rounded
  edges, transparent stops, path coverage, scene order, and console errors.
- Parley: run both forced backends. Cycle the three samples; switch narrow/wide
  and resize. Verify Arabic shaping and Hebrew/Latin visual order. Click/drag,
  use arrows and Shift, type, delete grapheme clusters, copy/paste, and try IME
  composition/commit/cancel with the candidate window near the caret.
- The Parley view and input handler are shared directly with `apps/xgpui-ext-lab/text-lab`.
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

The artwork is embedded unchanged from `apps/xgpui-ext-lab/svg-lab/samples`. The build copies
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


## Color / Gamut demo

Open `?demo=color` (or force `&backend=webgpu` / `&backend=webgl`). Four rows compare
encoded sRGB, linear RGB, OkLab and shorter-hue OkLCh interpolation from the same
Display P3 endpoints. Cycle three endpoint presets, toggle clipping/chroma
reduction, and change ending alpha. Amber strips mark out-of-sRGB samples before
mapping. The app uses `xgpui-color` directly; it does not depend on native gamut
producers. Output is a mapped sRGB preview, with 32 CPU samples per row displayed
through ordinary shader paints. This does not establish physical P3 presentation.


Run `bash apps/xgpui-ext-lab/extensions-web/check-color.sh` for the CPU preview-sampler tests.
The script compiles the dependency-free color core and sampler directly with
rustc, avoiding the browser platform dependencies for native test execution.

The runtime panel distinguishes P3 source endpoints, sRGB preview mapping, actual
canvas color space (or unknown when unavailable), and browser-reported P3 display
capability. For native P3 versus sRGB comparison, run the
[color-lab desktop companion](../gamut-lab/README.md#native-color-comparison).

## Universal brush and layer acceptance

Open `?demo=brushes&backend=webgpu` and `?demo=layers&backend=webgpu`;
repeat with `backend=webgl`. The in-app tabs switch views without reloading.

- Brushes: toggle the background, cycle all seven brush-local noise blends,
  toggle Hermite tangent bend, and switch direct/native polygon coverage. The
  polygon hole must stay open; the curved reference uses native coverage.
  Gradient typography uses monochrome glyph atlas coverage.
- Layers: cycle blur 0/4/8/16, Adaptive/Full resolution, each color filter and
  destination blend. Text and shapes blur together; the output stays clipped.
  Remove/restore the effect and resize/scroll to check stale pixels and cleanup.
- Backdrop: swap stripe colors and toggle blur. The foreground label stays sharp.
  This boundary is separate from the captured subtree; nested backdrops remain
  unsupported. Both examples stay within the four-group/four-backdrop limits.

These use the real public brush and layer APIs, not HTML/CSS approximations.
The pastel waves performance experiment remains a native-only follow-up (#35).

Browser presentation surfaces without `COPY_SRC` use one reusable full-window
SDR effect target and a final unblended presentation draw. The extra texture
costs four bytes per device pixel, resizes with the window, and is released when
no layer/backdrop effects remain. Copy-capable surfaces keep direct rendering.

## Browser startup failures

Start with **Automatic**; a forced **WebGPU** link intentionally disables WebGL2
fallback. If initialization fails, use **Try Automatic** or **Try WebGL2** on the
error panel. Switching reloads the app and creates a fresh canvas.

The panel reports the requested backend, secure-context status, whether
`navigator.gpu` is exposed, the renderer's actual canvas-context result, and the
browser version. WebGPU requires a secure context (HTTPS or localhost) and
browser/adapter support. An exposed API alone does not guarantee canvas or
adapter creation succeeds. These diagnostics stay in the page/console; they are
not sent to a telemetry service.

HTTP wildcard bind-address URLs (`http://[::]:PORT/` and `http://0.0.0.0:PORT/`)
are corrected to `http://localhost:PORT/` before starting WASM. This preserves
`backend=webgpu`; the correction does not substitute WebGL2. Open localhost
rather than copying a wildcard listening address from a server's output.

## Unified All demos page

The index lists all 19 hosted WASM examples from this app and `shader-examples`,
grouped into Brushes & paints, Layer effects, Shaders & motion, Text, Media & paths, and SVG &
color. Native labs are a separate section with repository-root launch commands,
including pastel waves and the full paint-lab/Hermite editor. The shader gallery
remains a focused subset; its All demos link opens this complete index.

For cross-gallery navigation, use the documentation preview at
`http://localhost:8000/examples/extensions/` after building both browser apps.

### Live Hermite editor

Open `?demo=hermite` (Automatic), `?demo=hermite&backend=webgpu` or
`?demo=hermite&backend=webgl`. Select a mesh point, drag the position/tangent
sliders or use their −/+ controls, switch 2×2/3×3 grids and presets, or reset.
Legacy and direct previews share paint-lab's `hermite_editor.rs` editing model.
Rejected direct geometry displays its reason while legacy rendering stays current.
The same field is also shown on borders and text.

## Typography acceptance

Open `?demo=typography` with each backend. Drag and Shift-select; double-click words,
triple-click lines; use Home/End and Cmd/Ctrl+A/C. Check combining accents, joined
emoji and flags, selection persistence through size/theme changes, and the narrow
overflow sample. Paste copied text into another app. The current web platform has no canvas accessibility bridge. Inspect the read-only
text field and selection with the native demo; full screen reader certification
remains manual. See [scope and API](../../../docs/extensions/typography.md).


## Media and contour-effects acceptance

Open `?demo=media-filters`, `?demo=mipmaps` and `?demo=path-effects`; repeat with
`&backend=webgpu` and `&backend=webgl`. Build with `--release` for testing.

- Filters: cycle all five controls, including grayscale; reset to identity.
  Compare transparent edges, change opacity/corners and confirm dedicated source
  preparations remain one. Original and filtered placements share one texture.
- Mipmaps: compare the ordinary GPUI atlas with the dedicated 11-level texture.
  Cycle all zooms down to 12.5%, animate and pause, and vary opacity/corners.
  Fixed-height headers align the comparison even when labels wrap.
- Paths: compare original/trimmed/dashed open curves, closed heart and multiple
  contours; zero trim hides only effect columns, full trim restores the contour.
  Cycle even/odd dash arrays, phase and width; animate/pause.

These use real public media filter math, `mipmapped_rgba8`, the wgpu image-output
SPI/compositor and shared `xgpui-svg` contour effects. The generated 1024² RGBA
fixture and first upload run synchronously on the browser thread, once per source.
Native `MediaLoader` threading, async staging admission and GPU surface imports
are not exercised. No sample file, decoder or browser worker pool is needed.
See the media guide for details and native 60 Hz benchmark results.

All three demos were checked in release on 2026-10-26 in in-app Chromium, Apple
M4 Max, with forced WebGPU and WebGL2: controls, transparent edges, minification,
source reuse and path-effect endpoints/animation rendered without console errors.
WebGL2 emitted capability and resize-poll warnings while rendering successfully.
This is visual/functional acceptance, not a browser performance benchmark.

The Image filters demo now starts with linear-sRGB EV exposure, contrast,
saturation and vignette on a rectangular generated RGBA fixture. Switch to
encoded filters to compare the earlier API. Animate, opacity/corners and reset
reuse one texture preparation on both WebGPU and WebGL2. See the
[media pipeline guide](../../../docs/extensions/media-pipeline.md).


### RGBA16F supplied pixels (Phase 3)

Run `just media --float-images` from the repository root for the release native
comparison, or open `?demo=media-float&backend=webgpu` /
`?demo=media-float&backend=webgl` in the rebuilt extensions web app.
Generated P3 colors, a near-black gradient and alpha edges need no external asset
or decoder. Native output compares explicit sRGB preview with accepted wide-SDR;
both browser panels are sRGB previews. Exposure/saturation, vignette, opacity,
reset and mode/zoom controls reuse one source upload.

See [media pipeline guide](../../../docs/extensions/media-pipeline.md) for the
`load_rgba16f` API, input metadata, byte limits and backend validation status.
Windows/Linux native runtime QA remains pending; 60 Hz stays the baseline.


### Progressive tiled canvas (Phase 4)

`just media --tiled` opens the release native app. Rebuild the web app and open
`?demo=media-tiles&backend=webgpu` or `?demo=media-tiles&backend=webgl`.
The shared view displays a generated 12,000 × 6,000 source without allocating it
in full. Fit/1:1, drag, wheel zoom and arrow controls change visible pages; source
replacement, Cancel/Restart and a 2 MiB cache mode exercise the scheduler.

Watch ready/visible tiles, mip, CPU/GPU reservations, upload staging, native
staging-pool allocations, eviction and cancellation. Preview timing is CPU
availability; view timing ends at reusable upload submission. Browser generation
is one small page per frame on the host thread. These are functional diagnostics,
not disk/decoder or sustained frame-time benchmarks. Keep 60 Hz as the baseline.

The [pipeline guide](../../../docs/extensions/media-pipeline.md) documents
`TileProvider`, `TileCanvas`, code examples, budget scope and pending native
Windows/Linux QA. No new decoder, blur pass or playback engine is added.

### Bounded media blur (issue #26 Phase 5)

The tiled demo now includes 0/4/8/16 px Gaussian blur and adaptive/full-resolution
quality. It blurs assembled pages once, avoiding internal tile seams. Nonzero
blur uses a smaller preview to fit the existing 1,024-device-pixel capture limit,
including halo. Filtering is premultiplied encoded sRGB; capture/filter textures
are additional to page budgets. Media uploads/imports and target allocations are
reused, but custom media captures refresh each paint. Native `just media --video`
also offers exposure, saturation, vignette and blur on generated BGRA/NV12
surfaces. Native imports are not available in Wasm; test portable blur via
`?demo=media-tiles&backend=webgpu` and `?demo=media-tiles&backend=webgl`.
