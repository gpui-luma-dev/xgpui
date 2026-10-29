# Spatial canvas prototype

Review against [issue #54](https://github.com/gpui-luma-dev/xgpui/issues/54),
October 29, 2026. The native prototype is usable for experimentation; the RFC is
not complete. Web integration is deferred by request. The crate is experimental,
unpublished, and outside the generated release package set.

The repository's current locations are `extensions/crates/xgpui-canvas` and
`apps/xgpui-ext-lab/canvas-lab`, rather than the paths proposed in the RFC.
Run `just canvas-lab` from the repository root; it defaults to release mode.

## RFC coverage

| Capability | Current implementation | Remaining scope |
| --- | --- | --- |
| Camera and projection | f64 center/zoom, bidirectional projection, pointer anchoring, 0.01–64 zoom | Rotation; general subtree transforms |
| Input | Native lab scroll pan, pinch zoom, middle drag, Space + left drag | Reusable input wiring; custom inertia; explicit wheel-zoom policy |
| Grid | One main-pass shader quad, antialiased line/dot grids, fading minor/major levels | Isometric grid; configurable spacing, colors, major interval |
| Culling and clipping | BVH query before child creation; clipped GPUI viewport | Rotated visibility; broader backend validation |
| Declarative integration | `Canvas::new(camera, &grid, style)` with projected children | RFC-style retained state and content callback are not implemented |
| Spatial index | Ordered AABB queries, topmost hit testing, per-item bounds refits | Incremental insertion/removal; bulk refits; rebuild policy |
| LOD | Projected-size suggestions; lab paints bounds/point proxies | Cached entity thumbnails; aggregate dense proxies; Parley-specific integration |
| Selection and snapping | Lab marquee/group move; core edge/center snapping with guides | Reusable gesture state; equal spacing; angle snapping; undo integration |
| Minimap | Lab whole-scene geometry thumbnail, viewport outline, click/drag recentering | Reusable component; revision-aware refresh during movement |
| Live viewport preview | Lab captures actual current view; T selects map/preview, M hides panel | GPU-resident capture or asynchronous readback |
| Delivery | Native lab, tests, local benchmark records, crate/lab READMEs and this page | Web demo, docs navigation/notes and crate-roadmap registration, packaging |

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
2. Whole-scene geometry is cached and refreshed on committed moves, scene
   switches, and mode switches. During an active move it can show the old card
   positions until commit. The viewport outline follows the camera immediately.
   A reusable component should key geometry by scene identity/revision and
   coalesce rebuilds, including cancellation and scene replacement.
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

## Next implementation plan

1. Harden the existing lab modes: coalesce whole-scene thumbnail refreshes by
   revision during movement/cancellation; test T/M transitions and focus loss
   during minimap and world gestures. Preserve both map and current-view modes.
2. Extract reusable input/gesture state with explicit begin/update/commit/cancel
   behavior and caller-owned selection policy. Add a second minimal consumer to
   prove the API does not depend on the lab's card IDs or keyboard choices.
3. Add configurable grid parameters, then measure group moves and dense views.
   Use those measurements to prioritize bulk refits, aggregation, or preview
   capture improvements. Keep live capture opt-in until its cost is addressed.
4. Decide whether rotation/isometric grids are required for the first supported
   milestone. Document their deferral explicitly if the milestone remains
   translation/scale only. Register docs/roadmap and packaging when the supported
   API scope is agreed. Web integration remains deferred.

This sequence keeps the working prototype available while establishing a smaller,
reviewable supported surface. No PR or remote issue update is part of this review.

## Verification and evidence

Core checks (15 tests with the GPUI feature passed during this review):

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

The latest native smoke run passed selection/movement/cancellation and minimap
navigation assertions, with visual captures of both modes. It preceded this
documentation-only review. Benchmark records live under
`apps/xgpui-ext-lab/canvas-lab/bench/`; the crate README gives the CPU-only spatial
query comparison command. Reported frame intervals include OS scheduling,
layout, and rendering; CPU cull/build times are not GPU timings. The separate
local runs are evidence, not a controlled cross-backend performance guarantee.
