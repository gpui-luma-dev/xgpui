# Canvas integration prototype

Run `just canvas-lab` from the repository root. Two-finger scrolling pans; pinch
zooms around the pointer. Space + left-button drag or middle-button drag also
pans. Left-click a card to select it. Press **G** for line/dot grids, **R** to
reset the camera, **O** for a 1% overview, and **B** to switch between 10,000 and
100,000 cards. The header
shows the visible count, zoom, and selection. Leaving the window clears held
Space/drag state.

**Shift + left-drag** replaces the selection with all rectangles intersecting the
marquee, in either drag direction. Drag a selected card to move the entire group;
dragging an unselected card selects and moves that card. Empty clicks clear the
selection. Pink guides show nearby edge/center alignments, with a six-logical-pixel
snap tolerance and 200-pixel neighbour search radius at every zoom. Hold **Option**
for free movement. **Escape** cancels a move or restores the prior marquee
selection. Losing focus or changing the camera also cancels an active gesture.
Mouse release commits movement, including release outside the viewport.

Moves update BVH leaf/ancestor bounds rather than rebuilding the scene. Snapping
uses the selection's group bounds and excludes selected items. Equal-spacing,
angle snapping, auto-scroll, undo history, and accelerated bulk refits are not
included in this slice.

The lower-right panel defaults to the **whole-canvas minimap** for cards. Press
**T** to opt into the **current canvas view**, a live scaled copy of the visible
viewport. It follows pan, zoom, resize, selection, guides, and scene edits, using
an actual rendered capture with the toolbar and preview panel excluded. **M**
hides/shows the panel. **T** switches between this display-only live preview and
the whole-canvas minimap. The minimap shows card geometry without a grid, with
an amber viewport outline; click or drag it to recenter while keeping zoom.
The whole-canvas mode uses a cached thumbnail and does not perform GPU readback.
Its geometry refreshes once per render when scene identity/revision changes,
including active moves, cancellation and scene replacement. T/M/Escape cancel
active world/map drags; focus loss clears held modifiers and pointer ownership.

Captures are coalesced to at most one every 80 ms and resized to fit 432×276,
with aspect ratio preserved. The existing canvas grid appears only as part of
the captured view; there are no synthetic density tiles. Two unpresented draws
exclude the HUD from capture and restore normal hitboxes before input resumes.
This native prototype uses GPU readback, so capture has a performance cost.
The local run in `bench/macos-arm64-live-preview.json` measured about 17.8 ms
mean frame intervals in normal views (roughly 50 ms p95 at capture boundaries),
and 46.2 ms in the dense overview. A future GPU-resident scaled capture can avoid
the synchronous readback cost; hide the panel with M to disable capture.

The scene contains 10,000 or 100,000 world-space rectangles. BVH AABB culling happens
before constructing GPUI children, so offscreen cards do not enter layout or
text shaping. Scene bounds and IDs are retained between frames. Full-detail
cards are ordinary positioned GPUI divs with an
explicitly projected size and font size; this is not a general subtree transform.
Below 48px on the longest side, cards become plain filled bounds; below 4px,
they become 1px center proxies. Proxies use one paint callback with native quads,
without per-card layout, borders or text shaping. Full-card borders remain one
logical pixel. The viewport clips drawing and input through
the normal GPUI container, and clicks are checked against inverse-projected
world bounds using the scene's topmost hit test. Empty clicks clear selection.

The `xgpui-canvas` crate holds dependency-free f64 camera math; its
optional `gpui` feature supplies the reusable `Canvas` container and grid painter.
Screen coordinates are local logical pixels; world origin starts at the viewport
center. Zoom is bounded to 0.01–64. Projection converts to f32 only at the GPUI
boundary.

The grid uses one procedural shader quad, with fading minor/major lines or
dots. This first experiment excludes rotation, custom inertia, incremental scene insertion/removal,
and web integration. It does not establish arbitrary subtree scaling. Dense
overviews still record one native quad per proxy; spatial indexing does not
reduce that paint cost. Aggregated proxies and cached per-entity LOD thumbnails remain follow-ups.

