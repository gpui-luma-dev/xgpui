# Spatial canvas prototype

Review against [issue #54](https://github.com/gpui-luma-dev/xgpui/issues/54),
October 30, 2026. The native prototype is usable for experimentation; the RFC is
not complete. Browser companions now cover the card scene and CPU Mandelbrot example. The crate is experimental,
unpublished, and outside the generated release package set.

The repository's current locations are `extensions/crates/xgpui-canvas` and
`apps/xgpui-ext-lab/canvas-lab`, rather than the paths proposed in the RFC.
Run `just canvas-lab` from the repository root; it defaults to release mode.

## Try it

- [Open the spatial card canvas](../examples/extensions/?demo=canvas): 10,000 or
  100,000 retained rectangles, pan/zoom, BVH culling, LOD proxies, marquee
  selection, group movement with edge/center snapping and a density minimap.
- [Open the Mandelbrot explorer](../examples/extensions/?demo=mandelbrot): the
  native lab's f64 escape-time math and palette, with progressive browser work.
- Force `&backend=webgpu` or `&backend=webgl` to compare browser backends.

The card canvas scrolls to pan; Ctrl/Cmd+wheel or pinch zooms at the pointer.
Space+left drag or middle drag pans. Click selects, Shift-drag replaces the
selection, and dragging a selected card moves the group. Hold Alt/Option to
bypass snapping; Escape cancels. Click/drag the minimap to navigate. Use the
buttons to reset, zoom, change object count, switch lines/dots or show an overview.

The fractal drags to pan and uses wheel/pinch or the −/+ buttons to zoom. It
computes at most 2,048 pixels per render slice and checks a 4 ms elapsed budget
every 32 pixels. A 120-pixel-wide coarse image precedes a 480-pixel-wide refined
image (height capped at 360). Camera/viewport changes discard unfinished work;
the last complete image remains projected in world space until replaced. This
browser companion does not reproduce the native background thread or its
384-tile cache. Zoom is bounded to 100 million×; f64 precision and a 2,048
iteration cap limit boundary detail. It is CPU computation, not a GPU fractal
shader or arbitrary-precision exploration. Output is ordinary sRGB image content.

Run the full native versions from the repository root:

```sh
just canvas-lab
just canvas-lab --mandelbrot
```

The native card lab additionally has alignment guides, a geometry thumbnail and
an opt-in live viewport preview (`T` switches, `M` hides). The native fractal
uses cached world-space tiles and background computation. Browser minimap
geometry is a density summary; neither browser example performs live GPU readback.

## Use the extension in a view

The crate is experimental and unpublished; use a checkout path dependency:

```toml
xgpui-canvas = { path = "path/to/extensions/crates/xgpui-canvas", features = ["gpui"] }
```

Retain the camera, scene and compiled grid painter in view state:

```rust
use xgpui_canvas::{Camera2D, GridPainter, Point, Scene, SceneItem, WorldBounds};
let camera = Camera2D::default();
let grid = GridPainter::new()?;
let scene = Scene::new([SceneItem {
    bounds: WorldBounds { min: Point::new(0., 0.), max: Point::new(160., 80.) },
    data: "first card",
}]);
```

During render, cull before constructing children. Each visible entry supplies
`screen_bounds` and `detail`; place full-detail GPUI children at those projected
bounds and paint lightweight bounds/points for smaller LODs. Wrap content in
`Canvas::with_grid_options(camera, &grid, GridOptions::default()).size_full()`.
The container clips content and paints the procedural grid; it does not transform
an arbitrary subtree. Project positions, sizes and fonts explicitly.

For pointer input, subtract the viewport's window origin, then call
`camera.screen_to_world(local_point, viewport_size)` for hit testing or
`camera.zoom_at(local_point, viewport_size, factor)` for anchored zoom.
`PanGesture` supplies logical screen deltas for `camera.pan_by(delta)`; end it
on release, Escape, focus loss or ownership transfer. Caller selection policy
stays separate. Use `Scene::cache_key()` to invalidate geometry caches when
bounds change or a scene is replaced.

See the complete [browser consumer](https://github.com/gpui-luma-dev/xgpui/blob/codex/canvas-prototype/apps/xgpui-ext-lab/extensions-web/src/canvas.rs)
and [native lab](https://github.com/gpui-luma-dev/xgpui/tree/codex/canvas-prototype/apps/xgpui-ext-lab/canvas-lab).

## RFC coverage

| Capability | Current implementation | Remaining scope |
| --- | --- | --- |
| Camera and projection | f64 center/zoom, bidirectional projection, pointer anchoring, 0.01–64 zoom | Rotation; general subtree transforms |
| Input | Native lab scroll pan, pinch zoom, middle drag, Space + left drag | Reusable selection/gesture policy beyond pan; custom inertia deferred; wheel scroll pans |
| Grid | One main-pass shader quad, antialiased line/dot grids, fading minor/major levels | Configurable spacing/color/major interval implemented; isometric grid deferred |
| Culling and clipping | BVH query before child creation; clipped GPUI viewport | Rotated visibility; broader backend validation |
| Declarative integration | `Canvas::new(camera, &grid, style)` with projected children | RFC-style retained state and content callback are not implemented |
| Spatial index | Ordered AABB queries, topmost hit testing, per-item bounds refits | Incremental insertion/removal; bulk refits; rebuild policy |
| LOD | Projected-size suggestions; lab paints bounds/point proxies | Cached entity thumbnails; aggregate dense proxies; Parley-specific integration |
| Selection and snapping | Lab marquee/group move; core edge/center snapping with guides | Reusable gesture state; equal spacing; angle snapping; undo integration |
| Minimap | Lab whole-scene geometry thumbnail, viewport outline, click/drag recentering; scene identity/revision refresh | Reusable GPUI minimap component |
| Live viewport preview | Lab captures actual current view; T selects map/preview, M hides panel | GPU-resident capture or asynchronous readback |
| Delivery | Native lab, tests, local benchmark records, crate/lab READMEs and this page | Docs navigation/notes and crate-roadmap registered; Browser companions included; release packaging deferred |

## Reusable API boundary

The default crate has no dependencies: `Camera2D`, `Point`, `WorldBounds`,
`Scene<T>`, `SceneItem<T>`, `LevelOfDetail`, alignment snapping, and `Overview`.
The optional `gpui` feature supplies `Canvas`, `GridPainter`, and `GridStyle`.

Screen points are viewport-local logical pixels, positive right/down. Subtract
the viewport's window origin before inverse projection or pointer-anchored zoom.
The world center maps to the viewport center after resize. Pan takes logical
screen deltas. Keep world calculations in f64; convert projected values at the
GPUI boundary. Inputs must be finite and viewport dimensions nonnegative.

`Canvas` clips and paints the grid. Callers own camera state, event routing,
selection, overlays, culling, element construction, and explicit projection of
positions, sizes, and fonts. It does not scale an arbitrary GPUI element subtree.
`Scene::visible` suggests LOD without constructing elements or shaping text.
Query results preserve paint order; hit testing uses original world AABBs.

Keep these application policies in the lab for now: keyboard bindings, card
appearance, selection modifiers, capture scheduling, benchmarking, and smoke
fixtures. Extract a reusable input/gesture controller only after its cancellation,
focus, coordinate, and ownership contracts are explicit. Keep native window
capture separate from the dependency-free camera/scene API.

`Overview` is a density summary plus aspect-preserving map projection. It is not
a thumbnail renderer. The lab rasterizes card bounds for the whole-scene map;
the live preview captures the rendered viewport, including text/grid/overlays.

## Review findings and limits

1. Live preview performs synchronous GPU readback and two unpresented draws,
   throttled to one capture per 80 ms. The recorded local run has about 17.8 ms
   mean normal-view frame intervals with roughly 50 ms p95 at capture boundaries;
   dense overview averages about 46.2 ms. Hiding the panel or selecting the
   whole-scene map disables capture. This cost prevents treating the live preview
   as a cheap default reusable HUD.
2. Whole-scene geometry is cached by scene identity and geometry revision.
   Visible maps refresh at most once per render, including active movement,
   cancellation and scene replacement. Hidden maps rebuild when next shown.
   The default card HUD uses this map; T opts into the expensive live preview.
3. Large moves refit the BVH once per selected item per input update. Large
   groups can repeat ancestor work, and large relocations can degrade tree
   quality. Scene additions/removals require rebuilding. Establish a bulk update
   path and rebuild policy before claiming scalable dynamic scenes.
4. Dense queries still visit many/all items and sort k results in O(k log k).
   Dense overview paints one native quad per proxy. Sparse-query speed does not
   establish dense rendering performance or a worst-case logarithmic guarantee.
5. Validation evidence is local macOS arm64. The shader uses the portable
   pipeline, but this canvas lab has not established Windows/Linux/web behavior.
   Tests cover camera, queries, refits, snapping, overview, and shader layout;
   screenshots require visual inspection rather than pixel assertions.

## Native prototype milestone

The supported experimental surface is translation/scale camera math, retained
AABB scenes, edge/center snapping, overview projection, `PanGesture`, and optional
GPUI clipped viewport/grid painting. `PanGesture` supplies begin/update/end state;
callers end ownership on release/cancel/focus loss. `examples/pan.rs` demonstrates
a second consumer independent of lab card IDs and GPUI.

`GridOptions` configures spacing (0.001–1e12), major interval (2–16), style and
finite RGBA channels (0–1), validated by the painter. `Canvas::with_grid_options`
uses the same compiled procedural shader as the default constructor.

Wheel/trackpad scroll pans; pinch zooms around the pointer. Custom inertia,
rotation, isometric grids, general subtree transforms, reusable selection and
GPUI minimap widgets, incremental scene insertion/removal, aggregate proxies,
GPU-resident capture remain follow-ups. Release packaging
remains deferred: `publish = false`, no new crate versions or registry publication.
The native prototype and bounded browser companions are ready for review within this boundary; the full RFC
remains open. Other backend runtime validation is still required before claiming
cross-platform support.

## Large group move measurement

October 30 local macOS release CPU-only run, 100,000 retained objects, 20 updates
per case: one object <0.001 ms/update; 1,000 objects 0.038 ms; 10,000 objects
0.399 ms; 100,000 objects 3.979 ms. Run `examples/group_moves.rs` to reproduce.
These measure repeated per-item BVH refits for small translations, excluding
snapping, thumbnail generation, layout, rendering and OS scheduling. The existing
path is retained for this prototype; bulk refits/rebuild policy remain appropriate
follow-ups for large selections or large relocations. This is no frame-rate
or worst-case performance guarantee.

The native map-default benchmark is recorded in
`apps/xgpui-ext-lab/canvas-lab/bench/macos-arm64-finalization.json`:
normal 10k/100k views averaged 16.67 ms/frame (p95 17.66 ms); the dense
100k overview averaged 23.32 ms (p95 26.56 ms). This is a separate local run,
not a controlled comparison or a guarantee for other hardware/backends.

## Verification and evidence

Core checks (19 tests with the GPUI feature passed during finalization):

```sh
cargo test --offline --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-canvas/Cargo.toml \
  --features gpui --target-dir target
```

Native smoke and profiling:

```sh
just canvas-lab --smoke /tmp/canvas-smoke
just canvas-lab --benchmark /tmp/canvas-profile.json
```

The October 30 native smoke run passed selection/movement/cancellation, minimap
navigation, scene replacement cache invalidation, T/M/Escape ownership and focus
loss assertions. Captures are in `/tmp/canvas-final-smoke` locally. The headless
pan example also passed. Captures require visual inspection rather than pixel
assertions. Benchmark records live under the lab's `bench/` directory; measured
frame intervals include scheduling/layout/rendering and are not GPU timings.

## GPUI public API changes: additions and compatibility

Canvas uses existing GPUI containers, clipping and the xgpui-shader pipeline;
no GPUI public signature changes are required. Its own unpublished API remains
experimental. See [canvas integration compatibility](gpui-public-api.md#canvas-reuses-existing-gpui-apis).

## Browser validation checkpoint — October 30, 2026

The optimized `wasm32-unknown-unknown` extension bundle built successfully.
In local in-app Chromium, forced WebGPU and WebGL2 both rendered the card and
Mandelbrot companions. Card selection/movement, 100k overview, grid switching
and minimap navigation were exercised; fractal zoom/pan and refinement were
visually checked. No browser errors were observed. WebGL2 reported the existing
dual-source-blending capability and resize-poll timeout warnings while rendering.
This is functional/visual evidence, not a browser performance guarantee or
validation of every desktop gesture on every browser. Native fractal math/tile
regressions still pass after sharing the math module.
