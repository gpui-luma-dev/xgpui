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
input/state. Retain `Scene::new(items)` with `SceneItem { bounds, data }`, then
iterate `scene.visible(camera, viewport)` before constructing children. Each
result contains the original item, projected screen bounds, and a suggested
`LevelOfDetail`: `Point` below 4px, `Bounds` below 48px, otherwise `Full` (longest
projected side, in logical pixels). Use lightweight painting for proxies and
construct GPUI children only for full-detail content. Callers can choose their
own representation; the scene query does not draw anything.

`scene.hit_test(world_point)` returns the topmost AABB hit. Items are stored and
queried in back-to-front paint order; hit testing reverses that order. Queries
are linear and scene data is immutable; rebuild the scene when its data changes.
Bounds must have finite, ordered min/max coordinates. There is no spatial index
yet. Proxy selection uses original world bounds, without a screen-space tolerance.

Project child positions, dimensions and font sizes explicitly. This is not a
general transform for arbitrary GPUI subtrees; rotation, transformed text APIs,
spatial indexes, cached layers and thumbnail LOD remain follow-ups. Inputs and
viewport dimensions must be finite; dimensions must be nonnegative.

This prototype is not registered for publishing or included in the generated
release package set. No framework rendering API changes are required.

See the native lab at `apps/xgpui-ext-lab/canvas-lab`; run `just canvas-lab`.

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-canvas/Cargo.toml --features gpui --target-dir target
```
