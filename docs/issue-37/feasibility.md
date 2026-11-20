# Issue 37: bounded descendant clipping investigation

Status: preserved feasibility baseline; see the [native milestone](rounded-overflow-milestone.md)
for subsequent experimental integration and its separate API review.
The inventory below describes the original proposal/probe, before that integration.
Investigated 2026-11-19 against `398458ee90ef9b26d2ba9283b47b4295478aa1cc`.
Issue: https://github.com/gpui-luma-dev/xgpui/issues/37. Separate from #36.

## Recommendation

Keep `ContentMask { bounds }` unchanged. Add independent, immutable shape-chain state to
recording and interaction, and preserve the rectangle-only renderer path. Prototype
rounded rectangles and ellipses first. Retain every ancestor shape; a rectangle or
single rounded rectangle cannot represent their general intersection.

The experiment establishes that the same intersection predicate can run in native
Metal, wgpu fragment shaders with storage records, and an actual WebGL2 fragment shader
with uint-texture records. It does **not** establish a production AA algorithm,
performance budget, D3D11 implementation, or integration with GPUI scene/input/effects.
Do not ship the fluent API on this evidence alone.

Use analytic containment to evaluate geometry, but do not multiply independently
antialiased ancestor coverages. For one common rounded clip, investigate an analytic
edge solution. For multiple intersecting edges, compare joint coverage evaluation
against a cached mask of the **whole intersection**. The 16-sample predicate implemented
here is a useful reference and fallback experiment, not the recommended unconditional
cost on every fragment of every descendant. Avoid per-container color render targets;
reserve subtree isolation for effects and incompatible producer outputs.

## Source map and consequences

Links refer to repository source at the investigated revision; line numbers are local
navigation aids. Each conclusion below comes from the listed implementation.

