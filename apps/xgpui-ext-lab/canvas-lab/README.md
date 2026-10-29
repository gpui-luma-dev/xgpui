# Canvas integration prototype

Run `just canvas-lab` from the repository root. Two-finger scrolling pans; pinch
zooms around the pointer. Space + left-button drag or middle-button drag also
pans. Left-click a card to select it. Press **G** for line/dot grids, **R** to
reset the camera, **O** for a 1% overview, and **B** to switch between 10,000 and
100,000 cards. The header
shows the visible count, zoom, and selection. Leaving the window clears held
Space/drag state.

The scene contains 10,000 or 100,000 world-space rectangles. Linear AABB culling happens
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
dots. This first experiment excludes rotation, custom inertia, spatial indexes,
and web integration. It does not establish arbitrary subtree scaling. Dense
overviews still record one native quad per proxy; spatial indexing does not
reduce that paint cost. Aggregated proxies and thumbnail caching remain follow-ups.

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
asserts selection for visible full cards, bounds proxies, and point proxies. Captures are for
visual inspection, not pixel assertions. The benchmark sweeps 10k/100k scenes at
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
