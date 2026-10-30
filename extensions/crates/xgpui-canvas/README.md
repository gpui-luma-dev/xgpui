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
is clamped to 0.01–64 and anchored to the supplied local point. Applications
can opt into different finite positive limits with `zoom_at_with_limits`; they
remain responsible for precision and content behavior at those scales.

The camera projects geometry; `Canvas` clips and draws the grid. Callers own
input/state. Retain `Scene::new(items)` with `SceneItem { bounds, data }`, then
iterate `scene.visible(camera, viewport)` before constructing children. Each
result contains the original item, projected screen bounds, and a suggested
`LevelOfDetail`: `Point` below 4px, `Bounds` below 48px, otherwise `Full` (longest
projected side, in logical pixels). Use lightweight painting for proxies and
construct GPUI children only for full-detail content. Callers can choose their
own representation; the scene query does not draw anything.

`scene.hit_test(world_point)` returns the topmost AABB hit.
`scene.query(world_bounds)` returns every intersecting item, including boundary
contact, in back-to-front order. This can drive rectangular selection. Visibility,
rectangle queries, and hit testing use a median-split AABB BVH with
up to eight items per leaf. Spatial partitioning never changes caller paint order.
`scene.items()` exposes stable paint indices; `scene.set_bounds(index, bounds)`
updates one item and refits its leaf and ancestors without rebuilding the tree.
It returns false for invalid bounds or a missing index and leaves the scene
unchanged. Rebuild for additions/removals or to restore partition quality after
many large moves. Bounds must
have finite, ordered min/max coordinates. Proxy selection uses original world
bounds, without a screen-space tolerance.

Sparse queries prune unrelated nodes. Dense or heavily overlapping scenes can
still visit every item; ordered rectangle results also cost O(k log k) to sort
k matches. The index is not a guarantee of logarithmic queries, and it cannot
reduce the cost of painting thousands of visible proxies. Building the index
is synchronous. A large group move currently refits each selected item separately.

`scene.snap_translation(group_bounds, delta, tolerance, search_radius, exclude)`
finds the nearest matching group/item edges or centers independently on each
axis. Divide logical-pixel tolerance and search radius by camera zoom. Exclude
selected items from candidates. `SnapResult` supplies the corrected translation
and optional world-space X/Y alignment guides; callers own input and painting.
This is bounded local alignment, without equal-spacing or angle snapping.

`scene.bounds()` reads the current BVH extent in constant time. Build and retain
`Overview::new(&scene)` for a 32×32 center-density summary with at most 1,024
occupied cells. Each cell reports world bounds and an object count. Use
`world_to_map`/`map_to_world` for aspect-preserving projection and navigation;
inverse mapping clamps letterboxed regions to scene bounds. Map sizes must be
positive and finite; world spans must be finite. Empty and zero-area scenes
receive a usable fallback domain. This is an approximate density map, not a
thumbnail renderer. Rebuild the summary after scene edits, not on every redraw.
`camera.center_on(world)` recenters without changing zoom.

Project child positions, dimensions and font sizes explicitly. This is not a
general transform for arbitrary GPUI subtrees; rotation, transformed text APIs,
bulk index updates, cached layers and thumbnail LOD remain follow-ups. Inputs and
viewport dimensions must be finite; dimensions must be nonnegative.

This prototype is not registered for publishing or included in the generated
release package set. No framework rendering API changes are required.

See the native lab at `apps/xgpui-ext-lab/canvas-lab`; run `just canvas-lab`.

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-canvas/Cargo.toml --features gpui --target-dir target
```

CPU-only index comparison, including index construction time and result checks
against a linear reference:

```sh
cargo run --release --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-canvas/Cargo.toml \
  --example spatial_queries --target-dir target
```

See the [RFC coverage review](../../../docs/extensions/canvas.md) for the current
crate/lab boundary, known limits, and proposed next milestones.

## GPUI public API changes: additions and compatibility

Canvas uses existing GPUI containers, clipping and the xgpui-shader pipeline;
no GPUI public signature changes are required. Its own unpublished API remains
experimental. See [canvas integration compatibility](../../../docs/extensions/gpui-public-api.md#canvas-reuses-existing-gpui-apis).

## Input and grid configuration

`PanGesture::begin/update/end` supplies logical screen deltas without choosing
mouse buttons or modifiers. End it on release, Escape, focus loss and mode
changes. `examples/pan.rs` is a headless consumer without lab IDs or GPUI.

Use `Canvas::with_grid_options(camera, &grid, GridOptions { spacing: 40.,
major_interval: 4, color: [0.3, 0.4, 0.5, 1.], ..Default::default() })` for
configurable spacing, major interval and RGBA color. Spacing must be finite in
0.001..=1e12; interval is 2..=16; color channels are finite 0..=1. The painter
returns an error for invalid settings. Line/dot style shares one shader.

`Scene::cache_key()` combines process-local scene identity and geometry revision.
Use it for geometry caches, including replacement scenes starting at revision
zero. Caller-owned item data is immutable through this API; content caches with
additional application state need their own key.

## Browser companions

The website includes `?demo=canvas` (retained cards, selection/snapping and density
minimap) and `?demo=mandelbrot` (bounded progressive CPU fractal). They share this
crate with the native lab. See [controls, API and browser limits](../../../docs/extensions/canvas.md).
Build with `bash apps/xgpui-ext-lab/extensions-web/build.sh --release`.