| Source | Observation | Required integration |
| --- | --- | --- |
| [`window.rs:2490`](../../gpui/crates/gpui/src/window.rs#L2490), `ContentMask`, `scale`, `intersect` | Public `Copy`, `repr(C)` rectangle. Intersection loses all other geometry. | Leave its layout, literals, scaling and return contracts intact. It remains conservative bounds. |
| [`window.rs:4502`](../../gpui/crates/gpui/src/window.rs#L4502), `with_content_mask` | Pushes an already-intersected rectangle, then pops it. `None` installs nothing. | Separate shape scopes must save/restore immutable ancestry; do not overload this method's existing semantics. |
| [`style.rs:644`](../../gpui/crates/gpui/src/style.rs#L644), `overflow_mask` | Insets border widths only with nontransparent border color; supports single-axis rectangular masks. | Keep this public resolver for legacy overflow. Add an internal shaped resolver with explicit geometric border insets. |
| [`style.rs:712`](../../gpui/crates/gpui/src/style.rs#L712), [`geometry.rs:2479`](../../gpui/crates/gpui/src/geometry.rs#L2479) | Painting independently clamps each radius to `min(width,height)/2`. This is not CSS proportional normalization. | For the initial styled helper, share the existing resolved outer radii with painting. Do not alter all legacy backgrounds to obtain CSS normalization. |
| [`div.rs:2441`](../../gpui/crates/gpui/src/elements/div.rs#L2441), [`div.rs:2604`](../../gpui/crates/gpui/src/elements/div.rs#L2604) | Prepaint and paint both install overflow scopes; prepaint inserts the hitbox inside the scope. | Install identical geometry in both phases, before applying child scroll offsets. Keep background/border painting outside the descendant clip. |
| [`window.rs:834`](../../gpui/crates/gpui/src/window.rs#L834), [`window.rs:1181`](../../gpui/crates/gpui/src/window.rs#L1181) | Public `Hitbox` stores a rectangle; reverse-order `Frame::hit_test` intersects that rectangle before applying mouse/scroll occlusion. | Add frame-owned shape metadata keyed by hitbox ID. Test containment before adding IDs or stopping at an occluding hitbox. Invisible corners must not block lower content. |
| [`window.rs:4333`](../../gpui/crates/gpui/src/window.rs#L4333), `reuse_prepaint` | Reuses hitboxes by cloning a range. | Reuse/remap the corresponding immutable clip descriptions too. Repaint-only changes to shape geometry must invalidate prepaint/hit-test reuse. |
| [`scene.rs:41`](../../gpui/crates/gpui/src/scene.rs#L41), `insert_retained_primitive`, `finish`, `batches` | Public per-kind vectors, private paint log, bounds-based ordering, then per-kind sorting/batching. | Clip ownership must follow commands through both representations and batch splits. A side array indexed before sorting is insufficient. |
| [`scene.rs:187`](../../gpui/crates/gpui/src/scene.rs#L187), `replay` | Clones retained primitives and reinserts them with stable command identities. | Retain an owned immutable chain in the paint log, then intern/upload it in the destination scene. Never replay a previous frame's temporary GPU offset. |
| [`window.rs:4844`](../../gpui/crates/gpui/src/window.rs#L4844), deferred paint around 4296; [`deferred.rs`](../../gpui/crates/gpui/src/elements/deferred.rs) | Deferred records store only the caller's optional rectangle; the wrapper supplies `None`. They do not snapshot ambient ancestor masks. | Preserve legacy escaped-overlay behavior. A shaped clipped subtree must not accidentally recapture every deferred overlay. Add an explicit inherited variant only after its API review. |
| [`window.rs:5635`](../../gpui/crates/gpui/src/window.rs#L5635), `with_layer_capture_masks` | Source capture deliberately bypasses ancestor paint masks; interaction keeps final visible bounds. | Split ancestor final shape chains from descendant source clips, mirroring this distinction. |
| [`window.rs:5811`](../../gpui/crates/gpui/src/window.rs#L5811), `paint_custom`; backend `custom_draw.rs` files | Producer preparation is separate from host composition. Intrinsic `CustomCoverage` is per-draw geometry. | Apply ancestor clipping in host composition without a new required producer method. Intrinsic geometry coverage is not a subtree clip. |

Renderer evidence:

- [`gpui-apple/src/shaders.metal`](../../gpui/crates/gpui-apple/src/shaders.metal)
  uses rectangle clip distances in multiple primitive pipelines. `metal_renderer.rs`
  prepares custom outputs/coverage around lines 763–885, then draws in scene order.
- [`gpui-wgpu/src/shaders.wgsl`](../../gpui/crates/gpui-wgpu/src/shaders.wgsl)
  carries rectangle distances and discards outside them (quad lines 534–567; other
  pipelines repeat it). `shaders_storage.wgsl` and `shaders_webgl.wgsl` supply distinct
  transports. `wgpu_renderer.rs:178` documents the uint texture route; 1545–1588
  prepares host-composited custom textures and intrinsic coverage masks.
- [`gpui-windows/src/shaders.hlsl`](../../gpui/crates/gpui-windows/src/shaders.hlsl)
  uses `SV_ClipDistance`, e.g. quads around lines 519–555. `directx_renderer.rs:821–859`
  prepares custom textures and coverage. D3D11 is a separate implementation from
  wgpu's DX12 backend and needs its own proof.
- [`instanced/compiler.rs:168–193`](../../extensions/crates/xgpui-shader/src/instanced/compiler.rs#L168)
  generates a flat rectangle clip and fragment discard. A core-only native quad
  change cannot clip these direct shader draws. Host bindings/wrappers and generated
  shader code must change together; cached programs need revisioned pipeline keys.

## Initial semantics

1. `.rounded(...)` and `.overflow_hidden()` keep their existing behavior.
   Candidate `.overflow_hidden_clipped()` sets hidden overflow on both axes and
   opts into descendant clipping using the element's effective painted radii.
2. Resolve existing outer GPUI radii, subtract adjacent geometric border widths
   independently in x/y, and clamp negative inner radii to zero. Unequal widths
   produce elliptical inner corners. Transparent borders still inset this new
   policy; color changes cannot change hit geometry. Padding is inside the clip,
   not an additional inset. If inner width or height is nonpositive, cull the
   subtree and its hitboxes. Reject nonfinite geometry before GPU upload.
3. Use unsnapped logical geometry for resolution and CPU containment, then scale
   to device pixels once. Define and test the transform used by every pipeline;
   do not inherit today's rectangle snapping for a curved boundary by accident.
4. Use GPUI's existing per-corner clamp for the styled helper, including oversized
   radii, so the inner clip matches the border. The standalone experiment also
   exercises CSS-style common-factor normalization. Adopting that for GPUI painting
   would be a separate behavior change, outside this proposal.
5. The viewport stays stationary during scrolling. Descendants and their hitboxes
   move through it. Scroll routing, hover, click and occlusion use the same chain.
   A mathematical boundary is included (`<=`); fractional AA alpha is never an
   input threshold. This does not change keyboard focus or accessibility policy.
6. Store clipping policy independently of overflow axes. Builder order changes
   overflow normally; shaped overflow is active only when both axes are hidden.
   A later mixed-axis refinement uses legacy rectangular clipping. Resetting the
   policy restores legacy overflow; do not invent an `Overflow` enum variant.
   Scrolling keeps existing GPUI programmatic behavior; no CSS auto/scroll layout
   model is added. A separate scrolling convenience method can follow later.
7. An experimental explicit shape scope may accept rounded rectangles and ellipses
   in window logical coordinates. Circles are equal-axis ellipses. Axis-aligned
   geometry only initially; no transformed paths, winding rules, polygons or SVG
   clip language. Nested shapes preserve every original shape.
8. Legacy deferred overlays remain outside the ambient subtree clip, including
   when their supplied mask is `None`. Document this as an overlay escape rather
   than promise every deferred descendant inherits clipping. An opt-in inherited
   deferred API must capture shapes at scheduling time in both phases.

CSS is a behavioral precedent, not a conformance claim. The draft
[overflow rounding rule](https://www.w3.org/TR/css-overflow-3/#corner-clipping)
uses rounded padding-edge clipping for both-axis hidden/scroll/auto. The
[inner-radius rule](https://www.w3.org/TR/css-backgrounds-3/#corner-shaping) subtracts
border thickness; [overlap normalization](https://www.w3.org/TR/css-backgrounds-3/#corner-overlap)
uses a common factor. GPUI's existing clamp differs as described above. Thick opposing
borders can truncate an inner corner arc; do not re-normalize inner radii as though
it were a newly authored outer rounded rectangle.

## Recording and backend design to prove next

Use an internal `Arc<ClipNode>` with validated geometry, parent, conservative bounds
and immutable geometry identity. No shape node or GPU resource for legacy rectangle
scopes. Empty chains take the original backend pipelines; shaped batches use a
separate pipeline/binding layout. Resolve CPU predicates and uploaded geometry from
one canonical description. Bound shape work and memory explicitly.

For scene recording, attach the owned chain to private paint operations. During
insertion, associate it with the per-kind record. In `finish`, sort the primitive
and metadata as one record, then expose aligned primitive/clip runs to backends.
Split an existing batch whenever clip identity changes; retain paint order and
existing texture/program batch constraints. Intern chains only when making the
backend upload, retaining old owners through replay/prepaint reuse. Geometry
mutation creates a new identity rather than editing a retained node.

There is an unresolved compatibility trap: callers can mutate public scene vectors
after insertion. An index-only metadata vector cannot detect same-length reorder or
replacement. The first integration must either expose a new authoritative prepared
command stream for shaped scenes and declare legacy direct-vector construction
rectangle-only, or mediate those mutations through reviewed APIs. Do not infer clip
identity from draw order (multiple commands can share it), and do not ship a
length-only consistency check. Existing manual scenes without a shape scope keep
legacy behavior. This is a release gate, not solved by the isolated probe.

For transport, a prototype record is four 16-byte vectors: bounds xywh, rx[4], ry[4],
and kind/reserved. Eight shapes occupy 512 bytes; a rounded/ellipse node is 64 bytes
before CPU ownership overhead. Metal constant/device buffers, wgpu storage records,
WebGL2 RGBA32UI texture words and D3D11 SRV/constant-buffer records can represent it.
This is a **private experiment ABI**, not a change to primitive layouts. Batch-level
chain bindings avoid adding a required field to every public GPU primitive; evaluate
batch fragmentation against an indexed per-instance side channel before choosing.

| Technique | What it buys | Cost and decision |
| --- | --- | --- |
| Analytic chain | No subtree color target; simple CPU containment; shallow shape data | Per-fragment shape work, AA of intersections, every pipeline must participate. Keep as the shallow candidate, optimize single-shape edges. |
| Stencil | Intersections without color isolation; mature hardware support | Clip push/pop passes, stencil attachments and custom pipeline state. Smooth edges still need coverage/MSAA. Not the first choice for existing non-stencil pipelines. |
| Cached joint coverage | One final coverage sample per descendant; deeper chains and future paths | R8 memory, rasterization, uploads, scale-aware keys and animated invalidation. Strong fallback candidate; must rasterize joint geometry rather than multiply AA masks. |
| Subtree color isolation | Host can clip otherwise incompatible outputs and apply group effects once | Color target bandwidth, larger halos, wide-gamut storage and group semantics. Use for required isolation, not every rounded div. |

For a cached mask, start from conservative intersection bounds expanded for edge
filter support, rasterize the original joint geometry, and key by geometry ancestry,
DPI, device-space placement and raster algorithm revision. An R8 400×300 region costs
120,000 bytes before alignment/cache metadata. Do not quantize animated coordinates
without a demonstrated tolerance. Retained content revisions and clip revisions are
separate dependencies; moving a clip may reuse content but invalidate coverage.

## Effects, text and foreign content

- A capture holds an ancestor-final chain separately from its source-local chain.
  Suspend the former while rendering/filtering the source halo; preserve descendant
  clips that belong inside the capture. Apply the suspended intersection exactly
  once to the completed group. Nested groups need explicit ownership of each clip
  boundary so the same ancestor is not also attenuated inside and outside the group.
- Backdrop sampling reads the required destination region before clipping its output.
  Final coverage belongs after the effect; clipping the read texture destroys blur
  inputs. Preserve existing backdrop pass barriers and dependency revisions.
- Opacity applies once at its existing group boundary. Coverage multiplies premultiplied
  RGB and alpha together in the target's linear composition space. It must not run
  on encoded P3 values before decode, or be repeated after wide-gamut conversion.
- Host-composited custom textures can receive the chain after producer preparation.
  Intrinsic producer/path coverage must be distinguished from ancestor coverage;
  coincident intrinsic and ancestor AA edges need the same joint-edge reference test.
  They are not automatically fixed by an ancestor-only joint mask.
- Direct shader draws require host-controlled wrappers/bindings for ancestor clips.
  Previously compiled/custom legacy shaders lacking that binding need a texture
  composition route or an explicit unsupported-capability error. No new required
  `CustomContent` trait method is proposed.
- Subpixel text needs separate investigation: scalar clip coverage cannot simply
  replace RGB glyph coverage. Compare the existing subpixel blend path with grayscale
  text in shaped scopes; any grayscale fallback must be documented and tested for
  text-cache separation. The demo's text is a texture, so it proves none of this.
- `PaintSurface` and custom outputs sampled by GPUI can be covered in the host pass.
  Actual platform child windows/foreign surfaces that bypass that pass are outside
  the initial subset. Do not promise HWND/CALayer/native overlay clipping from these
  shaders; require a supported texture route or report unsupported composition.

## Experiment and observed results

Source and reproduction: [`apps/xgpui-ext-lab/clip-chain-probe`](../../apps/xgpui-ext-lab/clip-chain-probe/README.md).
Captured numeric results: [`results/`](../../apps/xgpui-ext-lab/clip-chain-probe/results/).

On Apple M4 Max, 12 cases pass through each of native Metal, wgpu storage records,
wgpu uint-texture records and browser WebGL2. Cases include empty/unclipped chain,
rectangle, asymmetric corners, ellipse, rounded+ellipse intersection, coincident
ancestors, depth eight, oversized radii with unequal borders, disjoint shapes, and
1.25/1.5/2 scale factors. Fractional geometry is included; the scale cases use the
same fixed 96×96 observation viewport and do not exercise a whole GPUI window.

Native Metal and wgpu match the CPU 4×4 predicate exactly in this run. WebGL2 matches
the quantized RGBA8 CPU reference exactly. The harness permits one sample difference
for float portability on Metal/wgpu and one code value for WebGL2. A negative-control
unit check demonstrates that multiplying coincident coverages would darken some
pixels by at least 0.24; the joint predicate remains idempotent through depth eight.

Against a 32×32 CPU raster reference, whole-image mean absolute coverage error is at
most 0.001257; maximum single-pixel error is 0.125. Whole-image means dilute edge
errors and are not an edge-quality acceptance metric. Fixed-grid sampling therefore
needs an adaptive/reference-quality replacement before production. The CPU reference
uses independent point evaluation but shares the same shape formula, so this is not
an independent proof of CSS border geometry.

Representative wgpu submission+wait+readback elapsed values in the captured run:
per-corner storage 0.963 ms, nested 0.951 ms, depth eight 2.612 ms; uint-texture depth
eight 2.697 ms. Cold pipeline effects and readback dominate some cases. These are
96×96 one-draw smoke measurements, **not GPU timings, a backend comparison, a benchmark,
or ordinary UI overhead measurements**. Native Metal timings similarly include
synchronization/readback. No reliable budget can be derived from them.

The visible browser demo has rectangle/rounded/ellipse modes, radius and nesting
controls, animated radius, stationary clip with scrolled texture/children, and
clickable children at corners. Verified: point (50,46) misses in rounded mode but
hits child 1 in rectangular mode; point (135,60) hits the visible part of child 1
with nesting disabled. It deliberately composes a raster/text texture in a host
shader; it does not demonstrate GPUI image/text pipelines, scene replay or GPUI events.

## Bounded implementation plan and release gates

1. **Private core integration:** scoped chain state, owned replay metadata, hitbox
   reuse, and clip-homogeneous batches on Metal and wgpu. Keep styled builder private
   or experimental until the recording/public-vector contract is demonstrated.
2. **AA and costs:** compare single-shape analytic, adaptive joint coverage, and a
   cached R8 intersection against a dense independent rasterizer, including coincident
   and crossing edges. Measure edge-only error and temporal stability. Test 0/1/2/4/8
   distinct clips, repeated clips and animated intersections; identical nodes may be
   deduplicated only if geometry and coordinate space match.
3. **Capabilities:** run native D3D11 compile/readback with the same corpus; test actual
   GPUI wgpu WebGL2 and WebGPU shader generation/transport. The browser experiment is
   actual WebGL2, but does not run Naga's generated GLSL or GPUI's texture decoder.
4. **Integrated lab:** add a real-window lab/probe in `apps/xgpui-ext-lab`, register it
   in `labs.toml`, and exercise native image, glyph, path, custom shader and surface
   draws, retained/prepaint replay and deferred overlays. Keep the standalone harness
   as a transport/reference check. No lab registration is claimed by this change.
5. **Budgets and API:** choose production chain/cache limits from the integrated
   measurements, implement an exact supported fallback, and present every actual
   public addition individually. Until then, the standalone scope is capped at eight
   shapes and rejects overflow/invalid geometry. Empty geometry will cull in production;
   the probe rejects it before upload. Never silently replace shapes with bounds.

Production memory budgets and maximum depths remain undecided. A fallible experimental
scope may report `TooDeep`, `UnsupportedContent` or `MaskBudgetExceeded`; an infallible
style builder must not ship before its fallback/error presentation is resolved.
Do not turn a renderer failure into accidentally visible unclipped content.

Performance acceptance proposal: compare unchanged UI scenes before/after integration
with identical release builds, warm pipelines, fixed viewport/DPI and repeated runs.
Record CPU prepaint/paint/batching time, allocation counts, draw/batch counts, GPU
pass timings where available, bytes uploaded, mask bytes and cache hit rate. Include
text-heavy scrolling, 1,000 ordinary rectangular containers, many shallow rounded
containers, deep clips and animation. Require no shaped allocations/bindings for
rectangle-only scenes; investigate a repeatable >2% regression in the ordinary path.
The 2% figure is a proposed investigation threshold, not a measured result or a
promise. Report median and tails; do not subtract a single cold readback measurement.

Validation matrix for the integration:

| Area | Required check |
| --- | --- |
| Geometry | 1/1.25/1.5/2 DPI, subpixel translation, unequal/zero/oversized corners, transparent/thick/asymmetric borders, empty inner box, nonfinite rejection |
| AA | Coincident clips, near-coincident edges, tangencies, crossing ellipses, intrinsic/ancestor overlap, premultiplied color fringes, animation stability |
| Input | Click, hover, scroll and occlusion through excluded corners; visible child remains hittable; stationary scrolling viewport; boundary inclusion |
| Lifetime | Retained paint and prepaint reuse, cache eviction, resize/DPI changes, shape-only revisions, nested scopes, deferred explicit/escaped clipping |
| Ordering | Interleaved primitive kinds, texture/program batch splits, direct/manual scene insertion, native/custom output, layer/backdrop barriers |
| Effects | Blur halos, nested filter capture, backdrop source reads, opacity once, scalar/grayscale versus subpixel text, linear P3/wide output |
| Backends | Native Metal, wgpu native/WebGPU/WebGL2, native D3D11; capabilities must reject unsupported paths explicitly |

Windows/Linux and native D3D11 runtime behavior are unverified. Rely on applicable CI
for branch checks; user testing on those platforms occurs only after merge to main.
No cross-platform user test or remote-run script is requested.

## Public API and compatibility inventory

**Actual non-extension public additions/changes in this patch: none.** All executable
code is in a isolated unpublished lab package and Swift/browser fixtures.
No `ContentMask`, `Style`, `Hitbox`, primitive, custom producer or GPU renderer ABI
changes. No serialization migration. There is no final API approval request yet.

Candidate changes for a later individually reviewed implementation:

| Candidate | Purpose | Compatibility/migration | Extension alternative |
| --- | --- | --- | --- |
| Provided `Styled::overflow_hidden_clipped(self) -> Self` | Opt-in both-axis shaped overflow | Additive method can conflict with downstream traits; opt-in call sites change spelling only. No required trait method. | Prefer an extension-trait convenience method initially if a minimal core policy hook exists. |
| Independent policy on public `Style` and generated refinement | Carry policy through generic styled elements | New public field breaks exhaustive struct literals. Refinement Serialize/Deserialize/JsonSchema changes require legacy default, omission and round-trip tests. No GPU ABI change by itself. | Wrapper avoids a style-field break but must prove layout/input/effect parity; it cannot implement backend clipping alone. |
| Validated shape descriptor and fallible `Window` scope, exact signature TBD | Custom elements and experimental ellipse scope | Additive API; specify units, boundary and failure behavior. Keep `with_content_mask` and `ContentMask` unchanged. | Convenience wrappers can live outside core; recording/input state cannot. Keep private until feasibility integration. |
| Renderer-neutral prepared clip runs/accessors, exact signatures TBD | Share scene-owned clip descriptions across backend crates | Public across crate boundaries even if backend-facing. Preserve old vectors and `batches` where possible; authoritative shaped-stream/mutation contract needs resolution. Private record ABI does not imply zero API impact. | Not reliable as extension-only bookkeeping. |
| Optional inherited deferred scope, only if needed | Clip explicit deferred descendants while preserving escaped overlays | Additive API rather than changing `defer_draw(..., None)` semantics. No change to existing calls. | Helper can select the approved core variant. |

Keep public `ContentMask`'s layout/`Copy` contract, `Overflow` variants, all primitive
struct literals and required producer traits stable. Final signatures, serialization
rules, GPU alignment/transport tests and migration notes belong in a separate API
review before finalizing or releasing, as required by `AGENTS.md`.