Camera checks:

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-canvas/Cargo.toml --features gpui --target-dir target
```

Native checks and profiling (both quit after completion):

```sh
just canvas-lab --smoke /tmp/canvas-smoke
just canvas-lab --benchmark /tmp/canvas-profile.json
```

Smoke captures show default zoom, minimum/maximum zoom, a distant world origin,
and a resized viewport. Smoke mode also dispatches native GPUI mouse events and
asserts selection for visible full cards, bounds proxies, and point proxies. It
also checks forward/reverse marquee selection, group movement, Option bypass,
alignment guides, and Escape restoration. Captures include marquee and guide
overlays. Preview captures exercise current-view rendering at multiple zooms
and after resize. Captures are for visual inspection, not pixel assertions. The benchmark sweeps 10k/100k scenes at
1x zoom and a 100k scene at 0.01x, with 30 warmup and 120 measured frames per case.
It saves JSON and line/dot/overview PNGs captured during warmup.
CPU timings cover culling and child construction only. Frame intervals include
layout, rendering and OS scheduling; neither measurement is a GPU timing.

The October 29, 2026 local macOS arm64 release baseline is saved under
`bench/macos-arm64-baseline.json`. At a 1100×702 logical viewport and 2× display
scale, mean frame intervals were 16.66/16.67 ms for 10k/100k objects; mean CPU
cull/build times were 0.11/0.37 ms. Only 36–49 cards were visible. This is one
local scheduling measurement, not a performance guarantee for densely visible
scenes or other hardware/backends.

The retained-scene/LOD run is saved in `bench/macos-arm64-lod.json`. Normal views
averaged 16.66/16.74 ms between frames for 10k/100k objects. At 0.01x zoom, the
100k scene had 27,400–32,300 visible point proxies, zero GPUI child elements,
and a 24.51 ms mean interval. CPU query/build time averaged 0.11 ms in that
overview; most remaining cost is outside this measurement. The viewport is
1100×686 after the expanded status header, so this is not an exact before/after
comparison with the original baseline.

The BVH CPU comparison is recorded in `bench/spatial-query-baseline.json`.
For sparse rectangle queries averaging 35 matches at 100k objects, the local
indexed mean was 1.25 µs versus 66.30 µs for a linear scan. Building the immutable
100k index took 29.35 ms. At one million objects it took 347.51 ms to build and
4.35 µs per sparse query. These exclude GPUI layout and drawing; dense/overlapping
queries can still visit the whole scene. Run the crate's `spatial_queries`
example to reproduce the comparison and verify results against the linear scan.

The native BVH run is in `bench/macos-arm64-bvh.json`. At 100k objects/1x zoom,
mean cull/build time was 0.09 ms (previously 0.31 ms), while frame intervals
remained around 16.7 ms. The dense overview stayed around 24.5 ms/frame, with
higher query/build cost (0.35 ms versus 0.11 ms) from traversing and restoring
paint order for thousands of matches. These are separate local runs, not a
controlled hardware benchmark.

RFC coverage, API boundaries, limitations, and the next implementation plan are
recorded in [`docs/extensions/canvas.md`](../../../docs/extensions/canvas.md).

## Coherent-frame Mandelbrot explorer

Press **F** to switch between cards and Mandelbrot, or start directly with:

```sh
just canvas-lab --mandelbrot
MTL_HUD_ENABLED=1 just canvas-lab --mandelbrot
```

Left drag pans; two-finger scrolling or the mouse wheel zooms around the pointer.
Pinch is disabled in Mandelbrot mode. **Reset view (R)** restores the startup
position and 100% zoom. **M** enables the optional readback-based live preview;
it starts hidden to avoid its separate performance cost. Card controls are unchanged.

The native explorer displays one complete, immutable viewport image. Camera
changes immediately reproject that image, including an overscan guard band (1.5× viewport span for GPU frames; 3× for CPU
previews and 1.25× for CPU detail).
Its successor prepares offscreen with zero sampling opacity, then replaces the
front image only when ready. No world tiles, tile boundaries or partial detail
passes are presented. Large pans/zoom jumps can exhaust the guard band; newly
exposed areas retain the background until a complete replacement is ready.

GPU navigation uses one steady source resolution, capped at 1,920 pixels on the
longest side, with the same sampling density during movement and at rest. It does
not switch between a blocky GPU preview and a sharp final pass.

Deep CPU navigation uses a 128-pixel preview only when a replacement is needed.
A sharper completed image is retained and reprojected while it still covers the
view and has better sampling density. After 140 ms without a camera change, a
960-pixel detail frame is requested and blended in over 180 ms in linear light.
Refinement keeps the same iteration budget and palette. These are source-resolution
caps including overscan, not a full physical-pixel-resolution guarantee.
Only one successor is prepared at a time; obsolete detail is cancelled/rejected.
A CPU preview may finish across input events to avoid starvation, but is accepted
only if it remains near the current view. There is no queue of intermediate camera
states. Deep CPU views can still look softer while moving; their calculation is
not a deep-zoom GPU renderer.

On native Metal with accepted wide SDR, shallow views (up to 64×, subject to a
coordinate precision check) use a cached compute pipeline writing linear P3
RGBA16F textures directly on the host GPU. No CPU pixel generation, image upload
or pixel readback is involved in that route. Only the host submits commands;
preparation never waits for GPU completion. The front frame changes after a
nonblocking completion check. The header reports the current route and dimensions.

Deep views retain f64 CPU escape calculations on a background executor, using up
to four disjoint row workers and publishing only after all rows finish. Native
Metal uploads the resulting tagged linear-P3 float frame directly on the host
queue, avoiding the media staging lifecycle for small interactive frames. Other
hosts retain the xgpui-media upload/preview route. Other native backends and
unaccepted wide-SDR hosts use this route with the explicit sRGB preview where
needed. Runtime validation of the redesigned path is currently macOS arm64 only.
GPU perturbation/deep-zoom GPU arithmetic is not implemented. Force the CPU route
for comparison with `just canvas-lab --mandelbrot --mandelbrot-cpu`.

The palette is authored in Display P3, with saturated bands outside sRGB.
Presentation is wide SDR, not HDR, and does not establish physical display
coverage or calibration. The 0.01–10 billion× range remains finite f64 exploration;
the iteration budget grows with zoom and caps at 2,048. Unresolved points at the
limit are colored as interior. Deep regions can take longer to refine even when
camera presentation remains responsive. The browser companion retains its
separate bounded CPU-slice implementation.

Capture a refined native frame or reproduce a specific view:

```sh
just canvas-lab --mandelbrot --mandelbrot-capture /tmp/mandelbrot.png
just canvas-lab --mandelbrot --mandelbrot-zoom 1e10 \
  --mandelbrot-center -73.0931661111453 39.547771261599 \
  --mandelbrot-capture /tmp/mandelbrot-deep.png
