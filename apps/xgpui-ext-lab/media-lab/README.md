# Media lab

Native visual test app for `xgpui-media`. Generated offline samples compare the
standard image atlas with dedicated mipmapped textures at matching zoom and pan.

From the repository root (launch/build/capture recipes default to **release**):

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile ui
just --justfile apps/xgpui-ext-lab/media-lab/justfile ui --image /absolute/path/photo.png
just --justfile apps/xgpui-ext-lab/media-lab/justfile capture
just --justfile apps/xgpui-ext-lab/media-lab/justfile wgpu-capture
```

Without `just`, run:

```sh
cargo run --release --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin media-lab --target-dir target
```

Scroll to zoom, drag either viewport to pan, or use the zoom buttons. Reset view
restores centered 100% fit. Sweep zoom animates minification unless reduced motion
is enabled. Opacity and corner controls exercise alpha and clipping. Sample buttons
switch between frequency detail, translucent edges, color terrain and an optional
user PNG/JPEG/WebP. ICC profiles and EXIF orientation are not applied.

## Visual checks

1. Compare fine patterns while zooming out: mipmapped detail should settle instead
   of shimmering. Linear-light filtering can look brighter than atlas minification.
2. Select Alpha edges and vary opacity/corners. Look for colored fringes, clipping
   leaks or unexpected dark borders; pan beyond the viewport to exercise clipping.
3. Click Burst ×12. Waiting jobs should progress under bounded admission; completed
   snapshots retire to free source reservations. Repeat to inspect staging reuse.
4. Change count/byte presets, reload and repeat. Inspect upload counts and status;
   retained snapshots should upload once rather than once per redraw.

READY means host submission, not GPU completion. Diagnostics exclude atlas memory;
redraw interval p95 is not GPU timing. Headless capture checks upload completion at
the host, enforces one upload per image, and fails on errors or timeout. It writes
an unlabeled contact sheet: atlas left, media right; frequency, alpha and terrain
rows, followed by your optional image. Native capture is `/tmp/media-lab.png`;
wgpu capture is `/tmp/media-lab-wgpu.png`. The wgpu feature selects only the headless
backend; interactive macOS windows use Metal. Windows/Linux runtime QA remains
outstanding.

## Color filter checks

The five media-filter buttons cycle brightness, contrast, saturation (including
grayscale), hue and tint presets on the right panel only. Reset filters restores
identity. Check transparent edges while combining filters with opacity/corners;
source alpha should remain unchanged and the upload count should stay at one.
The filters compose in displayed order in encoded sRGB, with final SDR clipping.
Append `--vivid` to UI or headless commands for a combined adjustment capture.

## Path effects

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile paths
just --justfile apps/xgpui-ext-lab/media-lab/justfile paths-capture
just --justfile apps/xgpui-ext-lab/media-lab/justfile paths-wgpu-capture
```

Use **Path effects →** from the Images view or start with `--paths`. Three columns
compare original, trimmed and trimmed/dashed strokes; three rows cover open curves,
a closed heart, and multiple contours. Cycle trim to zero and full length, change
dash lengths/phase (including an odd array), vary width, and animate. Look for
continuous Bézier shapes and joins at closed seams. Animation respects reduced
motion. Measurement is cached; the effect is applied before native tessellation.
Trim uses total length across contours; dash phase restarts at each retained
contour. The headless PNG has the same layout at 68% trim, with pixel checks for
visible and decreasing coverage. See the SVG web guide for bounds and semantics.

