# Paint contract hardening proposal

Status: border fixes, the preparation request/compatibility adapter and Metal
submission validation are implemented. Failure isolation, host-managed recording,
color descriptors and coordinate extensions below remain proposals.

## Constraints and border baseline

Keep `Paint`, `.bg(...)`, `.bg_paint(...)`, `.border_color(...)`,
`.border_paint(...)` and the existing Window paint entry points. Effects,
gradient parameters, animation and producer caches stay in `xgpui-shader`.
GPUI owns shape coverage, scene order, inherited clip/opacity and submission.
Do not add gradient variants to GPUI or change native primitive GPU layouts.

Border setters now select one effective paint: the last setter wins, including
when applying a later hover/theme refinement. `border_color` writes both the
legacy color and an explicit native paint, since clearing an optional refinement
cannot remove an inherited override. This does duplicate the native value in
serialized styles; it adds no fields or enum variants. Raw field construction
still resolves `border_paint` first, then falls back to `border_color`.
Directly changing `border_color` after setting `border_paint` does not clear it;
use setters or explicitly clear the override when mutating raw fields.

A transparent native override hides a border even if the legacy color is opaque.
Custom alpha is unknown during layout, so custom borders count as potentially
visible; clip insets must not fluctuate with animation. Zero widths hide all
border kinds. Painting and overflow use the same visibility rule. Overflow
insets apply only on hidden axes. Solid colors retain the original native draw
path; custom and non-solid native paints use the existing coverage path.

## 1. Preparation: explicit request and validated backend output

The original `CustomContent::prepare(&dyn Any, physical_size)` returns another `Any`.
Metal recognizes either a texture result or a special native Background result.
The caller cannot express target color requirements or logical scale, and queue
ownership depends partly on documentation.

### Implemented first step

`PreparationRequest::new(logical_size, raster_size, scale_factor)` validates
finite positive geometry and exposes these values through accessors. Its fields
are private so later additions do not break struct literals. No color fields.
`CustomContent::prepare_with_request` is a provided method delegating to the
unchanged required `prepare` method with the exact raster size. Existing producer
implementations need no changes. Metal dispatches through the new method;
output types and error propagation are unchanged. Submission checks were
subsequently tightened as described below.

Window background, border and path painting retain their original logical size
and scale in a private producer wrapper. The renderer still chooses raster size
from the existing scaled bounds. This avoids adding fields to public CustomDraw
or changing its literals. The wrapper costs one small Arc allocation per recorded
custom draw and forwards to the original producer, which retains cache ownership.
Replay shares the wrapper. No extra GPU allocation or render pass is introduced.
Directly constructed scene records have a documented 1x default: their scaled
bounds supply logical size, and raster extent is rounded up as before. Supporting
explicit logical geometry on low-level records can be a later additive API if
needed; do not infer the window scale from these records.

Tests cover an unchanged legacy implementation, exact extent/backend forwarding,
unchanged errors, request override dispatch, invalid request geometry, retained
metadata, and all three Window paint sites at 1x/1.25x/1.5x/2x/3x with partial
clipping. A headless Metal test checks real renderer dispatch and unclipped
requests, alongside the existing pixel/coverage tests.

### Implemented submission validation

Metal validates output texture device, format, extent and sampling usage, and
requires every returned preparation command to be NotEnqueued and to belong to
the exact host queue. Before committing producer commands it revalidates the
entire batch, then deduplicates command buffers by retained object identity and
commits each once. Invalid batches submit none of their producer commands; native
coverage work may already have been submitted by the existing mask path.

Existing producer methods and MetalCustomOutput fields are unchanged. Empty
command lists remain supported for cached textures: the producer must ensure
the output is already ready or preparation was submitted earlier on the same
queue. There is no CPU wait for preparation completion. With raw handles this
remains a cooperative contract; the host cannot prove that an empty list refers
to initialized output, or prevent a producer from concurrently submitting work.

