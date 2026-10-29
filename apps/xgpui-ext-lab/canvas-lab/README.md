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

The lower-right **current canvas view** is a live scaled copy of the visible
viewport. It follows pan, zoom, resize, selection, guides, and scene edits, using
an actual rendered capture with the toolbar and preview panel excluded. **M**
hides/shows the panel. **T** switches between this display-only live preview and
the whole-canvas minimap. The minimap shows card geometry without a grid, with
an amber viewport outline; click or drag it to recenter while keeping zoom.
The whole-canvas mode uses a cached thumbnail and does not perform GPU readback.

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

The unpublished `xgpui-canvas` crate holds dependency-free f64 camera math; its
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

## CPU Mandelbrot image example

Press **F** to switch between cards and Mandelbrot, or start directly with:

```sh
just canvas-lab --mandelbrot
```

Drag with the left button or scroll to pan; pinch zooms at the pointer. **R**
resets the camera. **M** toggles the current-view preview. The whole-scene card
minimap is unavailable in this mode; switching content resets the camera.

The fractal uses f64 CPU escape-time calculations on a background executor and
uploads an ordinary BGRA image through GPUI's image painter. No custom fractal
shader or new canvas API is involved. A 240-pixel-wide render is followed by a
960-pixel-wide refinement after a 180 ms pause, with height capped at 960 pixels.
Both use 256 iterations. The previous image is projected at its original world
bounds while replacement work runs. Camera/size changes cancel obsolete work
at row boundaries; generation checks reject stale completions. This is a bounded
resolution demonstration, not a deep-zoom renderer or a CPU/GPU benchmark.
The existing camera retains its 0.01–64 zoom limits. The optional live preview
still carries its separate GPU readback cost.

Capture a refined native frame and exit:

```sh
just canvas-lab --mandelbrot --mandelbrot-capture /tmp/mandelbrot.png
```
