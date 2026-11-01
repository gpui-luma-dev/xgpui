# Portable content API and execution lifecycle

Status: design with staged implementation, 2026-11-01. CPU contracts, Metal and
wgpu execution/presentation and portable producer migration are implemented.
D3D11 execution and the later external-image integrations remain pending.
Issue: [#110](https://github.com/gpui-luma-dev/xgpui/issues/110).
Baseline regression: [#111](https://github.com/gpui-luma-dev/xgpui/pull/111).

This specification replaces producer-owned GPU preparation with immutable content
descriptions and framework-owned execution. Correctness and functionality take
priority over source compatibility. Rust declarations below specify the proposed
surface; full-pipeline examples illustrate the target and are not yet executable
against today's SDK.

## Decisions

1. Applications and portable extensions use one content API. Backend selection,
   native texture allocation, recording, submission, and completion tracking are
   renderer responsibilities.
2. Description is CPU-only. A producer cannot reserve a native command buffer,
   perform a GPU wait, or obtain a raw queue through the portable builder.
3. Typed operations form a dependency graph. The executor allocates resources
   and records bounded chunks while advancing submission and completion.
4. Content values are immutable snapshots. Graph references are temporary;
   reusable content handles are persistent. Neither exposes a command buffer.
5. Resource admission can defer work without blocking the UI. Impossible requests
   fail explicitly. No arbitrary producer-count cutoff is a correctness fix.
6. Submission is not a whole-frame transaction. Output publication is atomic for
   each immutable result; successfully completed independent results may survive
   a failed frame. Submitted work always keeps its owners until safe release.
7. The same contract applies to Metal, D3D11, and wgpu. Capability differences
   produce typed results, not application branches on backend names.
8. Native interoperability has an explicit advanced boundary. It cannot bypass
   synchronization and still claim the portable path's guarantees.

## Current seams being replaced

| Current surface | Replacement |
| --- | --- |
| `CustomContent::prepare*` with `Any` contexts and outputs | `ContentSource::describe`, typed builder and image references |
| Separate Metal/D3D11/wgpu output types | Common image descriptors and internal device-specific realizations |
| Producer caches keyed by native queue addresses/status | Framework caches keyed by immutable content and device generation |
| Separate texture and instanced paint choices | One semantic paint; renderer chooses a valid direct or materialized lowering |
| Producer-specific upload admission and worker GPU commands | Shared admission and staged-data adoption |
| Implicit ownership through a final compositor buffer | Explicit ownership on every submitted use, including failure paths |

Existing behavior is documented in [paint contracts](paint-contract-hardening.md),
[public API inventory](gpui-public-api.md), [portable paints](portable-paints.md),
and [media images](media-images.md). Those documents continue to describe the
current implementation until migration; this proposal does not redefine it.

## CPU foundation checkpoint

`gpui::content` now provides immutable `Content`/`ContentSource`, checked image and
buffer descriptors, builder-scoped references, CPU solid/upload/copy operations,
structural realization keys, and a dependency-first validated graph. Checked
`ImageExtent` and descriptor constructors/accessors replace freely writable
descriptor fields in the conceptual declarations below. A solid can be rebuilt
inline without losing structural equivalence; no device cache is implemented yet.

The reference `ExecutionPlan` accepts executor-supplied allocation charges,
including padding, and rejects an infeasible serial working set. Its tracker
enforces planned admission order, live-byte and unsubmitted-recording allowances,
submission/completion/discard ordering, invalidation, and last-reader retirement.
`SubmissionLease` retains output/input owners independently of tracker teardown;
an executor must install it in the native completion callback before submission.
Owner types can be thread-safe `Arc` or thread-affine `Rc`. `PresentationEpoch`
rejects stale and cross-window generation tokens.

`PortableProgram` now parses and validates WGSL with Naga, links selected
vertex/fragment interfaces, reflects used resource bindings (including helper
function use), and records image interpretation contracts. It supports generated
fullscreen vertices, mesh vertex-input reflection, compute workgroup/dispatch
validation, storage buffer runtime tails, and storage-image format/access
reflection. Programs have structural equality and are worker-safe CPU artifacts.
`UniformValues` packs f32/i32/u32 scalars and vectors and f32 matrices through
reflected offsets, with finite-value checks and initialized padding. Source WGSL
validation does not prove actual pixel color/alpha meaning or native compilation.

`ContentBuilder::render` and `compute` now build validated pass operations.
Fullscreen and triangle-list mesh passes support named images/samplers, typed
uniforms, read-only storage, vertex/instance streams, index ranges, explicit
clear/load, and replace or premultiplied-linear source-over. Compute passes produce
named fresh image/buffer versions with defined initialization or explicit initial
copies. Read/write declarations cannot mutate an existing immutable input.

One compute operation can have multiple outputs. Builder references and graph
roots carry an output selector; selecting two outputs never duplicates dispatch.
`ContentNode::outputs` describes the whole operation and `byte_len` charges all
outputs atomically. `operation_key` identifies that operation; per-resource caches
must use `output_key` so equally described outputs remain distinct. The reference
tracker conservatively retains an output group until all its readers retire.
Image usage is inferred from consumers during native planning; device realization
keys must include required native usages so a cached sampled-only allocation
cannot silently serve a later storage/render use.

Pass validation covers exact names/kinds/access, program/target interpretation,
buffer ranges/alignment, reflected vertex attributes, draw and instance bounds,
render/storage/filtering capabilities, and actual reflected sampler/image pairs.
Uniform payload is counted in graph limits; native allocation charges must also
include GPU uniforms/staging rather than treating output bytes as the entire
device working set. Index values and shader pixel meaning remain producer
contracts. Native lowering must protect vertex fetches for GPU-generated indices;
the CPU validator proves bound ranges, not the values generated by a shader.

The graph foundation now runs through the native Metal executor and the portable
window presentation path described below. Legacy CustomContent producers retain
their old path and still require migration for #110. The reference planner charges
every output as materialized. Asset readiness, external imports, advanced mapping,
and other backend implementations remain implementation work.

Run the deterministic integration suite from the repository root:

```sh
cargo test --manifest-path gpui/crates/gpui/Cargo.toml \
  --test portable_content --test portable_content_programs \
  --test portable_content_passes \
  --features gpui_platform/runtime_shaders
```

The focused integration target avoids an existing unrelated property-test macro
failure in the library unit-test target. Tests use fake completion notifications
and do not claim native GPU validation. The Metal baseline regression remains
unchanged and is still expected to reproduce the deadlock.

What to look for at this checkpoint: all three targets pass (22 graph/lifecycle,
17 program/uniform, and 27 pass-graph tests: 66 total). No window opens and no native GPU access is
required. Matrix tests cover all nine 2–4-column/row combinations; shader tests
reject mismatched stages, conflicting resource addresses, wrong/missing image
contracts, invalid uniforms, and unavailable/excessive compute dispatch. Pass tests
cover compute-to-mesh, multi-output selection/retirement, resource remapping,
atomic working sets, load/blend, filtering, draw ranges, and rejected bindings. Existing
apps should render as before because the new artifacts are not wired into draws.

### Native Metal execution checkpoint

`gpui_apple::content_executor` now lowers validated graphs to native Metal on a
dedicated serial worker. This backend interface is for headless validation;
application-facing content stays backend-neutral. Setup initializes a native device
and queue; submit is nonblocking and returns an awaitable job with CPU-only image
readback. Blocking readback waits are for tests outside UI callbacks.

The worker preflights/caches pipelines, charges native output/staging/uniform/size
buffer allocations, admits each operation, and commits before requesting another
command buffer beyond its queue allowance. Completion owns resource leases.
Failures and cancellation drain the submitted prefix, discard admitted/unencoded
reservations, and retire resources before accepting another job's GPU budget.
Dropping a job requests cancellation; dropping the executor handle does not join
the worker. A caller timeout cannot interrupt a hung driver.

Implemented operations include solids, mip uploads/copies, buffer zero/upload/copy,
render load/blend, uniforms, sampled images, robust mesh/index/instance vertex
pulling, and multi-output compute with dynamic workgroup memory and storage-size
tables. Native resource slots and actual pipeline threadgroup limits are checked.
Pipeline variants have an entry-count bound separate from buffer/texture byte
charges. The headless executor remains independently owned; window renderers now
share a device service with persistent output accounting as described below.

Run on macOS with an available Metal GPU from the repository root:

```sh
cargo test --locked --manifest-path gpui/crates/gpui-apple/Cargo.toml \
  --test portable_content_metal --features runtime_shaders,test-support \
  -- --test-threads=1 --nocapture
```

What to look for: **15 passed, 0 failed, 1 ignored**. The ignored test is a helper
that the parent dense test runs in a child process with a 45-second deadline.
Dense progress must report verified pixels and `max_in_flight=1` throughout three
512 → 1 → 512 cycles, each replayed twice: 6,150 producer operations and 6,150
readbacks. Tests also check FP16/P3 values, RGBA/BGRA/R8 ordering, mips, non-diagonal
matrix packing, multi-output dispatch, generated indices, declared vertex bounds,
workgroup barriers/runtime array lengths, cancellation, queue pressure, failure
recovery, and awaitable completion after dropping the executor handle. No window
opens. To also enable Apple's native API validation, prefix the command with
`MTL_DEBUG_LAYER=1`; the suite must still pass without validation errors. Both
normal and validation-enabled runs passed on the development machine. The original
WideColor regression is now migrated as described below; keep #110 open for the
remaining legacy producer work.

### GPU-only window presentation checkpoint

`Window::paint_content(CustomGeometry, ContentDraw)` records a backend-neutral
source and presentation policy. `ContentDraw::slot(PresentationSlotId)` supplies a
stable logical paint role; allocate that ID once per canvas role, then retain it
across source replacement. Slot IDs are scoped by the renderer/window, are not
source or native addresses, and duplicate use in one scene is an error. The
scene currently uses a private bridge through existing CustomDraw geometry;
portable producers never receive a native context or submit commands.

Each Metal device has one shared window-content worker, bounded producer queue,
and 128 MiB buffer/texture working-set allowance. Persistent output reservations
are deducted before admitting subsequent jobs, including jobs from another window.
Completed immutable textures transfer to a collision-checked structural output
cache; source/request descriptions are cached separately from native pipelines.
Standalone texture reservations round heap estimates to whole allocation pages
and verify the actual allocation before publication. Compositor command completion
retains the full image ownership/byte reservation, including after renderer teardown
or cache eviction. The budget covers portable content resources; existing window
render targets, compositor pools, and legacy producers remain separately owned.

Only completed producer generations are eligible for composition. Pending work
never adds an unsatisfied GPU dependency to a UI command, and UI draw never waits
for a worker, producer completion, staging readback, or producer queue capacity.
The ordinary window command can still use its existing transaction-bound
`waitUntilScheduled`/present path. A weak, coalesced completion subscription
dispatches to the main thread and requests presentation of the current scene;
a canceled generation cannot replace its successor or retain a dead window.

Default policy retains a slot's previous valid image while pending or after a
reported error. Anonymous/first draws show transparent content. Callers can select
transparent pending/error output or strict frame failure. Errors are reported once
per immutable request generation. Removal retires slot history and unused caches;
returning a removed slot starts without a previous image. Changing color mode
revalidates eligibility and never displays an incompatible retained wide image.
This checkpoint implements stretch of the full normalized source with linear-clamp
sampling, including natural source extents independent of destination size.
Rounded corners, path/border coverage, scene order, clip, and element opacity use
the host compositor. Portable descriptors explicitly control opaque/straight/
premultiplied alpha, coverage masks, and encoded/linear sRGB/P3/Rec.2020 sampling.
P3/Rec.2020 roots require WideGamutSdr presentation rather than silently clipping
to an ordinary sRGB window. Advanced fit/cover/crop mapping, placeholder content,
per-slot availability subscriptions, cross-window structural image deduplication,
priority/fairness scheduling, and device-loss reconstruction remain follow-up work.
The current scene's portable roots execute as one batch; a batch validation or
execution failure applies the selected policies to its draws. When the shared
budget/request queue rejects a generation, presentation follows its
error policy; retry requires a new immutable request (or changed presentation mode).

Run the native compositor acceptance suite on macOS:

```sh
MTL_DEBUG_LAYER=1 cargo test --locked \
  --manifest-path gpui/crates/gpui-apple/Cargo.toml \
  --test portable_content_presentation --features runtime_shaders,test-support \
  -- --test-threads=1 --nocapture
```

What to look for: **12 passed, 0 failed, 1 ignored**. The parent launches the ignored
dense helper in a subprocess with a 45-second deadline. Dense progress reports
verified final pixels, `producer_readbacks=0`, and `max_in_flight <= 2` through three
512 → 1 → 512 cycles with replay. Only the final compositor validation target is
read back. Additional tests cover deterministic pending/replacement/cancellation,
slot removal, transparent/strict failure policy, equivalent reconstructed sources,
compute-to-compositor flow, source-sized odd extents, color/alpha, shared-device
admission, and source leases surviving renderer teardown before compositor commit.

The interactive app uses only core content APIs:

```sh
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin portable-content --features runtime-shaders
```

What to look for: 512 rounded warm cells appear without another input event.
Press **R** (replace warm/cool), **D** (512 ↔ 1), **O** (opacity), and **C** (clip);
use the matching on-screen controls as well. Resize repeatedly and replace during
resize: the window stays responsive, completion appears automatically, the most
recent generation wins, and opacity/clip apply once. Cmd-Q exits. Initial display,
keyboard replacement, dense/sparse switching, opacity, and clipping were checked
in a real macOS window under Metal API validation.

Verification note: the 66 CPU tests, 15 native executor tests, 12 compositor tests,
production runtime-shader build, and interactive app build pass. Clippy passes
with the existing `approx_constant` lint allowance; existing unrelated warnings
remain. The optional standalone bench-support check is blocked by the pre-existing
unresolved `bench_metrics` import in core `app/bench_context.rs`.

Next: migrate existing extension producers onto the portable API, expand the
presentation/mapping surface, and implement the remaining backends against these
same lifecycle contracts. Keep #110 open until the original legacy reproduction
and the migrated application workflows are resolved.

Current explicit program limits: one vec4<f32> color target, no fragment depth
output, read-only render storage, ordinary samplers, non-arrayed single-sample
floating-point 2D images, and top-level scalar/vector/matrix uniform members.
Nested uniform structs/arrays, specialization overrides, comparison/depth samplers,
and additional image/output kinds return errors instead of silent reinterpretation.
These are initial implementation boundaries, not permanent API design exclusions.

## Public types and construction

Core contracts live in `gpui::content` (the library name of the xgpui package).
Backend crates implement private executor traits. Extensions such as shader,
gamut, and media construct portable content instead of importing backend contexts.
Shader parsing/translation can stay in a supporting library, with typed validated
program handles crossing into core; core must not depend on an extension crate.

```rust
pub trait ContentSource: std::fmt::Debug + Send + Sync + 'static {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> Result<ImageRef, ContentError>;
}

#[derive(Clone)]
pub struct Content { /* immutable identity + retained source */ }

impl Content {
    pub fn new(source: impl ContentSource) -> Self;
    pub fn solid(color: ColorValue) -> Self;
    pub fn labeled(self, label: impl Into<Arc<str>>) -> Self;
    pub fn image(image: ImageAsset) -> Self;
    pub fn external(image: ExternalImage) -> Self;
}

pub struct ContentRequest {
    /* logical size, exact raster extent, scale, requested image interpretation */
}
impl ContentRequest {
    pub fn logical_size(&self) -> Size<Pixels>;
    pub fn extent(&self) -> ImageExtent;
    pub fn scale_factor(&self) -> f32;
}

#[derive(Clone, Copy)]
pub struct ImageRef { /* builder identity + node + output index */ }

#[derive(Clone, Copy)]
pub struct BufferRef { /* builder identity + node + output index */ }
```

`Content::new` assigns an identity. Clones share identity; a changed source creates
a new `Content`. Sources must not vary their description through interior mutable
state, clocks, or hidden globals. Animation creates new parameter snapshots; video
uses a new frame identity. Ready/pending transitions of explicitly referenced
assets are tracked dependencies and do not require a new wrapper identity.

Instance identity is distinct from realization equivalence. Built-in pure values
(`solid`, portable programs with uniform snapshots, and pure graph operations)
derive structural realization keys from their complete validated descriptors and
dependencies. Reconstructing equal values inline during `render()` therefore
reuses compatible realizations without storing a handle per cell in view state.
Image/external wrappers use the underlying asset/frame identity. Generic sources
have instance identity by default, but equal pure nodes emitted by separate
descriptions can still share realizations; describing those sources may repeat.
Hashes accelerate lookup; equality checks resolve collisions. Keys include type,
program and schema identity, canonical validated parameters, geometry actually
used, and dependency versions. No unchecked public numeric key can assert that
different content is interchangeable. Labels never affect equivalence.

`describe` is object-safe. Its error type does not contain native handles. Sources
can share CPU data, but must not wait for workers or invoke native GPU APIs here.
Callbacks are trusted SDK extension code: an arbitrary blocking callback cannot
be forcibly interrupted by a UI-thread timer. Node and byte accounting occurs as
the builder accepts operations; recursion and graph-size limits are configurable
resource policies, reported as errors, not unexplained draw-count limits.

`ImageRef` and `BufferRef` are meaningful only in their builder. Cross-builder/stale references are
rejected even if their numeric node IDs match. It cannot be stored as a durable
GPU cache entry. The builder owns all accepted dependencies and payloads before
`describe` returns. `ContentBuilder` has no public constructor; xgpui owns its
lifetime. `ContentRequest` describes source geometry before clip/opacity; extent
is exact and must not be reconstructed from logical size and scale.

## Operations and resource descriptors

```rust
impl ContentBuilder {
    pub fn source(&mut self, content: &Content, request: &ContentRequest)
        -> Result<ImageRef, ContentError>;
    pub fn solid(&mut self, extent: ImageExtent, color: ColorValue)
        -> Result<ImageRef, ContentError>;
    pub fn image(&mut self, asset: &ImageAsset) -> Result<ImageRef, ContentError>;
    pub fn external(&mut self, image: &ExternalImage) -> Result<ImageRef, ContentError>;
    pub fn convert(&mut self, image: ImageRef, to: ImageInterpretation,
        policy: ColorConversion) -> Result<ImageRef, ContentError>;
    pub fn render(&mut self, pass: RenderPass) -> Result<ImageRef, ContentError>;
    pub fn copy_image(&mut self, source: ImageRef) -> Result<ImageRef, ContentError>;
    pub fn compute(&mut self, pass: ComputePass) -> Result<ComputeOutputs, ContentError>;
    pub fn upload_buffer(&mut self, descriptor: BufferDescriptor, bytes: Arc<[u8]>)
        -> Result<BufferRef, ContentError>;
    pub fn zeroed_buffer(&mut self, descriptor: BufferDescriptor)
        -> Result<BufferRef, ContentError>;
    pub fn copy_buffer(&mut self, source: BufferSlice, destination: BufferDescriptor)
        -> Result<BufferRef, ContentError>;
    pub fn capabilities(&self) -> &ContentCapabilities;
}

pub struct ImageDescriptor {
    pub extent: Size<DevicePixels>,
    pub format: PixelFormat,
    pub interpretation: ImageInterpretation,
    pub mip_levels: u32,
}
pub struct ImageInterpretation {
    pub color: ColorEncoding,
    pub alpha: AlphaMode,
}
pub enum AlphaMode { Opaque, Straight, Premultiplied }

pub struct BufferDescriptor {
    pub byte_len: u64,
    pub layout: BufferLayout, // validated element type, alignment and stride
    pub usage: BufferUsage,  // vertex, index, uniform, storage, copy source/destination
}
pub struct BufferSlice {
    pub buffer: BufferRef,
    pub offset: u64,
    pub byte_len: u64,
}
impl RenderPass {
    pub fn vertex_buffer(self, slot: u32, slice: BufferSlice, layout: VertexLayout) -> Self;
    pub fn index_buffer(self, slice: BufferSlice, format: IndexFormat) -> Self;
    pub fn storage_buffer(self, name: &str, slice: BufferSlice) -> Self;
}
impl ComputePass {
    pub fn storage_buffer(self, name: &str, slice: BufferSlice) -> Self;
    pub fn write_buffer(self, name: &str, descriptor: BufferDescriptor) -> Self;
    pub fn read_modify_write_buffer(self, name: &str, descriptor: BufferDescriptor,
        initial: BufferSlice) -> Self;
}
impl ComputeOutputs {
    pub fn buffer(&self, name: &str) -> Result<BufferRef, ContentError>;
    pub fn image(&self, name: &str) -> Result<ImageRef, ContentError>;
}
```

Uploaded CPU bytes are retained immutable payloads. Zeroed buffers have defined
contents; there is no public uninitialized readable buffer. Compute write bindings
produce fresh logical versions retrieved from `ComputeOutputs`; read-modify-write
uses a declared input and distinct output. Reflection and validation check ranges,
alignment, element layout, usage, access, and initialized output ranges. Render
storage inputs are read-only in the initial contract. Compute-generated vertex
and index buffers flow directly into render bindings with dependency edges.

### Source extent and destination mapping

The root `ImageRef` may have a different extent from `request.extent()`. Images,
external frames, and source-resolution filters retain their natural dimensions.
The request extent is the destination rasterization requirement, not an implicit
resize operation. A materialized pass has the extent in its target descriptor.

Draw records carry an `ImageMapping` independent of source descriptors. Default
mapping stretches the full normalized source rectangle [0,1]² over the full
unclipped destination bounds, with linear-clamp sampling. Explicit fit/cover,
alignment, source rectangle, and sampler options determine alternative mappings;
mapping is resolved before clipping. Source origin is top-left with +y down;
sampling uses pixel centers. Coverage masks are destination-space resources at
the exact requested raster extent. Borders and paths can mask source-sized images
without allocating a destination-sized source copy. Clip and element opacity are
applied once after source sampling/conversion. Mapping/coverage are draw cache
inputs; they do not invalidate an unchanged source image realization.

Descriptors distinguish storage layout from color meaning. Initial portable
storage includes RGBA8, BGRA8, RGBA16Float, and single-channel masks. Color encoding
records primaries, transfer function, reference white, and range. HDR does not
follow automatically from float storage. Initial standard encodings include
encoded sRGB, linear sRGB, linear P3, and linear Rec.2020 SDR. YUV imports carry
plane layout, matrix, range, chroma siting, and transfer metadata separately;
sampling yields the declared RGB interpretation through a validated conversion.

`ColorValue` constructors validate finite values and declare interpretation and
alpha. Premultiplication occurs in the declared domain; the compositor converts
to its working representation before blending. Conversions and gamut/tone mapping
policies are explicit; unsupported wide presentation never silently becomes an
sRGB reinterpretation. Window presentation negotiation remains a separate concern.

`RenderPass` contains a validated portable program, named typed bindings, uniform
snapshot, target descriptor, load/clear operation, and geometry. The minimum
surface supports fullscreen rendering and indexed/nonindexed meshes with typed
vertex layouts. Bindings declare sampling and access. Samplers declare filtering,
addressing, and mip use; absent/incompatible mips are a validation error unless
generation is explicitly requested. Multiple render targets are capability-gated.

`ComputePass` declares the program, dispatch dimensions, typed input/output images
and buffers, and access modes. Storage formats, workgroup limits, and compute
availability are validated against capabilities. `ComputeOutputs` names typed
image/buffer outputs. Buffer creation, upload, and copy use the same graph ownership
and admission rules as images; they are not raw backend buffer handles. This is
an extensible operation model, not a promise that every platform supports compute.

Operations produce new logical versions. They cannot mutate a submitted immutable
image, sample their own output, or introduce an undeclared alias. An optional load
input is a dependency on a previous version; the executor decides whether physical
storage can be reused. `source` expands dependencies with identity-based cycle
detection. Clipping and element opacity remain compositor operations applied once.

Builder methods describe intent, not GPU encoding. All payloads are owned or
retained; no pass captures a borrowed encoder or arbitrary execution closure.
Shader bindings and access declarations are checked against validated reflection.
Invalid builder operations poison the current description attempt: catching their
error in a producer cannot publish a partially invalid graph. Capability probing
is side-effect-free and does not poison the builder; portable alternatives must
be selected before attempting an unsupported operation.

`PortableProgram` is an immutable, reflected program artifact compiled from the
SDK's validated WGSL contract. Program construction and translation can run off
the UI thread; device-specific pipeline creation belongs to executor admission.
Each binding includes resource type, access, sampler constraints, and image
interpretation. Uniform values use reflected packing, not unchecked backend ABI
bytes. Shader code cannot read an undeclared texture or silently change the
declared output color/alpha contract. Extension convenience programs and the
general render/compute surface share this program representation.

## Four worked cases

### Solid wide-color fill

```rust
#[derive(Debug)]
struct Accent { color: ColorValue }

impl ContentSource for Accent {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest)
        -> Result<ImageRef, ContentError>
    {
        cx.solid(request.extent(), self.color.clone())
    }
}

let accent = Content::new(Accent {
    color: ColorValue::linear_p3([0.9, 0.2, 0.1, 1.0])?,
});
painted().bg_paint(accent)
```

The renderer may draw the solid directly. If another operation samples it, the
same logical value can be materialized. Application semantics and caching do not
depend on that choice. A dense scene must not allocate one native buffer per fill.

### Multipass shader

```rust
#[derive(Debug)]
struct Blur {
    input: Content,
    horizontal: PortableProgram,
    vertical: PortableProgram,
}
impl ContentSource for Blur {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest)
        -> Result<ImageRef, ContentError>
    {
        let input = cx.source(&self.input, request)?;
        let horizontal = cx.render(
            RenderPass::fullscreen(&self.horizontal,
                ImageDescriptor::linear_p3_f16(request.extent(), AlphaMode::Premultiplied)?)
                .image("source", input)
                .sampler("source_sampler", Sampler::linear_clamp())
        )?;
        cx.render(
            RenderPass::fullscreen(&self.vertical,
                ImageDescriptor::linear_p3_f16(request.extent(), AlphaMode::Premultiplied)?)
                .image("source", horizontal)
                .sampler("source_sampler", Sampler::linear_clamp())
        )
    }
}
```

Both programs declare matching input/output interpretations. Uniform setters,
omitted here, create immutable reflected values. xgpui orders input → horizontal
→ vertical → composition, retains inputs through reads, and can recycle the
horizontal intermediate after its last use. It may split submission between
passes. Stencil, blend, and load behavior must be explicit in general render
passes; the fullscreen helper chooses overwrite semantics.

### Background image upload

```rust
let asset = image_loader.load(path, ImageLoadOptions::default())?;
let content = Content::image(asset.clone());
painted().bg_paint(content)
```

`ImageAsset` is an immutable request identity with a readiness signal; loading
options include decoding/color policy and source-memory budget. Workers decode,
build mip payloads, and fill admitted staging allocations. A backend may allocate
worker-safe resources through a broker, but workers cannot hold unsubmitted host
queue commands. Adoption and GPU copies are scheduled by the executor.

Until admitted/ready, the asset reports `Pending`. A draw can explicitly select a
placeholder or previous valid content through `ContentPresentation` below. Ready
signals trigger redraw only for current subscribers. A canceled or superseded
request releases CPU/staging reservations once no worker or submission owns them.
Staging bytes cannot be refilled merely because copies were submitted; reuse waits
for safe completion. A single source image stays source-sized under scaling.

### Imported video frame

```rust
// A decoder/platform adapter owns native import and synchronization details.
let frame: ExternalImage = decoder.next_frame().await?;
let content = Content::external(frame);
painted().bg_paint(content)
```

`ExternalImage` holds a validated descriptor, immutable frame identity, native
backing owner, acquire dependency, and release obligation. Import adapters live
at the integration boundary (`CoreVideo`, DXGI, DMA-BUF); they return the same
portable handle. Acquisition may be GPU-ordered or asynchronously CPU-observed,
depending on backend capability. Busy acquisition is pending, never an unbounded
UI wait. Unsupported synchronization returns `UnsupportedCapability`.

No consumer may sample before acquisition. Every submitted use retains the
ownership lease. Release occurs only after all relevant uses finish and the
snapshot is retired, using the adapter's defined protocol (completion/fence/keyed
handoff). GPU object retention alone is insufficient for decoder pool ownership.
A cached snapshot intentionally keeps its lease; ephemeral video defaults to no
result-cache retention so retired frames return to their producer promptly.

## Description, realization, and presentation lifecycles

These are distinct state machines; submitted does not mean GPU-completed.

| Object | States and transitions |
| --- | --- |
| Graph attempt | Describing → Validated → Planned; any pre-execution error → Rejected |
| Image realization | Planned → Admitted → Encoding → Submitted → Completed; errors → Failed; unsubmitted cancellation → Abandoned |
| Asset | Pending → Available or Failed; cancellation is subscription/request ownership, not mutation of ready pixel data |
| Frame | Requested → Preparing → ReadyToPresent → Presented; stale generation → Superseded; errors → Failed |

An image is publishable to ordered consumers after successful encoding and
submission with its dependencies. This is an internally tracked submission
dependency, not a public "ready texture" assertion. GPU completion permits pool
reuse and retirement, not initial same-queue sampling. A subsequent GPU failure
invalidates dependent realizations even if they were previously published as
submitted. No failed realization can be served from cache as successful content.

Execution proceeds as follows:

Pending assets or external acquisition suspend the affected planned nodes before
admission. They do not occupy command-buffer slots. Wakeup retries use generation
checks; terminal failures invalidate dependent nodes, while temporary admission
pressure preserves the validated graph without marking it failed.

1. Snapshot scene and presentation configuration; assign a frame generation.
2. Expand content descriptions, deduplicate identities/requests, validate the
   graph, program interfaces, capabilities, and color contracts before recording.
3. Plan topological execution and resource lifetimes. Prove that required live
   resources can fit or return a resource error; don't allocate the whole scene
   to discover pressure. Account for payloads, intermediates, staging, retained
   cache entries, and already in-flight uses.
4. Admit a bounded ready subset. Allocate/encode only that subset, retaining all
   referenced owners. Flush work before requesting additional command capacity.
5. Submit in dependency order; track completion and reclaim eligible resources.
   If temporarily full, suspend/reschedule execution rather than waiting in the
   UI callback. Each renderer has one logical serial execution owner. Native
   preparation can use a renderer worker; presentation follows platform affinity.
   Browser execution uses its owning event-loop thread and async continuation.
6. Compose only when the root's dependencies are ordered and valid. Publish the
   presentation only for the current frame generation. A superseded frame may
   complete GPU work but must not overwrite newer navigation or resize results.
7. Observe completion/errors, release leases, invalidate failures, and notify
   relevant subscribers. Notifications carry frame/content identity and are
   delivered outside cache/admission locks.

Admission reserves complete minimum working sets or makes no reservation. It must
not hold partial staging/intermediate reservations while waiting for another
reservation that depends on its own release. Jobs have bounded chunks and fair
admission; newer frames can supersede queued work without starving active uploads.
Already submitted work cannot be canceled. Any driver-required blocking allocation
is isolated from UI execution; arbitrary driver hangs still require process-level
regression timeouts and cannot be converted into ordinary recoverable errors.

### Platform presentation and synchronous resize

Serial ownership does not require every operation to run on one worker thread.
Ownership handoff between worker recording and platform presentation is explicit;
the two never concurrently mutate a recorder or immediate context. Inline work
is permitted only with resources already admitted and recording capacity reserved
without a dependency on future UI progress. A small operation count alone does
not establish that native allocation is safe; potentially blocking preparation
allocation stays off the UI thread.

On macOS transactional resize, the final composition command must be committed,
scheduled, and presented on the platform-required thread before `draw()` returns,
preserving the existing `commit → waitUntilScheduled → present` ordering. This
platform scheduling wait is an explicit exception to avoiding UI waits; it is
not a GPU-completion wait or permission to wait for deferred producers. Compose
from ordered current results and ready pending-policy fallbacks, with compositor
resources reserved before entering that path. A fallback requiring new work is
not immediately ready: use an already ready previous/clear result instead.
If composition cannot safely be admitted, preserve the previous presentation and
request another draw; do not enqueue an unsatisfied dependency behind the UI wait.
An asynchronous producer never moves a transaction-bound present to a worker or
presents its superseded drawable after the transaction has ended.

### Presentation and readiness policy

```rust
pub enum ContentAvailability { Pending, Submitted, Completed, Failed(ContentError) }
pub enum PendingPresentation {
    RetainPrevious,  // same presentation slot; no previous result means transparent
    Placeholder(Content),
    Transparent,
}
pub struct ContentPresentation {
    pub slot: Option<PresentationSlotId>,
    pub pending: PendingPresentation,
    pub on_error: ContentErrorPresentation,
}
pub enum ContentErrorPresentation {
    ReportAndRetainPrevious,
    ReportAndPlaceholder(Content),
    FailFrame,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresentationSlotId { /* window-scoped stable logical paint identity */ }
```

Window default is retain the last valid presentation and report errors. Initial
windows have a clear background. Scene draw records carry the presentation slot,
mapping, and policies explicitly. Identified elements derive a slot from their
stable element identity plus a semantic paint role (fill, border, named effect).
Multiple canvas draws use explicit stable slot IDs scoped to the window/element;
neither traversal order nor a content/native address supplies identity. Replacing
content in the same slot can retain its previous valid output. Anonymous draws
without a slot cannot retain per-draw history and resolve that policy to transparent.
Duplicate slot use in one scene is rejected; replay preserves IDs, while removal
retires slot history and a new element incarnation cannot inherit it. Removed elements lose
their subscriptions; fallback content cannot recursively refer to its own slot.
Fallbacks must independently validate under the active capability/color policy.
Retaining a previous result is explicit visible staleness, not proof of success;
applications can choose placeholders to avoid showing stale content.

Headless validation defaults to strict mode: await the requested generation or
return an error, with an explicit caller deadline. It never substitutes an older
frame for a successful readback. Async completion APIs are primary; a blocking
convenience entry point is restricted to callers outside renderer/UI execution.

## Caching and replay

Description cache identity includes content instance identity and complete request.
Realization cache identity uses structural equivalence where defined, relevant
request geometry, output/color policy, dependency versions, and internal device
generation. Source-sized image realizations omit destination-only geometry.
Program/pipeline
caches are separate from output caches. Stable labels are diagnostics only.

Cache policy can be automatic, transient, or retained within an explicit budget;
retained does not imply unlimited memory. Device-specific realizations never leak
into application handles. Eviction releases cache ownership only; submissions
retain outstanding reads. Device loss clears realizations and rebuilds from CPU
descriptions where possible; external imports need revalidation/reimport and may
require a new frame from their producer.

Replay records references to immutable descriptions, not one-shot native commands.
Duplicate draws share a compatible realization and still apply independent scene
geometry, masks, and opacity. Failed/abandoned realizations are regenerated; a
successful output is never regenerated solely because an unrelated later draw
failed. Changing parameters creates a new snapshot; dependency changes invalidate
only the affected subtree. Physical aliasing is internal and never spans live uses.

## Errors and partial execution

`ContentError` has structured kinds: invalid graph/reference, cycle, invalid
descriptor/binding/color contract, unsupported capability, resource limit,
allocation failure, import/synchronization failure, shader failure, device loss,
and execution failure. Pending is not an error. Errors retain content label/ID,
operation ID, frame generation, and a backend diagnostic cause where available.

Before submission, rejected/abandoned nodes release reservations immediately once
CPU/worker use ends. After submission, completion tracking owns resources even if
the frame, scene, producer, or window is dropped. A later failure does not roll
back earlier commands. Root output stays unpublished if required dependencies
fail; independent completed content can remain cached. Device-loss retirement
uses backend teardown rules when normal completion can no longer arrive.

GPU work never captures a strong ownership cycle between its completion callback,
recording owner, and cache. Renderer teardown transfers in-flight retirement to a
device-owned completion service. CPU-only Drop on the UI thread neither waits for
GPU completion nor prematurely returns an external pool slot.

## Backend execution contract

| Backend | Recording and submission | Ownership / synchronization |
| --- | --- | --- |
| Metal | Host allocates command buffers during admitted execution; groups compatible operations and commits before allocating beyond its recording allowance | Resources and external leases retained through completion; framework events/fences express required cross-queue edges |
| D3D11 | Host records deferred contexts or immediate work under serial executor ownership; executes bounded command-list groups | Explicit submission/retirement tracking; adapter-specific keyed ownership protocols; no unbounded UI acquisition |
| wgpu | Host creates encoders during admitted execution; finishes/submits bounded groups | Queue ordering plus supported synchronization; completion callbacks/polling owned by executor; platform imports capability-gated |

Backend limits are implementation inputs, not public command counts. wgpu native
and browser variants share the contract, with different execution affinity and
capability sets. Compute or wide presentation may be unsupported on a particular
target. Baseline portable render/copy operations must have equivalent semantics;
optional functionality must never silently degrade.

Direct drawing is an optimization only when observable pixels, sampling,
derivatives, geometry, blending, masks, color conversion, and ordering are
preserved. Sampling or readback can force materialization. Native backgrounds,
coverage, backdrops, and layer effects participate in the same scheduler/resource
accounting instead of creating hidden allocation/submission paths.

Initial direct lowering is allowed for a solid or supported fullscreen render
output at a compositor use only when its inputs are available and all of the
equivalence conditions above hold, including the logical source-resolution and
derivative domains. Unsupported blend/load behavior or a required persistent
image result forces materialization. A sampled consumer requires materialized
storage for that consumer, but does not categorically prohibit a separate direct
compositor use of the same value. The planner accounts for both uses and their
lifetimes; graph out-degree alone is not a correctness criterion.

Internal validation options include `ExecutionPolicy::ForceMaterialize`. It
disables direct lowering for selected content roots, forcing ordinary offscreen
execution and admission; native UI/background primitives need not be rewritten.
Tests assert materialization counters and pixel parity, and include explicit
multipass graphs, so an optimization cannot make a stress regression vacuous.

## Advanced native boundary

Portable content never receives `Any`, a native device, or a queue. Platform
adapters may register a `NativeContentProvider` with a private backend executor
interface. A provider declares resource reads/writes, image descriptors,
dependencies, native owner leases, and required capabilities before execution.
Encoding receives host-managed recording access only during its admitted scope;
the provider cannot retain the recorder or submit it independently.

An external engine that owns its queue publishes an immutable external image
plus explicit acquire/release synchronization; it does not hand xgpui an arbitrary
uncommitted buffer. Unsafe imports document device compatibility, ownership
transfer, and aliasing obligations. Unsupported imports fail early. Platform code
belongs in the adapter/provider, not repeated throughout application paint calls.

## Diagnostics and validation gates

Expose structured events for graph validation, admission/defer decisions, memory
usage, operation/chunk encoding, submission, completion, supersession, and errors.
Include stable content/frame/node identities, labels, timings, resource bytes,
pending/in-flight counts, and pressure reason. Distinguish CPU description,
encoding, submission, and GPU completion durations. Backend-specific details may
appear in diagnostics without entering ordinary API control flow.

Required acceptance scenarios:

| Scenario | Evidence required |
| --- | --- |
| Dense scenes on Metal/D3D11/wgpu | Subprocess timeout detects old Metal stall; new execution finishes with correct pixels or a typed resource error |
| Deliberately small recording allowance | Submission advances before further allocation; no fixed hardware queue-capacity assumption |
| Multipass dependency and shared input | Correct pixels across chunk boundaries, exactly one shared realization, no premature intermediate reuse |
| Cached replay and parameter changes | No recommit of one-shot commands; unchanged results reused; changed dependency rebuilds correct subtree |
| Failure after earlier submission | Root invalidation, independent cache validity, leases retained until safe retirement |
| Worker staging and cancellation | Bounded memory, no pending host-queue commands held by workers, safe refill only after completion |
| Navigation/resize and repeated frames | Superseded generations cannot present late; old owners retire correctly |
| External acquire/release | No sampling before acquire, no reuse before release, pending acquisition keeps UI responsive |
| Device loss and renderer teardown | Structured failure, no stale realization reuse, no UI-thread wait or leaked ownership cycle |
| Color/alpha and direct/materialized parity | Linear-P3/sRGB, alpha, masks, and opacity produce defined results on each supported path |
| Invalid graph/capability | Rejection before execution for cycles, foreign handles, aliases, reflection mismatches, unsupported features |
| Inline value construction | Equal rebuilt descriptors reuse realizations; unequal descriptors and hash collisions never alias |
| Source-sized masked content | Correct fit/crop/filtering and destination-space border/path coverage under scaling |
| Compute-to-mesh | Typed buffer dependencies, range/alignment validation, correct geometry after chunk boundaries |
| Stable presentation slots | Content swaps retain the intended previous result; anonymous/removed/duplicate slots obey defined policy |
| Transactional resize | Platform-thread present ordering preserved, deferred producer cannot block transaction progress |

Existing regression #111 intentionally remains a baseline of the old API until
the migration step. A migrated equivalent must exercise independent producers
and multipass materialization, so direct-solid optimization cannot hide a broken
allocation/submission contract. Injected recording limits and fake completion
services provide deterministic scheduler tests; native tests retain process-level
timeouts for driver/queue stalls.

## Implementation sequence and review boundary

1. Review this contract, especially immutable identity, pending/error presentation,
   external lease retirement, optional compute, and loss of batch rollback.
2. Define Rust modules/types, portable program validation and descriptors; add a
   CPU graph validator and deterministic admission/lifetime tests.
3. Add executor infrastructure and migrate Metal with bounded execution. Preserve
   the old regression as evidence; add the new-API stress/materialization cases.
4. Implement D3D11/wgpu executors and run their equivalent native stress cases.
5. Migrate gamut/shader/media and native import adapters, coverage/layers/backdrops,
   then applications and labs. Remove `Any` preparation/output and producer queue
   caches rather than carrying a second ordinary producer protocol indefinitely.
6. Publish API docs and backend capability reports only after validation. Update
   issue #110 with measured coverage and remaining limitations.

Backend implementation follows the CPU foundation checkpoint above. Exact default
budgets, initial optional capability coverage, and internal worker integration
are implementation measurements to record in follow-up work; public semantics
above must not change merely to match one backend's convenient execution model.

### First legacy producer migration: WideColor

`WideColor` is now a pure ContentSource with `content()`/`From<WideColor> for Content`.
It validates straight input before association, describes a premultiplied linear-P3
FP16 solid, and preserves source identity across clones. The Metal/D3D11 preparation
implementations and normal backend dependencies have been removed. The shader
extension's Paint::Content arm accepts Content or ContentDraw and records portable
fill, border, and path draws through Window::paint_content. The gamut lab and native
comparison tests now use that path. Instanced wide fills/gradients remain separate.

The original issue-110 app now awaits the requested producer generation outside
UI drawing, checks exact FP16 red/green pixels and cached replay, and enforces zero
producer readback. In addition to the original equal-color scene it runs distinct
natural extents to force hundreds of separate operations despite solid deduplication.
The parent bounds child lifetime and treats typed execution errors as failures.
Both control and dense cases must finish with PASS. See its README for commands.

Strict real-window snapshots first prepare/promote portable content and return a
typed Pending error until ready, before compositor readback. They propagate producer
errors instead of accepting the previous/transparent window fallback as a successful
snapshot. The gamut lab retries after yielding, with a deadline. This does not add
blocking producer waits to UI drawing. Native capture verified the migrated surfaces
and sRGB overlays; the portable compositor suite includes a deterministic Pending
snapshot test.

Portable WideColor now requires a backend with portable execution (currently Metal).
Windows users can use the separate instanced painter while the D3D11 portable
executor is implemented. Other legacy producer families remain migration work;
issue #110 stays open pending that work and user review.

### SolidColor migration checkpoint

`xgpui-shader::SolidColor` now follows the same CPU-only source model as WideColor.
Its input/output contract stays straight-alpha encoded-sRGB, with RGBA8 storage.
`content()`, `From<SolidColor> for Content`, and `From<SolidColor> for Paint` retain
immutable identity. It is available on all targets without a wgpu feature. The
Metal preparation/cache, D3D11/wgpu clear adapters, allocation counters and cached
texture inspection have been removed from this producer. Other producer families
continue to use their existing adapters until migrated separately.

`PlatformHeadlessRenderer::prepare_content_snapshot` is now a backend-neutral,
nonblocking validation entry point. Metal starts/promotes portable work; pending
requests return typed Pending, and unsupported executors explicitly reject portable
records before drawing. Callers yield/retry with a deadline outside UI drawing,
then capture the requested generation. This does not make fallback display pixels
a successful strict snapshot or add a worker wait to the window render path.

The paint lab now validates portable solids through that contract. Its legacy
preparation/uniform probes and shader backdrop/layer tests use explicit native
Background fixtures, preserving their protocol coverage without reintroducing
producer-owned GPU work. The gamut lab's sRGB reference uses retained SolidColor
content and stable semantic slots. The browser demo uses a native solid reference
until its portable executor exists; other browser effects remain unchanged.

Native SolidColor acceptance covers encoded-sRGB alpha/opacity and 8-bit rounding,
rounded corners, clip, scene order, equal-source realization sharing, replay,
resize, zero producer readbacks, and deterministic Pending/UnsupportedCapability
results. Run the command in the shader extension README and expect four tests.
The 125-test shader library suite, 20 paint-lab tests, native paint/gamut captures,
and wasm browser-demo compilation passed during this checkpoint.

Next: migrate analytic gradients, then mesh and experimental shader producers.
Metal is the current portable execution backend; D3D11/wgpu execution and the
remaining presentation features remain follow-up work. Keep issue #110 open.


### Analytic gradient migration checkpoint

`Gradient` is now a CPU-only `ContentSource` with retained `content()` and owned
conversions into `Content`/`Paint`. Clones share identity; kind/stop edits create
new immutable snapshots. A single portable fullscreen WGSL program handles all
four geometries using reflected scalar/vector uniforms and a fixed, initialized
1024-byte readonly storage table for up to 32 stops. Geometry, duplicate-stop
hard edges, endpoint clamping and premultiplied encoded-sRGB interpolation remain
unchanged; output is straight-alpha RGBA8 sRGB.

Producer-owned Metal, D3D11 and wgpu gradient adapters and cache diagnostics have
been removed. Native paint-lab presentation and strict captures now use portable
content. Its browser preview uses an app-local public shader-paint adapter until
portable wgpu execution exists, without exposing native contexts in Gradient.
Unsupported portable executors continue to return typed capability errors.

Native acceptance compares independent pixel references, checks pipeline reuse,
structural stop-buffer/output sharing, immutable edits and resize, and bounds a
512→1→512 stress child process while verifying cached replay, zero producer
readbacks and at most two command buffers in flight. Paint-lab retains geometry,
hard-stop, transparent interpolation and background/border/path coverage checks.
See the extension and paint-lab READMEs for runnable commands and visual checks.

Next: migrate MeshGradient, then experimental shader producers. D3D11/wgpu
portable execution remains follow-up work; keep issue #110 open.


Analytic presentation follow-up: the paint-lab migration initially recorded
anonymous content draws, causing pending parameter updates to become transparent.
Its background, border and path now retain separate presentation slots across
source replacement. Completed snapshots alone did not catch this; a deterministic
native acceptance test pauses production and checks unchanged visible pixels on
pending edit/replay frames for every analytic kind, followed by the new pixels on
completion. Source identity and destination presentation identity remain separate.


Continuous-edit scheduling correction: polling and harvesting completed work must
precede canceling a replaced request. A completed result can become the active
destination slot's retained history even when the next input arrives before its
redraw. Otherwise sustained input discards useful completed frames and visibly
stalls. Results canceled while unfinished remain excluded, removed slots cannot
regain history, and incompatible wide results cannot enter an SDR fallback.
Strict snapshots still return Pending until the requested generation is ready.
A conic pixel test verifies the intermediate completed angle appears during the
next pending angle, then the newest angle appears after completion. A 60-update,
three-extent test verifies every completed generation advances without producer
readback. Existing supersession, failure, slot removal and strict snapshot tests
remain required. This changes host scheduling, not the portable producer API.


### MeshGradient migration checkpoint

MeshGradient now produces a CPU-only portable graph: immutable 48-byte controls,
one guarded WGSL tessellation dispatch expanding 32-byte triangle vertices, then
one portable mesh render pass. The Hermite basis, patch-local subdivisions,
triangle replacement order, folds, gaps, opaque encoded-sRGB RGB and transparent
outside regions remain intact. Output is RGBA8 sRGB/straight alpha. The host shares
geometry across output extents and owns native pipelines, caches, completion and
in-flight retention. Producer preparation/cache counters and Metal/D3D11/wgpu mesh
adapters, including the now-unused built-in Metal output cache, are removed.

MeshGrid retains 2..=16 points per axis and 2..=64 samples per patch edge. Geometry
budgeting counts the actual control and expanded triangle payloads (64 MiB cap);
intermediate coefficient/sample allocations are eliminated. MeshStats still
reports unique sample and triangle counts. Clones retain Content identity; edits
create immutable snapshots. Main paint-lab roles and Hermite comparison retain
presentation slots across parameter changes. The browser demo explicitly labels
its mesh producer preview unavailable until portable wgpu execution is implemented;
no replacement native mesh adapter is kept for compatibility.

Native acceptance compares independent CPU triangle rasterization for regular,
curved, inset and folded grids, and covers pending/replay/edit/resize behavior,
shared geometry/pipelines, 60 completed continuous updates and bounded 512→1→512
workloads with zero producer readbacks. The paint lab retains CPU Hermite samples,
clip/opacity and background/border/path coverage and failed-frame retry checks.
Portable reference comparisons run on Metal; wgpu tests explicitly assert the
missing executor rather than treating fallback pixels as successful output.

Next: migrate experimental ShaderProgram/ShaderPaint producers and their demos,
remove their superseded adapters and aliases, then complete portable execution on
D3D11/wgpu. Backward compatibility is not a constraint on that final API. Keep
issue #110 open until the remaining lifecycle/backend work is complete.


### Procedural ShaderProgram/ShaderPaint migration checkpoint

Procedural shader paints now describe a portable fullscreen pass with reflected
geometry and core UniformValues. ShaderProgram is CPU-only; paint() creates a
validated immutable ContentSource snapshot. Clones keep identity, edits replace
snapshots, and stable presentation slots retain completed images during updates.
User PaintInput logical/raster semantics, output clamping and straight-alpha sRGB
RGBA8 remain intact. Integer/matrix parameters, loops, derivatives and discard
follow shared core packing/WGSL validation rather than a duplicate restricted API.

PortableProgram::uniform_layout also reflects declared unused uniforms, without
making them execution bindings. Parameter validation therefore remains consistent
when a compiler eliminates all reads. Unsupported unused layouts are reported on
explicit reflection, preserving ordinary programs' selected-stage binding rules.

Removed: ShaderParameters, Shader/shader aliases, Paint::Custom/Arc conversions,
all shader-paint Metal/D3D11/wgpu adapters/caches and native production dependencies
from xgpui-shader. Native backgrounds and portable ContentDraw are the extension
paint surface. Core external/video CustomContent hooks remain separate framework
facilities. Main-pass instanced shaders are a current rendering workload used by
brushes/text, not a legacy offscreen producer interface.

The native lab has an interactive User WGSL mode: angle drives progress and the
selected stop drives tint/alpha. The browser producer page explicitly awaits the
portable wgpu executor; native sRGB swatches and direct brush editors remain.
The browser-only gradient shader adapter is removed rather than kept for compatibility.
Native acceptance verifies source pixels, typed parameters, unused bindings,
logical/raster independence, pending frames, pipeline reuse, 60 continuous updates
and bounded 512→1→512 workloads with zero producer readbacks. See the procedural
paint guide for commands and visual checks.

Next: implement portable execution/presentation on wgpu and D3D11, then close the
remaining issue-110 lifecycle/backend acceptance gaps. No superseded shader paint
API or compatibility adapter is part of the final design; keep issue #110 open.


## Implementation: wgpu execution and presentation

The framework now lowers portable graphs on wgpu: initialized solids/uploads,
image and bounded buffer copies (including unaligned byte copies), reflected
render/compute bindings, fullscreen and indexed/instanced meshes, multi-output
compute, load/blend and mip readback. Native jobs run on one serial worker;
browser jobs run asynchronously on the owning event loop and use nonblocking
polls/timers. Error scopes are popped synchronously before any yield, preventing
browser tasks from capturing another job's errors. No GPU handles cross browser
thread-affinity boundaries.

The per-device/queue service admits a bounded submission window (default two),
retains graph and sampling leases through GPU completion, and charges retained
exports against subsequent admission. wgpu hides allocation sizes: padded image
payloads are reserved in 64 KiB granules and buffers in 4 KiB granules. Pipeline
variants, pending requests and validation staging have independent bounds. These
reservations bound host-owned graph resources, not physical driver allocation.

Presentation consumes completed textures directly, schedules redraws on browser
and Linux window loops and retains slot history during continuous updates.
Headless validation reports Pending until its exact generation is complete.
Browser Shader, Color and Hermite producer previews use the same portable APIs.
The browser execution target for storage/compute producers is WebGPU; WebGL2
capability errors remain explicit. The wgpu compositor currently accepts sRGB,
linear-sRGB and coverage; wide P3/Rec.2020 presentation is explicitly unsupported.
Offscreen FP16 execution/readback remains available.

Validation and manual acceptance: [portable wgpu paints](portable-paints.md).
D3D11 execution/presentation is the next backend step; no producer-native or
compatibility APIs were added.

Runtime validation on macOS/wgpu-Metal: 15 graph acceptance tests plus a
512-root subprocess, 26 paint-lab tests and three producer/presentation tests.
The latter cover six color/alpha/coverage interpretations, 60 consecutive slot
updates, completed-before-replacement ordering, strict snapshots, duplicate slots,
zero producer readbacks and resource release after scene removal. Browser debug
build and WebGPU initial Shader/Color/Hermite rendering were checked in in-app
Chromium without warning/error console messages; interactive animation and edits
remain manual acceptance. Linux cross-check stopped in the Wayland C dependency
because this macOS environment lacks a Linux C sysroot; Linux and Windows native
runtime acceptance are not claimed.


### Browser animation admission correction

Continuous browser frames could cancel an unfinished multi-output graph before
its later mesh/WGSL outputs ran. wgpu presentation now lets its one running
graph finish, harvests completed slot history, then admits the latest requested
description. Newer inputs coalesce without adding jobs or restarting the running
graph; scene removal still cancels it. A deterministic gate regression changes
inputs twelve times while execution is paused and requires one unchanged request
count, completion of the admitted pixels, then the latest generation. The browser
animated cards are first in the gallery so their motion is visible beside the
controls.
