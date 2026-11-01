# Portable content API and execution lifecycle

Status: proposed full design, 2026-11-01. The CPU foundation described below is
implemented; backend execution and application integration remain pending.
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

This is CPU-only infrastructure, not a connected renderer or the fix for #110.
It currently materializes every planned node. Shader/render/compute bindings,
asset readiness, external imports, draw mapping/slots, device-wide admission,
platform presentation, native completion services, and backend lowering remain
implementation work. Buffer operations validate and describe data; their GPU
consumers arrive with render/compute integration.

Run the deterministic integration suite from the repository root:

```sh
cargo test --manifest-path gpui/crates/gpui/Cargo.toml \
  --test portable_content --features gpui_platform/runtime_shaders
```

The focused integration target avoids an existing unrelated property-test macro
failure in the library unit-test target. Tests use fake completion notifications
and do not claim native GPU validation. The Metal baseline regression remains
unchanged and is still expected to reproduce the deadlock.

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
    pub fn extent(&self) -> Size<DevicePixels>;
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
    pub fn solid(&mut self, extent: Size<DevicePixels>, color: ColorValue)
        -> Result<ImageRef, ContentError>;
    pub fn image(&mut self, asset: &ImageAsset) -> Result<ImageRef, ContentError>;
    pub fn external(&mut self, image: &ExternalImage) -> Result<ImageRef, ContentError>;
    pub fn convert(&mut self, image: ImageRef, to: ImageInterpretation,
        policy: ColorConversion) -> Result<ImageRef, ContentError>;
    pub fn render(&mut self, pass: RenderPass) -> Result<ImageRef, ContentError>;
    pub fn copy(&mut self, copy: ImageCopy) -> Result<ImageRef, ContentError>;
    pub fn compute(&mut self, pass: ComputePass) -> Result<ComputeOutputs, ContentError>;
    pub fn upload_buffer(&mut self, descriptor: BufferDescriptor, bytes: Arc<[u8]>)
        -> Result<BufferRef, ContentError>;
    pub fn zeroed_buffer(&mut self, descriptor: BufferDescriptor)
        -> Result<BufferRef, ContentError>;
    pub fn copy_buffer(&mut self, copy: BufferCopy) -> Result<BufferRef, ContentError>;
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
    pub fn vertex_buffer(self, slot: u32, slice: BufferSlice) -> Self;
    pub fn index_buffer(self, slice: BufferSlice, format: IndexFormat) -> Self;
    pub fn storage_buffer(self, name: &str, slice: BufferSlice) -> Self;
}
impl ComputePass {
    pub fn read_buffer(self, name: &str, slice: BufferSlice) -> Self;
    pub fn write_buffer(self, name: &str, descriptor: BufferDescriptor) -> Self;
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
            RenderPass::fullscreen(&self.horizontal, request.extent())
                .sampled("source", input, Sampler::linear_clamp())
                .target(ImageDescriptor::linear_p3_f16(request.extent()))
        )?;
        cx.render(
            RenderPass::fullscreen(&self.vertical, request.extent())
                .sampled("source", horizontal, Sampler::linear_clamp())
                .target(ImageDescriptor::linear_p3_f16(request.extent()))
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
