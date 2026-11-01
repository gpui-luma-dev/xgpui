# Portable wgpu paints

SolidColor, Gradient, MeshGradient and procedural ShaderPaint use portable content;
Metal is the current executor and portable wgpu execution remains pending. The
extension-owned wgpu producer adapters are removed. The browser producer page
reports pending execution; native color samples and direct brushes remain.
The low-level recording API below describes core external-content infrastructure,
not an alternate xgpui-shader producer API.

## Use

```toml
# App under apps/:
[dependencies]
xgpui-shader = { path = "../../extensions/crates/xgpui-shader", features = ["wgpu"] }
```

```rust
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_shader::{painted, MeshGradient, MeshGrid, WindowPaintExt};

let paint = Arc::new(MeshGradient::new(MeshGrid::regular(2, 2, vec![
    [1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [1.; 3],
])?, 18)?);
painted().w(px(320.0)).h(px(240.0)).bg_paint(paint.content());
window.paint_path_with_paint(path, paint.content());
```

## Producer API

`gpui_wgpu::custom_draw` — signature summary:

```rust
pub struct WgpuCustomContext {
    pub device: Arc<wgpu::Device>,
    // Private recording state.
}
impl WgpuCustomContext {
    pub fn cache_key(&self) -> usize;
    pub fn create_output(&self, extent: Size<DevicePixels>) -> Result<WgpuCustomOutput>;
    pub fn record<T>(
        &self,
        encode: impl FnOnce(&mut wgpu::CommandEncoder) -> Result<T>,
    ) -> Result<T>;
    pub fn can_reuse(&self, output: &WgpuCustomOutput) -> bool;
}
pub struct WgpuCustomOutput { /* private fields; Clone */ }
impl WgpuCustomOutput {
    pub fn texture(&self) -> &wgpu::Texture;
}
```

## Contract

```text
producer    reusable cache hit OR allocate → record → return output
host        validate → stage → submit accepted batch → compose
failure     discard unsubmitted batch → frame error
reuse       same recording / accepted batch / submitted queue

output      RGBA8Unorm · straight alpha · encoded-sRGB SDR
mapping     full unclipped bounds; logical size ≠ raster extent
coverage    analytic rounded rect; native border/path mask red channel
cache key   queue + extent (+ logical size for WGSL)
cache       64 MiB live output; separate 64 MiB coverage
mesh        portable content requires the pending wgpu executor
```

No producer submission/GPU wait. Cache budgets exclude in-flight retention.
[Recording details](paint-contract-hardening.md#implemented-discardable-recording-transactions).

## Checks

```sh
just --justfile apps/xgpui-ext-lab/paint-lab/justfile wgpu-test
# Mesh execution currently returns UnsupportedCapability until the portable wgpu executor exists.

# One headless backend/device per process; unavailable backend → error.
WGPU_BACKEND=metal just --justfile apps/xgpui-ext-lab/paint-lab/justfile wgpu-test
WGPU_BACKEND=vulkan just --justfile apps/xgpui-ext-lab/paint-lab/justfile wgpu-test
# Windows PowerShell: $env:WGPU_BACKEND = "dx12"

cargo test --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-wgpu/Cargo.toml \
  --features test-support --lib wgpu_custom --target-dir target
```

Before the portable source migration, macOS wgpu/Metal mesh parity was exercised.
Current portable mesh runtime acceptance is Metal-only; wgpu asserts its explicit
unsupported capability. Legacy transaction and direct shader/brush tests remain. SPIR-V/HLSL generation passes; Vulkan/DX12 runtime acceptance
remains outstanding. Software/readback timing is not hardware GPU timing.

[Native D3D11](native-dx11-paints.md) · [WGSL](portable-wgsl-v1.md)

Backend workflows, focused diagnostics, caching, and cold/warm measurement are
documented in [Paint GPU validation](paint-validation.md).