## Native video presentation (macOS Metal)

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile video
just --justfile apps/xgpui-ext-lab/media-lab/justfile video-capture
```

The Video surfaces button opens a generated ~30 fps IOSurface source. Format
cycles BGRA8 and NV12 full/video range using BT.601/BT.709. Both
viewports share one snapshot per render. Pause holds the displayed frame while
production continues; Burst 120 replaces pending frames, Resume picks the latest,
and Stop retains the final frame. Restart creates a fresh source. Opacity and
corners exercise styled presentation. Leaving the view stops the source thread.
Paused statistics update on UI interactions; observed snapshots are not GPU fences.

The source writes CPU pixels into new IOSurfaces. Presentation imports a native
texture view without a staging upload or display-sized intermediate. This is a
presentation test, not a decoder or A/V playback clock. Native NV12 samples luma
and chroma in the compositor without an intermediate conversion target. Additional
YUV formats/backends, HDR and premultiplied-alpha frames remain pending. Headless
capture validates all five modes for RGB values, corners, clipping, opacity and
retained import reuse, saving the requested BGRA PNG and four NV12 sibling PNGs.
No sample file is required.

All launch/build/capture recipes default to release. The window title identifies
RELEASE/DEBUG. Use `just --justfile apps/xgpui-ext-lab/media-lab/justfile profile=dev ui` for an
explicit development override. The synthetic video source needs no sample file.

## Windows GPU surface checks

On a Windows SDK host, `just --justfile apps/xgpui-ext-lab/media-lab/justfile dxgi-test` runs the
native BGRA and keyed NT-handle snapshot regressions in release mode. Separate
WARP devices test busy acquisition, handle lifetime, retained ownership, compositor
pixels and producer-key handoff. This requires no video file or decoder. The
interactive synthetic video source remains macOS-only. Windows test code is
cross-checked from macOS; native execution is still pending.

## Linux DMA-BUF layout checks

`just --justfile apps/xgpui-ext-lab/media-lab/justfile dma-buf-layout-test` runs release layout
contract tests on the current host, without a sample file. The initial opt-in
Linux Vulkan adapter imports dedicated immutable single-plane linear BGRA buffers
whose producer fence and EXTERNAL/GENERAL release have already completed. It
records queue ownership acquisition and caches the native source texture. Explicit
fences, mutable pools, tiled/YUV/foreign-family buffers and a Linux synthetic source
remain pending. The implementation is cross-checked for Linux; layout tests on
macOS do not establish Linux GPU import or pixel correctness.

On a compatible Linux Vulkan host, run:

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile dma-buf-native-test
```

This generates and exports its own four-quadrant GPU surface,
imports the DMA-BUF and validates rendered pixels, clipping/opacity, cache reuse
and teardown. It runs in release mode with Khronos validation enabled and saves
`media-lab-dma-buf.png` in the system temporary directory. It needs linear BGRA
DMA-BUF export/import capability and `VK_LAYER_KHRONOS_validation`; unsupported
hardware/layers cause an explicit failure. No sample video or decoder is needed.
The native harness is cross-checked for Linux but has not been executed on this
macOS development host. See the web guide for exact coverage and remaining limits.

## Release upload CPU benchmark

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile upload-bench
just --justfile apps/xgpui-ext-lab/media-lab/justfile upload-wgpu-bench
```

This generates six 1024² noisy PNGs, warms the pipeline, then measures three
portable upload waves using fresh renderers and 120 cached effect frames. Source
decode/mips happen before timing. Schema 2 reports CPU snapshot/render samples and
asynchronous readiness wall time, including polling waits. Final PNG pixels are
checked outside timed samples. Driver/device pools may survive fresh renderers.
There are no producer upload counts, staging-pool metrics or GPU timestamps.
The smaller corpus fits the portable executor working-set budget. Schema 1
reports are not directly comparable. Native outputs are
`/tmp/media-upload-bench.json` and `.png`; wgpu outputs use `media-upload-bench-wgpu`.

## Visible-window cadence run

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile ui-perf
```

Keep the window visible and foreground without changing controls until
`/tmp/media-ui-perf.json` is written (normally about seven seconds). This release
run warms the primary image, animates zoom for 180 baseline intervals, automatically
loads 12 fresh frequency-field snapshots, then records 180 cached intervals. The
window remains available for manual testing afterward. JSON retains every measured
render-start interval, including gaps over 250 ms, and records p50/p95/max and counts
over 25/50 ms. Burst-trigger work is included; only the warmup boundary is skipped.
A 60-second timeout or source failures are recorded in `errors`; check that it is
empty and `decoded_sources` is 12. Minimized/occluded windows invalidate a cadence
comparison. Reduced motion suppresses the sweep; frame requests still run.

These are observed UI render-start intervals, not GPU durations, presentation
feedback or measured input latency. The baseline has no burst previews; the cached
phase retains the final previews, so they are not identical draw workloads. The
burst includes decode/mips and admission waits as well as staging/submission.
See the media web guide for three versioned macOS observations and review status.

