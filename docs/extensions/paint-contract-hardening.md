# Paint preparation and failure contracts

Downstream GPUI integration. Implemented behavior first; deferred designs last.

## Existing producers still work

A complete legacy-signature producer that delegates to the portable solid paint:

```rust
use anyhow::Result;
use gpui::{CustomContent, DevicePixels, Size};
use std::any::Any;
use xgpui_shader::SolidColor;

#[derive(Debug)]
struct Accent(SolidColor);

impl Accent {
    fn new() -> Result<Self> {
        Ok(Self(SolidColor::new([0.9, 0.2, 0.3, 1.0])?))
    }
}

impl CustomContent for Accent {
    fn prepare(
        &self,
        backend: &dyn Any,
        size: Size<DevicePixels>,
    ) -> Result<Box<dyn Any>> {
        self.0.prepare(backend, size)
    }
}
```

The provided trait method forwards the exact raster extent:

```rust
// Existing GPUI default implementation — no producer migration required.
fn prepare_with_request(
    &self,
    backend: &dyn std::any::Any,
    request: &PreparationRequest,
) -> anyhow::Result<Box<dyn std::any::Any>> {
    self.prepare(backend, request.raster_size())
}
```

## Read logical and raster geometry separately

An optional override inside `impl CustomContent`:

```rust
fn prepare_with_request(
    &self,
    backend: &dyn std::any::Any,
    request: &gpui::PreparationRequest,
) -> anyhow::Result<Box<dyn std::any::Any>> {
    let logical = request.logical_size();   // GPUI Pixels, full unclipped bounds.
    let raster = request.raster_size();     // Exact allocated DevicePixels.
    let scale = request.scale_factor();

    // Do not reconstruct raster by multiplying logical by scale.
    // This solid producer needs only the exact extent.
    self.prepare(backend, raster)
}
```

Window recording retains logical size and scale in a private wrapper; replay
shares it. Direct low-level scene records default to 1x logical geometry.

```rust
let request = gpui::PreparationRequest::new(
    size(px(120.0), px(80.0)),
    size(gpui::DevicePixels(180), gpui::DevicePixels(120)),
    1.5,
)?;
assert_eq!(request.scale_factor(), 1.5);
```

## Implemented discardable recording transactions

Internal wgpu/D3D11 flow — pseudocode, not a public API:

```text
for draw in custom_draws:
    attempt = batch.begin()                  // No encoder allocated yet.
    output = producer.prepare(attempt)       // First record() allocates it.
    validate_device_queue_extent(output)
    prepare_composition(output)
    batch.stage(attempt, output)

submit_in_order(batch)                       // Only after all draws succeed.

on any returned error:
    drop(current_attempt)                   // Pending output → abandoned.
    drop(batch)                             // All staged output → abandoned.
    return frame_error                      // No paint skipping yet.
```

```text
Output state     Reusable by
──────────────   ──────────────────────────────────────────
pending          Its own recording only
staged           Later transactions in the same batch
submitted        Later compatible preparations
abandoned        Nobody

record() callback error → discard entire attempt
producer catches that error → attempt still invalid
cache-only preparation → no recorder, no empty submission
failed later attempt → previously submitted cache stays valid
```

Retry allocation counts differ by backend: wgpu and D3D11 abandon staged output
when a later producer aborts the batch, so the first successful retry regenerates
the output once. Compiled pipelines remain reusable. Metal's legacy producer path
retains its unsubmitted command and reuses it on retry. After a successful retry,
all paths must reuse the submitted output without another preparation; pixels and
pipeline compilation counts must remain stable.

No new public GPUI methods. No normal-path GPU completion wait. Native mask work
has an independent submission path and is not rolled back. Raw GPU handles remain
a cooperative contract; transactions cannot undo externally submitted work.

## Metal compatibility path

```text
prepare all outputs
    ↓
validate texture device / format / extent / sampling usage
    ↓
validate every command: exact host queue + NotEnqueued
    ↓
revalidate whole batch before commit
    ↓
deduplicate retained command-buffer identities → commit each once
```

Invalid batches submit no producer commands. An empty command list is allowed
for ready or previously host-submitted cache output; the producer must guarantee
readiness. Metal's raw-command compatibility path is not a host recording transaction.

## Color and coordinates today

```text
source format       BGRA8Unorm
source encoding     encoded sRGB, SDR
source alpha        straight
stop interpolation  premultiplied encoded sRGB (analytic gradients)

source origin       top-left; +x right; +y down
uv at pixel center  (pixel_index + 0.5) / raster_extent
logical position    uv * logical_size
background / border full outer element rectangle
path                original geometric bounds
clip / opacity      host composition; never remap the source
coverage            native mask red channel; rounded rect analytic coverage
```

```text
Metal SolidColor / Gradient / MeshGradient
  key       = queue identity + physical width/height
  capacity  = 64 MiB live output textures per producer snapshot
  eviction  = individual least-recently-used entries
  pending   = return the same command buffer until host submission
```

