# Media lab

Native visual test app for `xgpui-media`. Generated offline samples compare the
standard image atlas with dedicated mipmapped textures at matching zoom and pan.

From the repository root (launch/build/capture recipes default to **release**):

```sh
just --justfile apps/media-lab/justfile ui
just --justfile apps/media-lab/justfile ui --image /absolute/path/photo.png
just --justfile apps/media-lab/justfile capture
just --justfile apps/media-lab/justfile wgpu-capture
```

Without `just`, run:

```sh
cargo run --release --offline --locked --config .cargo/validation.toml --manifest-path apps/media-lab/Cargo.toml --target-dir target
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
just --justfile apps/media-lab/justfile paths
just --justfile apps/media-lab/justfile paths-capture
just --justfile apps/media-lab/justfile paths-wgpu-capture
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
just --justfile apps/media-lab/justfile video
just --justfile apps/media-lab/justfile video-capture
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
RELEASE/DEBUG. Use `just --justfile apps/media-lab/justfile profile=dev ui` for an
explicit development override. The synthetic video source needs no sample file.

## Windows GPU surface checks

On a Windows SDK host, `just --justfile apps/media-lab/justfile dxgi-test` runs the
native BGRA and keyed NT-handle snapshot regressions in release mode. Separate
WARP devices test busy acquisition, handle lifetime, retained ownership, compositor
pixels and producer-key handoff. This requires no video file or decoder. The
interactive synthetic video source remains macOS-only. Windows test code is
cross-checked from macOS; native execution is still pending.

## Linux DMA-BUF layout checks

`just --justfile apps/media-lab/justfile dma-buf-layout-test` runs release layout
contract tests on the current host, without a sample file. The initial opt-in
Linux Vulkan adapter imports dedicated immutable single-plane linear BGRA buffers
whose producer fence and EXTERNAL/GENERAL release have already completed. It
records queue ownership acquisition and caches the native source texture. Explicit
fences, mutable pools, tiled/YUV/foreign-family buffers and a Linux synthetic source
remain pending. The implementation is cross-checked for Linux; layout tests on
macOS do not establish Linux GPU import or pixel correctness.

On a compatible Linux Vulkan host, run:

```sh
just --justfile apps/media-lab/justfile dma-buf-native-test
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
just --justfile apps/media-lab/justfile upload-bench
just --justfile apps/media-lab/justfile upload-wgpu-bench
```

This generates six 2048² noisy PNGs with marker colors, warms the pipeline, then
measures three upload waves and 120 cached frames for the same twelve-placement
scene. Two upload workers share 64 MiB staging, with one source / 32 MiB per-frame
admission. All sources, duplicate placements, upload counts and pool accounting
are checked. The JSON contains raw CPU samples, nearest-rank summaries, sampled
resource peaks and pool allocations/reuses. The matching PNG verifies final pixels.
Native outputs are `/tmp/media-upload-bench.json` and `.png`; portable outputs use
`media-upload-bench-wgpu`. No personal image/video is needed.

Decode/mip wall time is reported separately from timed rendering. Generation,
warmup, pacing sleep and final readback are excluded. CPU `render_scene` and
source-preparation time are not GPU time, whole-window frame time or proof of
zero stutter. Background load is uncontrolled. See the media guide for full
interpretation and versioned macOS observations; do not use those as thresholds.

## Visible-window cadence run

```sh
just --justfile apps/media-lab/justfile ui-perf
```

Keep the window visible and foreground without changing controls until
`/tmp/media-ui-perf.json` is written (normally about seven seconds). This release
run warms the primary image, animates zoom for 180 baseline intervals, automatically
loads 12 fresh frequency-field snapshots, then records 180 cached intervals. The
window remains available for manual testing afterward. JSON retains every measured
render-start interval, including gaps over 250 ms, and records p50/p95/max and counts
over 25/50 ms. Burst-trigger work is included; only the warmup boundary is skipped.
A 60-second timeout or source failures are recorded in `errors`; check that it is
empty and `completed_sources` is 12. Minimized/occluded windows invalidate a cadence
comparison. Reduced motion suppresses the sweep; frame requests still run.

These are observed UI render-start intervals, not GPU durations, presentation
feedback or measured input latency. The baseline has no burst previews; the cached
phase retains the final previews, so they are not identical draw workloads. The
burst includes decode/mips and admission waits as well as staging/submission.
See the media web guide for three versioned macOS observations and review status.

## Sustained scrolling with large-image replacements (60 Hz baseline)

```sh
just --justfile apps/media-lab/justfile scroll-perf
```

Select **60 Hertz** for the test display in System Settings → Displays first.
Keep the window foreground and avoid interacting during measurement. The recipe
uses release mode and generates its own four 3072×2048 PNGs before timing.
No sample files are needed. After warming all sources, a native scrolling feed
with 24 cards moves at 1200 points/second, reversing at its ends. It measures five
seconds of cached baseline, 30 seconds of sustained file decode/mip/upload
replacement, drains pending work, then measures five seconds of cached scrolling.
The same four source slots and card geometry are used throughout. Each full mip
chain is 33,554,428 bytes, just under the 32 MiB per-scene limit. At most one new
source is adopted per scene. A single replacement job starts at most every 350 ms;
the previous source remains visible until the replacement is host-submitted.

The JSON at `/tmp/media-scroll-60hz.json` retains every measured render-start
interval and reports p50/p95/max, gaps above 33.333/50 ms, completed replacements,
errors, observed scroll travel and sampled upload reservations. A provisional
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
