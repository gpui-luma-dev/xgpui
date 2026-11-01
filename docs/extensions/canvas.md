# Spatial canvas

<aside class="canvas-beta" aria-label="Canvas beta warning"><strong>BETA</strong> — Expect significant API changes.</aside>

`xgpui-canvas` adds a spatial viewport to GPUI: an f64 pan/zoom camera, retained
scenes with BVH culling and hit testing, LOD hints, alignment snapping, and a
procedural line/dot grid. Native and browser labs show the same camera driving
cards and fractals. The native fractal uses coherent viewport images; the browser
companion uses bounded CPU rendering.

## Try it

- [Card canvas](../examples/extensions/?demo=canvas): 10k/100k cards, selection,
  group movement, snapping and a navigable density minimap.
- [Mandelbrot explorer](../examples/extensions/?demo=mandelbrot): progressive
  CPU rendering with pointer-anchored zoom.

Both support WebGPU and WebGL2. Run the native labs from the repository root:

```sh
just canvas-lab
just canvas-lab --mandelbrot
```

| Action | Cards | Mandelbrot |
| --- | --- | --- |
| Pan | Scroll, middle drag or Space-drag | Left drag |
| Zoom | Pinch; browser also supports Ctrl/Cmd+wheel | Pinch; browser also supports wheel |
| Select/move | Click or Shift-drag; drag selected cards | — |
| Bypass snapping / cancel | Alt/Option / Escape | — |
| Reset | R or browser Reset button | R or browser Reset button |

Click/drag the card minimap to navigate. Native **T** switches minimap/live
preview; **M** toggles the panel. Browser buttons change scene size and grid style.

## Use it

For development against this checkout:

```toml
xgpui-canvas = { path = "path/to/extensions/crates/xgpui-canvas", features = ["gpui"] }
```

Retain a `Camera2D`, `Scene<T>` and `GridPainter` in view state. Cull before
building children, then place them using the returned screen bounds:

```rust
for entry in scene.visible(camera, viewport_size) {
    // entry.item: your data and world bounds
    // entry.screen_bounds: viewport-local position and size
    // entry.detail: Point, Bounds or Full
}

Canvas::with_grid_options(camera, &grid, GridOptions {
    spacing: 40.,
    major_interval: 4,
    ..Default::default()
}).size_full().children(children)
```

`Canvas` clips and paints the grid. Project child positions, sizes and fonts
explicitly. `GridOptions` also controls line/dot style and RGBA color.

Subtract the viewport origin from pointer coordinates before calling
`screen_to_world` or `zoom_at`. Feed `PanGesture` deltas to `camera.pan_by`;
end the gesture on release, Escape or focus loss. Use `scene.hit_test` for
selection, `scene.snap_translation` for alignment, and `scene.cache_key()` to
invalidate geometry caches.

See the [crate API](#crate=xgpui-canvas&tab=surface),
[browser implementation](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/extensions-web/src/canvas.rs)
and [native lab controls and benchmarks](https://github.com/gpui-luma-dev/xgpui/tree/main/apps/xgpui-ext-lab/canvas-lab).

## Scope and performance

The native fractal presents complete buffered viewport images, reprojecting the
last completed image during navigation. Shallow Metal views use GPU calculation
with linear P3 float textures at a steady resolution; deep views use f64 CPU
calculation, retain sharper completed images where possible, and blend in detail
after settling. GPU deep-zoom
perturbation is not implemented. The browser retains bounded CPU slices, refining
a coarse image to at most 480×360; f64 precision and a 2,048-iteration cap limit
deep detail. The native live preview uses GPU readback; the default minimap
avoids that cost.

Local macOS measurements averaged 16.7 ms/frame in normal card views and
23.3 ms in the dense 100k overview. These are observations, not guarantees.
The original native smoke checkpoint recorded 19 core tests and three fractal
tests; the native frame redesign adds scheduler/precision tests and Metal pixel
comparison with the f64 reference. Both browser demos were checked in Chromium with WebGPU and WebGL2. Native Windows/Linux
runtime validation remains outstanding.

Rotation, isometric grids, general subtree transforms, reusable selection/map
widgets and large-scene indexing/rendering improvements remain follow-ups in
[issue #54](https://github.com/gpui-luma-dev/xgpui/issues/54).

## GPUI public API changes: additions and compatibility

Canvas uses existing GPUI containers, clipping and the shader pipeline, with no
new GPUI signatures or fields. See [integration compatibility](gpui-public-api.md#canvas-reuses-existing-gpui-apis).