GPU tests cover valid and foreign queues, enqueued/completed commands, state
changes after preparation, all-or-nothing producer batch validation, shared-buffer
deduplication, invalid extent/format, and foreign-device rejection when a second
GPU is available. A shared-event-gated renderer test holds preparation pending
across two frames to verify cached reuse without a CPU wait, then checks pixels.
Existing paint-lab tests cover unchanged solid, mesh and gradient producers.
Failure still propagates to the frame; no paint skipping, fallback or diagnostic
suppression is introduced in this step.

### Remaining proposal

Proposed boundary:

* A small, non-exhaustive preparation request in GPUI carries logical source
  size, exact raster extent, scale factor and destination color requirements.
  Accessors/constructors allow additions without breaking struct literals.
* A backend context remains backend-owned. Downcasting happens once in the
  adapter, with an explicit unsupported-backend error. GPUI must not depend on
  Metal types. Avoid a second public hierarchy merely to disguise type erasure.
* Metal exposes a validated output constructor with private fields: texture,
  color/alpha descriptor and pending work. Validate device, queue identity,
  extent, format, sampleability and submission state before composition.
  The internal native-background adapter takes an explicit host branch instead
  of reserving an undocumented producer result type.
* The host owns command creation/submission through a recording context. It
  submits each recording once before sampling and retains resources until GPU
  completion. A cached output refers to a completed or host-tracked submitted
  resource; an empty command list alone does not establish readiness.
* Retained producers remain immutable snapshots. Prepared resources are scoped
  to device/queue, request and snapshot identity. Clip, opacity and shape coverage
  do not invalidate the source texture. Budget and recycle output resources
  independently of the coverage cache.

The first migration step is implemented: a provided `prepare_with_request` method
defaults to existing `prepare`, and the renderer calls it. Existing
implementations keep compiling. The legacy adapter retains today's size and
implicit color contract, with no color descriptor fields yet. A v2 producer can override the new
method; the existing required method can return a descriptive error when used
by an old renderer. Do not add another required trait method. Typed Metal output
construction and removal of raw queue access are backend API changes: introduce
the replacement, migrate `xgpui-shader` and labs, then deprecate the old adapter.
Removing old fields/methods remains a separately reviewed breaking change.

Raw Metal handles can always escape a cooperative API. Constructors and host
recording eliminate accidental misuse, not malicious producer behavior. Queue
validation is still required for the compatibility adapter.

Failure policy recommendation: validate before submitting producer work. An
unsupported or invalid custom paint should skip that paint, retain its layout,
and report a bounded diagnostic while allowing unrelated content to draw.
Use an explicit native fallback only when the caller supplies one; do not silently
reuse stale animation output. Device loss remains a frame/platform failure.
This changes current whole-frame error propagation and needs dedicated tests.

## 2. Color: separate interpolation, storage and destination

Today custom output means straight-alpha encoded-sRGB channels in BGRA8Unorm.
That is a compatibility contract, not a high-gamut contract. Format alone cannot
describe primaries, transfer function or alpha association. Straight-alpha
bilinear sampling can introduce color fringes around transparent pixels.

Proposed descriptor records storage format, color encoding/primaries and alpha
association. The renderer advertises supported combinations and the destination
requirements in the preparation request. Unsupported combinations fail
explicitly; no inference from a texture's pixel format.

Recommended new path: filter premultiplied linear-light texels, convert primaries
in linear light, then encode/blend according to the actual render target.
Producer interpolation is a separate artistic choice in `xgpui-shader` and must
remain explicit; selecting a linear output encoding must not silently redefine
gradient stop interpolation. Choose sampler/view and compositor math together
so transfer decoding and alpha multiplication each happen exactly once.

Keep the existing encoded-sRGB/straight-alpha adapter unchanged during migration.
Do not silently relabel its textures. Correct filtering requires conversion to
premultiplied linear storage before filtering, not premultiplication after a
straight-alpha sample. Adding the new path therefore needs new output storage
and compositor support, with reference images for both contracts.