Eviction drops cache ownership only; retained/in-flight outputs stay valid.

```text
Metal / wgpu / D3D11 coverage cache
  validate  = path vertices <= 1,000,000 before hashing
  lookup    = streamed normalized-geometry fingerprint; no temporary Vec
  equality  = exact stored geometry comparison inside fingerprint bucket
  insert    = retain normalized words only on a successful cache miss
  budget    = mask texture + retained normalized geometry
  CPU cost  = O(vertices); allocation-free does not mean constant-time
```

Source cache keys include backend ownership and extent; WGSL also keys logical
size. Shape coverage has its own cache. Metal coverage and extension live-output
budgets are 64 MiB each; in-flight resources are outside those live-cache budgets.

## Compositor uniform uploads

```text
payload      64 bytes/draw; bounds, clip, radii, viewport, opacity, source flags
wgpu         persistent aligned arena; one slot/draw; one packed upload/batch
             grow capacity only; upload only after preparation succeeds
             submit frame before reuse; queue ordering protects in-flight reads
D3D11        one DYNAMIC constant buffer/compositor; WRITE_DISCARD per draw
             fixed-size stack packing; driver renames discarded backing storage
```

```sh
# Growth, queued frame snapshots and uniform buffer creation counts.
cargo test --offline --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-wgpu/Cargo.toml --target-dir target \
  --features test-support --lib wgpu_custom_uniform
# Windows: same command with gpui-windows and filter dx11_custom_uniform.

# Per-draw values, failed-frame recovery, native batches after custom draws.
cargo test --offline --config .cargo/validation.toml \
  --manifest-path apps/paint-lab/Cargo.toml --target-dir target \
  --features wgpu,portable-shaders uniform_frames
```

wgpu still allocates staging memory for its batch upload and creates per-draw
texture views/bind groups. Effect-producer uniforms are separate. No public API
change or GPU completion wait. D3D11 execution requires Windows/WARP validation.

## Low-alpha compositor checks

```sh
MTL_DEBUG_LAYER=1 cargo test --offline --config .cargo/validation.toml \
  --manifest-path apps/paint-lab/Cargo.toml --target-dir target \
  --features wgpu,portable-shaders low_alpha_saturated_edges -- --nocapture
```

```text
sources     saturated RGB solids + antialiased stripes; sRGB/Oklab gradients
alpha       0, 1e-5, 5e-4, 1/255, 2/255, 0.02, 0.2 (solid/stripe cases)
backdrops   #101010 / #f0f0f0
geometry    fractional origin/extent; bilinear sampling; rounded AA edges
reference   quantized native source → CPU bilinear filter → premultiplied source-over

Metal + wgpu/Metal: 188 cases/backend; maximum error 1/255 per RGB channel.
No shader/blend changes. No demonstrated need for an epsilon cutoff or immediate
premultiplied compositor migration in this 8-bit path.
```

Gradient alpha is inferred from black/white readbacks, with up to one level of
source-alpha uncertainty. These tests cannot resolve sub-8-bit errors or establish
HDR, Vulkan or Windows runtime behavior. Premultiplied output remains a separate
color-contract change, not a correction established by these results.

## Deferred — not callable APIs

| Proposal | Required decision/test |
| --- | --- |
| Skip failed paint, bounded diagnostics | Unrelated UI survives; no stale animation fallback |
| Metal host recording | Preserve legacy producers before deprecating raw handles |
| Color descriptors | Primaries, transfer function, alpha association, validated output |
| Linear premultiplied filtering | Saturated translucent-edge references; preserve legacy output |
| Shared paint rectangle/transform | Coordinate space, sampling rules and cache dependencies |
| High gamut/HDR | Separate window/display negotiation; not enabled by float textures |

## Run the contract tests

```sh
# Native/portable geometry, alpha, clipping, replay and legacy-producer recovery.
just --justfile apps/paint-lab/justfile metal-test
just --justfile apps/paint-lab/justfile portable-test

# Request validation, old signature forwarding and fractional-scale geometry.
cargo test --offline --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui/Cargo.toml --target-dir target \
  --features test-support --lib preparation_

# Built-in Metal cache: size alternation, LRU eviction, pending reuse, queues.
MTL_DEBUG_LAYER=1 cargo test --offline --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --target-dir target --lib metal_cache

# Allocation counts, forced collisions, normalization and early path limits.
cargo test --offline --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-wgpu/Cargo.toml --target-dir target \
  --features test-support --lib coverage_cache
# Same filter on gpui-apple and gpui-windows for their private implementations.

# Recording errors, staged reuse, abandonment and submission ordering.
WGPU_BACKEND=metal MTL_DEBUG_LAYER=1 cargo test --offline \
  --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-wgpu/Cargo.toml --target-dir target \
  --features test-support --lib custom_draw
```

Local wgpu tests use Metal. D3D11/WARP and Vulkan runtime acceptance require their
platforms; cross-compilation and shader translation do not establish it.

[Application API](extension-paints.md) ·
[D3D11 recording notes](native-dx11-paints.md)
