# xgpui-canvas (experimental, unpublished)

Spatial camera and clipped viewport prototype for issue #54. Default features
provide dependency-free f64 camera math. Enable `gpui` for `Canvas`, `GridPainter`,
and `GridStyle` through the existing portable shader pipeline.

Retain a `Camera2D` and `GridPainter::new()?` in view state. Build the viewport
with `Canvas::new(camera, &grid, GridStyle::Lines).size_full().children(children)`.
`GridStyle::Dots` uses the same compiled program. The grid is one main-pass quad,
with antialiased minor/major levels and fading at zoom transitions. World phases
are reduced modulo their spacing in f64 before GPU conversion.

Screen coordinates are **viewport-local logical pixels**, positive x right and
positive y down. Subtract the viewport's window origin from pointer positions
before `screen_to_world` or `zoom_at`. The world center maps to the viewport
center, including after resizing. `pan_by` accepts logical screen deltas; zoom
is clamped to 0.01–64 and anchored to the supplied local point.

The camera projects geometry; `Canvas` clips and draws the grid. Callers own
input/state and must query `visible_world_bounds` before constructing children.
Project child positions, dimensions and font sizes explicitly. This is not a
general transform for arbitrary GPUI subtrees; rotation, transformed text APIs,
spatial indexes, cached layers and automatic LOD remain follow-ups. Inputs and
viewport dimensions must be finite; dimensions must be nonnegative.

This prototype is not registered for publishing or included in the generated
release package set. No framework rendering API changes are required.

See the native lab at `apps/xgpui-ext-lab/canvas-lab`; run `just canvas-lab`.

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-canvas/Cargo.toml --features gpui --target-dir target
```