Coverage remains a scalar geometric factor, independent of source color space.
Apply coverage and inherited opacity once to premultiplied RGB and alpha. Keep
the current white-mask red-channel interpretation until native border coverage
has an independently verified replacement; changing it to alpha would change
existing antialiasing.

High gamut/HDR window negotiation, display conversion and tone mapping belong
to the separate window capability work. This proposal reserves an honest target
descriptor; it does not claim that a float texture alone enables HDR or P3.

## 3. Coordinates: logical geometry with explicit raster mapping

Default source space remains the entire unclipped outer rectangle. The producer
sees a local origin at its top left: x increases right, y increases down.
Normalized coordinates run from (0,0) at the outer top-left edge to (1,1) at the
outer bottom-right edge. Pixel centers use `(pixel_index + 0.5) / raster_extent`.
The Metal adapter owns any texture-orientation conversion.

The request distinguishes logical size in GPUI Pixels, raster extent in physical
DevicePixels, and window scale factor. Raster extent follows the renderer's
snapping/allocation decision; it must not be reconstructed by multiplying size
by scale. Map source UVs from the same final snapped quad used for composition.
Specify fractional-scale behavior in tests before changing the current mapping.

Default mappings:

| Site | Source rectangle | Coverage |
| --- | --- | --- |
| Background | Full outer element rectangle | Rounded outer shape |
| Border | Same full outer element rectangle | Border ring/dashes |
| Path | Original path bounds before clipping | Native fill or stroked geometry |

Clipping, border widths, corner radii and opacity never remap the source.
Moving a local paint without resizing it should reuse prepared output. A single
producer used at two sizes must retain independently keyed outputs rather than
oscillate through a one-entry cache. Geometry coverage has its own cache.

Do not expose screen origin in the default request: it would unnecessarily tie
local output to position. Later, offer an explicit shared paint rectangle or
source transform through one mapping abstraction, so adjacent shapes can sample
one field. Specify its coordinate space, transform, out-of-bounds sampling and
cache dependencies together. Defer this optional API until a concrete use case;
do not add mapping arguments to every paint method now. Glyphs, shadows and
backdrop/filter effects need separate site contracts before integration.

## Verification and rollout

The current patch adds style-level regression tests for setter/refinement order,
transparent override, legacy fallback, native-gradient transparency, asymmetric
widths, zero widths and per-axis clipping. Existing paint-lab Metal tests verify
coverage parity, clipping, opacity, replay, resize and gradient sampling.

Before implementing the proposed contracts, agree on these gates:

1. Preparation: old producer still builds; wrong backend/device/queue/extent,
   already-submitted work, duplicate work, reuse before completion and producer
   failure have deterministic outcomes. One failed paint leaves unrelated UI
   visible. Resource lifetime survives replay and in-flight resize.
2. Color: reference translucent saturated edges over light/dark backgrounds,
   scaled filtering, native/custom parity under the legacy descriptor, and
   explicit conversion tests for every newly supported descriptor. Never validate
   a new color path solely by comparing two implementations of the same formula.
3. Coordinates: non-square shapes, offset/negative origins, partial clipping,
   1x/2x/fractional scales, resize, asymmetric borders, path bounds and sharing
   one producer across differently sized windows. Verify both sampled pixels and
   cache reuse/invalidation.
4. Performance: measure GPU time and peak/in-flight memory in paint-lab for many
   simultaneous sizes and animated paints, plus warm and cold coverage caches.
   Cache budgets include resources retained by pending GPU submissions. Keep
   native solid-border primitive counts and native pipeline behavior unchanged.

Implement in that order: request/compatibility adapter and submission validation,
then explicit color descriptors with a preserved legacy path, then coordinate
extensions only if the default contract proves insufficient. These are separate
reviewable changes. No new effect-specific GPUI API is needed.
