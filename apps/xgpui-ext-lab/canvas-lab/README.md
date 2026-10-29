# Canvas integration prototype

Run `just canvas-lab` from the repository root. Two-finger scrolling pans; pinch
zooms around the pointer. Space + left-button drag or middle-button drag also
pans. Left-click a card to select it. Press **G** for line/dot grids, **R** to
reset the camera, and **B** to switch between 10,000 and 100,000 cards. The header
shows the visible count, zoom, and selection. Leaving the window clears held
Space/drag state.

The scene contains 10,000 or 100,000 world-space rectangles. Linear AABB culling happens
before constructing GPUI children, so offscreen cards do not enter layout or
text shaping. Each visible card is an ordinary positioned GPUI div with an
explicitly projected size and font size; this is not a general subtree transform.
Borders remain one logical pixel. The viewport clips drawing and input through
the normal GPUI container, and clicks are checked against inverse-projected
world bounds.

The unpublished `xgpui-canvas` crate holds dependency-free f64 camera math; its
optional `gpui` feature supplies the reusable `Canvas` container and grid painter.
Screen coordinates are local logical pixels; world origin starts at the viewport
center. Zoom is bounded to 0.01–64. Projection converts to f32 only at the GPUI
boundary.

The grid uses one procedural shader quad, with fading minor/major lines or
dots. This first experiment excludes rotation, custom inertia, spatial indexes,
and web integration. It does not establish arbitrary subtree scaling. At very
small zoom, constant-width card borders merge; automatic LOD/proxies and text
suppression remain follow-ups. Culling helps offscreen content, but does not
reduce layout cost when every object fits onscreen.

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
and a resized viewport. They are for visual inspection, not pixel assertions.
The benchmark sweeps the camera at 1x zoom with 30 warmup and 120 measured frames
for each scene size. It saves JSON and line/dot PNGs captured during warmup.
CPU timings cover culling and child construction only. Frame intervals include
layout, rendering and OS scheduling; neither measurement is a GPU timing.

The October 29, 2026 local macOS arm64 release baseline is saved under
`bench/macos-arm64-baseline.json`. At a 1100×702 logical viewport and 2× display
scale, mean frame intervals were 16.66/16.67 ms for 10k/100k objects; mean CPU
cull/build times were 0.11/0.37 ms. Only 36–49 cards were visible. This is one
local scheduling measurement, not a performance guarantee for densely visible
scenes or other hardware/backends.