## Sustained scrolling with large-image replacements (60 Hz baseline)

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile scroll-perf
```

Select **60 Hertz** for the test display in System Settings → Displays first.
Keep the window foreground and avoid interacting during measurement. The recipe
uses release mode and generates its own four 3072×2048 PNGs before timing.
No sample files are needed. After warming all sources, a native scrolling feed
with 24 cards moves at 1200 points/second, reversing at its ends. It measures five
seconds of cached baseline, 30 seconds of sustained file decode/mip/upload
replacement, allows a one-second settling interval, then measures five seconds of cached scrolling.
The same four source slots and card geometry are used throughout. A single replacement job starts at most every 350 ms; stable presentation slots
retain previous pixels until renderer completion. Identical reloaded pixels can
reuse structural GPU identity; this does not force a fresh upload every time.

The JSON at `/tmp/media-scroll-60hz.json` retains every measured render-start
interval and reports p50/p95/max, gaps above 33.333/50 ms, decoded replacements,
errors and observed scroll travel. Decode completion is not GPU readiness. A provisional
60 Hz check requires p95 ≤20 ms in all phases, loading p95 no more than 2 ms above
baseline, at most 1% of intervals above 33.333 ms, none above 50 ms, at least 30
replacements, over 1000 points of scroll travel, and no errors. These are explicit
project targets, not industry standards. Independently verify the display mode;
`target_hz: 60` labels the budget and does not cap or change the display.

The report is written once at completion; the window remains available for manual
wheel/trackpad checks. Start auto-scroll resumes animation after measurement.
Changing auto-scroll during measurement records an error. Reduced motion suppresses
automated scrolling and records an error rather than reporting a valid scroll run.
The 90-second timeout is checked on renders; an occluded window may pause updates.
Keep it visible. Restore your prior refresh-rate preference afterward.

This stresses warm-cache file loading, CPU decode/mips, source allocation and GPU
staging/submission. It does not measure cold-disk I/O, OS input delivery latency,
GPU timestamps, actual presented frames or lower-powered machines. Resource peaks
are sampled and may miss short worker activity; zero observed active workers does
not mean no worker ran. They are not total memory/VRAM bounds. See the web guide
for the fixed-60-Hz M4 Max observations and raw reports.

## Linear adjustments (issue #26 Phase 2)

Run `just media --adjustments` from the repository root. Both panels share one
source upload: original on the left, linear-sRGB matrix/vignette on the right.
Cycle EV exposure, contrast around 0.18, saturation and vignette, then animate,
reset, zoom/pan and check alpha/opacity/corners. The upload count stays at one.
The mode toggle returns to the encoded-sRGB filters/atlas comparison.
Use `--headless --png /tmp/media-adjustments.png` to capture generated samples;
add `--wgpu` when built with the app's `wgpu` feature to capture that backend.
RGBA16F/P3 uploads are available in Phase 3 below. Progressive/tiled images are available in Phase 4 below. Playback remains #40.


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

Watch decoded/visible pages, mip, payload reservations, eviction and cancellation.
Both timing values describe CPU availability. Renderer-owned portable uploads and
atomic groups keep the previous complete preview/refinement visible until GPU
completion. Browser generation is one small page per advance on the host thread.
These are functional diagnostics, not disk/decoder or frame-time benchmarks.
Pan/zoom rapidly, toggle the 2 MiB cache, replace the source and cancel/restart:
look for no internal seams, doubled translucent colors or holes during refinement.
Try deep zoom and the blur controls; blur should cross page boundaries smoothly.

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


Filters and adjustments now use portable placement transforms and share the source
upload with ordinary views. Check rapid filter changes, exposure, saturation and
vignette for responsive edits without placeholder flashes; reset should restore
the baseline. Check opacity and rounded edges while editing. Use `--adjustments`
for linear controls and `--float-images` for native wide-color images.


### Portable CoreVideo BGRA/NV12 validation

Run `just media --video` to open the generated video GUI. Format cycles BGRA and
NV12 full/video range with BT.601/BT.709. After startup, look for consistent color
bars, a moving marker, responsive exposure/saturation/vignette controls, and no
checkerboard flashes during frame replacement. Pause holds the visible frame;
Burst 120 replaces pending frames; Resume should adopt the latest. Check opacity,
rounded edges and blur in both placements. Their stable element IDs keep separate
presentation slots while sharing one imported/converted source.

`just media --headless --video --png /tmp/portable-video.png` runs pixel checks and
writes five captures without opening a window. `just media-import-test` runs nine
release-mode GPU tests on the command line; look for `9 passed; 0 failed` and no
warnings. Prefix either command with `MTL_DEBUG_LAYER=1` for Metal API validation.
Fixtures are generated locally; no decoder or video file is needed.

Tests check BGRA direct import, all four NV12 matrix/range combinations, converter
reuse, source-sized half-float output, placement adjustments, replay, pending
acquisition, cancellation, descriptor rejection, submitted-prefix cleanup and
plane ownership through paused dependent work. Source-sized NV12 conversion adds
one GPU pass; direct BGRA imports allocate no new image storage. Neither path
reads producer pixels back to the CPU. Headless validation reads final targets.

CoreVideo and the macOS video lab now use portable draws. The old CoreVideo
producer/cache and compositor-side NV12 paths are removed. DXGI/DMA-BUF executor
integration remains next; Windows runtime testing is still outstanding.
