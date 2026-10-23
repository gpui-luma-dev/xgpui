# xgpui-shader

Design home for the custom shader and canvas extension described in
[#7](https://github.com/gpui-luma-dev/xgpui/issues/7).

The intended crate owns the public element API, shader parameters, retained GPU
resources, pipeline caches, diagnostics, and backend implementations/adapters as
the agreed dependency graph permits. GPUI supplies a small retained scene and
renderer integration contract.

The reference consumer is an animated mesh gradient that composes with normal
GPUI layout and styling. The Metal solid-color and Hermite mesh producers are implemented;
other backend adapters remain future work.

## Proposed first contract

Status: the macOS Metal solid-color and mesh slices are implemented. The generalized
cross-platform contract below remains a design proposal. Illustrative signatures
are not the exact shipped API; see `src/` for the first implementation.

### Implemented first slice

- `gpui::CustomContent`, `CustomDraw`, and `Window::paint_custom` retain content
  and participate in normal scene insertion, clipping, cloning, replay, and batches.
- `gpui_apple::custom_draw` exports an owned device/queue context and texture plus
  unsubmitted-command output. The host validates and submits preparation before
  ordered composition. Shared pending commands are submitted once.
- `xgpui_shader::shader(Arc<dyn CustomContent>)` returns a styled wrapper around
  existing div/canvas elements. `SolidColor::new([r,g,b,a])` supplies an immutable
  cached producer using an offscreen attachment clear.
- The initial intermediate is BGRA8Unorm with straight alpha and encoded sRGB
  channel values, matching the legacy Metal UI target's interpretation. No
  changes to existing color types, UI shader layouts, or window format.
- Payloads require Debug + Send + Sync; this avoids adding a non-thread-safe
  field to Scene. Preparation receives physical pixel size, with placement,
  clipping, radii, and opacity applied by the compositor.
- Cache identity is host-queue scoped. A retained pending command can be returned
  again after an aborted frame; submitted output is reused without new work.
- The compositor shader compiles lazily on first custom use. Existing frames with
  no custom records allocate no custom resources. Normal GPU submission does not
  wait for completion; test/readback paths intentionally do.
- Windows and wgpu reject custom batches explicitly; no implementation is claimed
  for them or for iOS consumers. Public primitive enums gain new variants,
  requiring downstream exhaustive matches to be updated.

Run and validation instructions are in [shader-lab](../../../../apps/shader-lab/README.md).
Still deferred: GPU timestamp measurement, pipeline compilation
scheduling beyond the initial compositor, generalized errors/fallback UI,
transparent-window validation, and device-loss/multi-window stress coverage.
The current renderer's error path reports a failed frame rather than silently
ignoring unsupported content.

### Objectives and limits

Expose custom GPU content as an ordinary styled element. Start with independent
offscreen rendering and ordered texture composition in SDR windows. Support mesh
geometry and buffers, rather than limiting the extension to fragment functions.

The first contract does not sample existing UI content, modify other primitives,
own a window drawable, present frames, or expose GPUI's active render pass.
SwiftUI-style effects over arbitrary child view hierarchies require an additional
subtree-rendering contract and are not implied by a shader element.

### Dependency graph

```text
application / mesh-gradient consumer
                  |
                  v
             xgpui-shader
               /     \
              v       v
            gpui   selected backend integration API
                         |
                         v
                        gpui
```

Backend modules in `xgpui-shader` depend on small public integration types exported
by the existing renderer crates. Those renderer crates never depend on
`xgpui-shader`. GPUI core knows only an opaque retained payload and geometry.
No global plugin registration is required for this proposal.

Before selecting dependencies, check target-specific packaging and compile-time
cost. In particular, Windows' renderer is currently private; its integration
types would need a new public module without exposing the renderer itself.

### Core record and preparation protocol

An additive `Window::paint_custom` entry point resolves ordinary paint state and
inserts an owned scene record. The record carries:

- Scene-assigned order, snapped bounds, and inherited rectangular mask.
- Resolved local corner radii and inherited element opacity.
- Shared ownership of an immutable content snapshot.

Conceptually, core defines a protocol resembling:

```rust,ignore
trait CustomContent: Any {
    fn prepare(
        &self,
        backend: &dyn Any,
        request: &CustomPrepareRequest,
    ) -> Result<Box<dyn Any>, CustomRenderError>;
}
```

This sketch uses checked `Any` downcasts, not raw pointers or layout casts.
The renderer passes an owned-handle context of its known backend type; the
extension returns that backend's known prepared-output type. The renderer
validates the output type and device identity. An unsupported context or invalid
output is an explicit error. A typed helper API in `xgpui-shader` should hide
this protocol from ordinary component authors.

`Any` requires static concrete types. Contexts must therefore contain owned GPU
handle clones, not borrowed render passes, encoders, or a borrowed renderer.
Borrowing the context for the duration of `prepare` is fine. Precise error types,
trait bounds, and thread requirements remain design decisions; do not impose
`Send + Sync` on all scene payloads without checking native and browser needs.

The preparation request specifies local logical size, allocated pixel size,
scale, device generation, and an explicit intermediate color/alpha contract.
Absolute placement, inherited clipping, and opacity belong to composition so
moving an unchanged element need not invalidate its image. Render the full local
element and clip on composition initially; visible-region rendering needs an
additional coordinate/cache policy.

Snapshots contain values, resource identities, and content revisions. They must
not contain borrowed view state or read mutable application state during
rendering. Persistent device resources can live behind an extension-owned cache
with interior mutability. The cache key includes device generation, content
revision, pixel size, and output contract; geometry-only movement should not
trigger shader recompilation or offscreen rerendering.

### Backend integration types

Each backend exports a narrowly scoped preparation context and output type:

| Backend | Context contains | Prepared output contains |
| --- | --- | --- |
| Metal | Active device handle and device generation/capabilities | Sampleable texture, recorded offscreen command buffer(s), retained dependencies |
| D3D11 | Active device handle and capabilities | Sampleable texture/SRV, recorded deferred command list(s), retained dependencies |
| wgpu | Active device handle and generation/capabilities | Sampleable texture/view, recorded command buffer(s), retained dependencies |

The output also describes extent and the agreed format/encoding/alpha mode.
Resource creation uses the host device. Commands are recorded but not submitted
by the extension. Backend wrappers should expose only what is necessary and
validate what can be validated. This is an in-process cooperation contract, not
an isolation boundary against malicious native extension code.

Queue ownership stays with the renderer. For Metal, how the host supplies the
queue used to allocate compatible command buffers needs an explicit API; an
owned queue handle may be necessary. The extension still must not commit those
buffers itself. D3D11 deferred-context support and command-list execution cost
must be validated on real supported devices before accepting this strategy.
If that path is unsuitable, consider a short-lived typed encoding callback in
the backend layer instead of expanding the core protocol prematurely.

### Frame execution and resource lifetime

1. Collect visible custom records and prepare independent outputs before the
   main scene pass. Reuse cached output when its content key is unchanged.
2. Submit/execute offscreen commands through the host's queue/context, ordered
   before the main pass samples their results. No CPU completion wait in normal
   drawing. Device/queue compatibility is a contract requirement.
3. At each custom primitive's position in `Scene::batches`, composite its output
   using a host-owned texture pipeline. Apply rectangular mask, local rounded
   coverage, and inherited opacity exactly once.
4. Keep resources valid until GPU completion. Do not equate dropping a scene or
   prepared CPU object with completion of its submitted commands.

State restoration belongs to the host. A custom producer cannot change the
main pass because it never receives that pass. Composition must still restore
scissor/viewport, pipeline, and bindings as required by subsequent draws.

The backend's generic texture compositor is a small integration cost that stays
in the renderer crate. Shader compilation, mesh generation, producer pipelines,
and effect-specific caches stay in the extension. This is preferable to exposing
an arbitrary main-pass callback solely to move the compositor out of GPUI.

### Style and color contracts

Use existing layout and style resolution. `Styled` alone does not guarantee
rounded content clipping: resolve radii explicitly and apply coverage in the
compositor. This contract covers the element's own rounded output plus inherited
rectangular masks; arbitrary rounded ancestor clipping is not promised.

Choose and document one SDR intermediate encoding/format and alpha convention
before implementation. Convert appropriately for each backend's current target
and blend state. Premultiplied intermediate output is a candidate, not a claim
that existing GPUI pipelines all use that convention. Avoid changing standard
UI shaders or public color types to accommodate the extension.

Never infer color encoding from Float16 storage. High-gamut presentation remains
the separate window capability tracked by #8.

### Scene reuse, scheduling, and errors

Scene replay clones the shared snapshot and recomputes ordering through normal
insertion. It does not consume a one-shot callback. Animation requests a GPUI
frame and invalidates the owning view so a new parameter snapshot is recorded.
Use existing scheduling; decorative animation observes reduced motion.

Empty/fully clipped records perform no preparation. Cache eviction and device
recreation must release stale outputs while respecting in-flight GPU ownership.
The local app must exercise multiple windows, including device identity changes
where supported. Repeated errors should not flood logs; fallback and error
reporting must be deterministic and inspectable in the app.

Headless rendering uses the same preparation/composition path. A test platform
without a GPU may retain the record for scene assertions, but must report that
pixel rendering is unsupported instead of claiming success.

## Primary mesh implementation reference

Use [patskovn/MeshGradient](https://github.com/patskovn/MeshGradient), reviewed at
commit [`f16b544`](https://github.com/patskovn/MeshGradient/tree/f16b544336b7bdaf7e3eba4ac6cba50e1160220e),
as the primary algorithm and Metal source reference. MeshingKit is useful for
component ergonomics and presets, but wraps Apple's built-in mesh renderer.

### Mapping into the extension

| Reference stage | Adaptation in xgpui-shader |
| --- | --- |
| ControlPoint / Grid | Validated immutable mesh snapshot: dimensions, positions, tangents, RGB colors |
| shuffleCoefficients compute kernel | Build per-patch Hermite geometry/color coefficient matrices |
| computeHermitPatchMatrix compute kernel | Evaluate interpolated positions/colors on a subdivided grid |
| computeMeshTrianglePrimitives compute kernel | Expand evaluated vertices into triangle geometry; consider indexed geometry later |
| prepareToDrawShader / drawRasterizedMesh | Draw geometry into the extension's offscreen texture |
| Optional computeNoize | Optional cached grain texture; keep disabled for the initial correctness baseline |
| MetalMeshRenderer / SwiftUI wrapper | Replace MTKView, default device, frame loop, and presentation with the retained GPUI element and host-device preparation contract |

The producer records compute and render commands, returns output and commands to
the host, and never presents or commits them itself. GPUI composites the result
at scene order with opacity and coverage. The generic integration must support
this sequence without mesh-specific changes in GPUI.

### Adapt rather than copy the renderer

Source inspection found concrete issues to address in a port:

- `MTLDrawMeshTrianglesFunction.swift` submits one draw call per triangle. Use a
  single triangle-list draw for the contiguous output buffer, unless a measured
  correctness constraint requires splitting it.
- `MTLComputeNoiseFunction.swift` commits a separate buffer and calls
  `waitUntilCompleted`. Record noise and mesh work with host-controlled ordering;
  do not introduce a CPU completion wait in normal drawing.
- Noise dispatch divides dimensions by an 8x8 group size with truncation. Inspect
  the kernel bounds checks and use dispatch/rounding that covers non-multiple
  extents safely. Include tiny and odd texture sizes in tests.
- `MeshAnimator.swift` mutates animation on grid access and advances according
  to configured FPS. Use explicit elapsed time and seeded randomness so retries,
  scene replay, and variable refresh rates do not change animation progression.
- The reference color evaluator outputs alpha 1 and directly interpolates RGB.
  It does not establish Oklab, linear-light, wide-gamut, or HDR correctness.
  Choose the SDR interpolation/output contract explicitly and keep element
  opacity in composition.
- The subdivided output includes patch endpoints and joins neighboring patch
  blocks. Verify boundary continuity, triangle winding, seams, folds/overlaps,
  and degenerate triangles before changing its topology or sample indexing.
- Metal's SIMD alignment must be reflected in Rust upload types. For example,
  a float2 followed by a float4 needs appropriate padding/alignment; do not pack
  fields merely according to the sum of scalar sizes. Validate strides/offsets
  independently against the shader interface.
- Validate grid dimensions, point count, finite values, subdivisions (at least
  two for the reference's division by `s - 1`), integer overflow, and allocation
  limits before recording work.
- Rework completion-based buffer recycling for the host submission lifecycle;
  scene replay must never recycle buffers still in use by an earlier frame.

Useful source files at the reviewed revision:

- [Renderer](https://github.com/patskovn/MeshGradient/blob/f16b544336b7bdaf7e3eba4ac6cba50e1160220e/Sources/MeshGradient/MetalMeshRenderer.swift)
- [Hermite evaluation](https://github.com/patskovn/MeshGradient/blob/f16b544336b7bdaf7e3eba4ac6cba50e1160220e/Sources/MeshGradient/MTLComputeHermitPatchMatrix.metal)
- [Mesh draw wrapper](https://github.com/patskovn/MeshGradient/blob/f16b544336b7bdaf7e3eba4ac6cba50e1160220e/Sources/MeshGradient/MTLDrawMeshTrianglesFunction.swift)
- [Shared shader types](https://github.com/patskovn/MeshGradient/blob/f16b544336b7bdaf7e3eba4ac6cba50e1160220e/Sources/MeshGradientCHeaders/include/MetalMeshShaderTypes.h)

The reference uses the MIT license, copyright 2022 Nikita Patskov and others.
The adapted Metal implementation preserves its copyright and permission notice
in LICENSE-MeshGradient.

### Local validation progression

1. Solid-color producer: prove host submission, scene replay, ordering, clipping,
   opacity, rounded coverage, and cleanup before debugging mesh math.
2. Static mesh without grain: 2x2 and 3x3 grids, fixed inputs, patch seams,
   tangents, odd viewport sizes, and CPU reference samples for Hermite math.
3. Animated mesh: deterministic seed, elapsed-time progression, pause/resume,
   static output reuse, repeated resize, and frames in flight.
4. Optional grain: non-multiple-of-eight sizes, zero intensity, cache invalidation,
   GPU ordering, and no synchronous wait.
5. Performance sweeps: baseline UI, cached static mesh, animated mesh, and grain;
   report draw count, subdivisions, texture extent, and timing conditions.

## Implemented mesh producer

`MeshGradient` implements the same `CustomContent` protocol as `SolidColor`.
This increment makes no additional changes to GPUI or renderer APIs. The Metal
implementation lives in `src/mesh/`; attribution is in `LICENSE-MeshGradient`.

```rust,ignore
use gpui::{prelude::*, px};
use std::sync::Arc;
use xgpui_shader::{MeshGradient, MeshGrid, shader};

let grid = MeshGrid::regular(2, 2, vec![
    [1.0, 0.1, 0.2], [0.2, 0.4, 1.0],
    [1.0, 0.8, 0.1], [0.1, 0.8, 0.6],
])?;
let mesh = Arc::new(MeshGradient::new(grid, 18)?);
let element = shader(mesh.clone()).w(px(480.0)).h(px(320.0)).rounded_lg();
```

Keep the Arc for static reuse. To edit/animate, construct a validated grid from
new control points and use `mesh.with_grid(new_grid)`; this creates an immutable
snapshot while sharing compiled pipelines. `with_subdivisions` does the same for
quality changes. Do not call `MeshGradient::new` every animation frame, which
would discard the shared pipeline cache.

Public `ControlPoint` values have normalized top-left-origin positions, u/v
patch tangents, and opaque encoded-sRGB RGB colors. `MeshGrid::regular` initializes
straight, uniformly spaced geometry. For custom geometry, clone `grid.points()`,
edit the points, and validate them with `MeshGrid::new(width, height, points)`.
Positions/tangents can produce folds or gaps; clipping and deterministic triangle
order are supported, but this API does not repair self-intersecting geometry.

The adapted patskovn pipeline computes Hermite coefficient matrices, evaluates
patch-local vertices, expands triangles, then records one triangle-list draw.
Shared-edge samples are duplicated per patch rather than connected by artificial
cross-patch cells. Each compute dispatch is guarded; tracked-resource encoder
boundaries order the stages. The host alone commits the recorded command buffer.

Rust upload records have explicit 16-byte alignment and float4-sized fields;
Metal compile-time assertions and unit tests check strides/offsets. Colors use
the reference's Hermite RGB interpolation with zero color tangents. This is not
Oklab interpolation or high-gamut output. Grain is not included in this slice.

Resource limits: 2..=16 control points per axis; 2..=64 samples per patch edge;
64 MiB maximum geometry buffers and 64 MiB maximum output texture per generation.
Finite positions/tangents must be in -16..=16, RGB in 0..=1. Geometry is immutable
per snapshot, output is cached per current queue/extent, and compilation is
shared per current host queue across related snapshots. Animated generations
currently allocate fresh buffers/textures; pooling and total in-flight budgets
remain future performance work.

Real-GPU tests compare against an independent CPU Hermite basis evaluator for
2x2, rectangular 3x2, and deformed 3x3 grids. They also check complete coverage,
replay, resize/opacity, old snapshot preservation, shared pipeline compilation,
minimum subdivisions, shared first-use output, and aborted-frame retry.

## Compatibility budget

| Surface | Proposed impact |
| --- | --- |
| Existing `Element`, `Render`, `canvas`, and view APIs | No signature changes |
| Existing colors and GPU-uploaded primitive layouts | No changes |
| `Window` | Additive paint method |
| Scene | New retained record collection and insertion/replay/batching handling |
| Public `Primitive` / `PrimitiveBatch` | New variants break downstream exhaustive matches; document and accept only after review |
| Existing platform traits | No new required method in this proposal |
| Renderer crates | Add public backend integration types and internal preparation/composition dispatch |
| Existing applications without custom records | No custom target allocation or preparation; measure any dispatch overhead |

Core's new payload ownership may affect auto traits such as `Send`/`Sync` even
without changing a method signature. Audit existing `Scene` constraints and
backend ownership before choosing the trait bounds. Adding `non_exhaustive` now
would itself be a compatibility change, not a way to erase the enum break.

The touched core paths are `scene.rs`, `window.rs`, and public exports, plus
backend dispatch and compositor code. Also inspect scene labels, diagnostics,
debug/profiling overlays, tests, headless paths, and GPU header generation.
Keep retained Rust objects outside generated shader structs.

## Local test/performance app

`../../../../apps/shader-lab/` is a required deliverable. It should run against local
path dependencies and provide a baseline UI plus a mesh-gradient panel, effect
toggle, animation pause, control-point/color editing, and workload presets.

Compare static cached output, animated output, and effect-disabled baseline.
Measure preparation, encoding/composition, and GPU time where supported; report
warmup separately. Exercise layout, overlap ordering, rounded edges, opacity,
scrolling, scene replay, scaling, resize, removal, multi-window use, and failures.
Use the same contracts for visual/headless checks. Agree budgets after gathering
baseline data; a successful root build does not validate other platform backends.

## Next design decisions

The first slice establishes isolated producers, checked type erasure,
host-controlled submission, straight-alpha SDR output, and Metal-first staging.
Before generalizing it:

1. Validate D3D11 deferred recording and wgpu adapters rather than assuming that
   Metal's command model transfers directly.
2. Decide whether producers need logical size, scale, target color contracts,
   or explicit device-generation metadata beyond the current physical extent
   and queue-scoped context.
3. Define broader cache eviction, resource budgets, error/fallback reporting,
   transparent-window behavior, and device-loss handling.
4. Measure animated mesh allocations before adding buffer/texture pooling or
   optional grain; the compute/geometry producer already uses this contract.
5. Review public enum compatibility and collect optimized performance budgets.

## Component use

Keep a producer handle in application state to reuse static output:

```rust,ignore
use gpui::{prelude::*, px};
use std::sync::Arc;
use xgpui_shader::{SolidColor, shader};

let content = Arc::new(SolidColor::new([0.2, 0.6, 0.9, 1.0])?);
let element = shader(content)
    .w(px(240.0))
    .h(px(160.0))
    .rounded_lg();
```

Create a new immutable snapshot when parameters change. Internally the producer
uses the host Metal device and queue; application view code does not encode or
submit GPU commands.

## Extension paint integration

The first generic paint milestone also accepts the same producer in
`div().bg_paint(mesh.clone())`, `div().border_paint(mesh.clone())`, and
`window.paint_path_with_paint(path, mesh.clone())`. The mesh mathematics and GPU
producer stay in this crate. GPUI owns native coverage masks and composition.
See [paint-lab](../../../../apps/paint-lab/README.md) and the
[paint contract](../../docs/extension-paints.md) for current scope and API impact.
