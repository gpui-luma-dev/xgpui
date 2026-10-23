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

Source cache keys include backend ownership and extent; WGSL also keys logical
size. Shape coverage has its own cache. Metal coverage and extension live-output
budgets are 64 MiB each; in-flight resources are outside those live-cache budgets.

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

# Recording errors, staged reuse, abandonment and submission ordering.
WGPU_BACKEND=metal MTL_DEBUG_LAYER=1 cargo test --offline \
  --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-wgpu/Cargo.toml --target-dir target \
  --features test-support --lib custom_draw
```

Local wgpu tests use Metal. D3D11/WARP and Vulkan runtime acceptance require their
platforms; cross-compilation and shader translation do not establish it.

[Application API](extension-paints.md) ·
[D3D11 recording notes](../../../extensions/xgpui-ext/docs/native-dx11-paints.md)