```

Run a bounded native camera sweep through zoom-in, zoom-out, maximum zoom and
reset, saving settled captures plus frame-callback timing JSON:

```sh
just canvas-lab --mandelbrot --mandelbrot-check /tmp/mandelbrot-check
```

The bounded sweep disables the normal inactive-window frame throttle so focus
changes do not contaminate timings. Ordinary interactive launches retain that
throttle. The report measures local render-callback intervals during movement, not confirmed
displayed FPS or isolated GPU time. It does not assert every fractal location
fits a particular frame budget. Native Metal tests compare computed pixels with
the f64 CPU reference at stable interior/exterior points; boundary precision is
not implied by those samples.

The initial whole-frame checkpoint (before steady-resolution navigation) is recorded in
`bench/macos-arm64-coherent-frames.json`: 510 moving callback intervals averaged
16.5 ms, with a 17.7 ms p95; the sampled maximum-zoom view settled in 583 ms.
The aggressive sweep recorded five frames with exposed guard-band edges and
three without a usable front during the abrupt minimum-to-deep-view jump.
These are local observations, not a universal 60 FPS or zero-gap guarantee.
That checkpoint passed the default release build, eight regression tests, card
input smoke and its bounded Mandelbrot sweep. The steady-resolution revision adds
sharp-frame retention and blend tests plus an ignored manual GPU timing probe.

The steady-resolution checkpoint is recorded in
`bench/macos-arm64-steady-navigation.json`: moving callbacks averaged 16.5 ms
(p95 18.5 ms), and the sampled maximum-zoom frame plus its blend completed in
767 ms. The rapid deep zoom-out still exposed guard-band edges in some frames;
this revision prioritizes stable sharpness rather than claiming every rapid
camera sweep has complete coverage. Ten regression tests passed. The manual
GPU timing probe sampled 0.9–1.8 ms at the steady GPU resolution for 1×/32×/64×
views; those samples do not establish a universal GPU budget.
