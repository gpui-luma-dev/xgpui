# Native D3D11 paints

Windows · feature level ≥ 11.0 · `xgpui-shader` · no app-side OS branches.

Migration status: SolidColor, Gradient and MeshGradient now use portable
ContentSource descriptions. Their D3D11 producer adapters have been removed;
portable D3D11 execution is pending, so these sources return UnsupportedCapability
on Windows for now. Metal is the current executor. The low-level recording API
below remains transitional infrastructure for experimental shader paints.

## Portable source API (D3D11 executor pending)

```rust
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_shader::{painted, MeshGradient, MeshGrid};

let grid = MeshGrid::regular(2, 2, vec![
    [1.0, 0.1, 0.2], [1.0, 0.8, 0.1],
    [0.2, 0.3, 1.0], [0.1, 0.9, 0.7],
])?;
let mesh = Arc::new(MeshGradient::new(grid, 18)?);
painted().w(px(960.0)).h(px(640.0)).bg_paint(mesh.content());
```

## Producer API

`gpui_windows::custom_draw` — signature summary:

```rust
pub struct Dx11CustomContext {
    pub device: ID3D11Device,
    // Private recording state.
}
impl Dx11CustomContext {
    pub fn cache_key(&self) -> usize;
    pub fn create_output(&self, extent: Size<DevicePixels>) -> Result<Dx11CustomOutput>;
    pub fn can_reuse(&self, output: &Dx11CustomOutput) -> bool;
    pub fn record<T>(
        &self,
        encode: impl FnOnce(&ID3D11DeviceContext) -> Result<T>,
    ) -> Result<T>;
}
pub struct Dx11CustomOutput { /* private fields; Clone */ }
impl Dx11CustomOutput {
    pub fn texture(&self) -> &ID3D11Texture2D;
}
```

## Contract

```text
extension   WGSL → Naga → HLSL SM5 → private pipeline → record draws
host        validate → stage → execute accepted batch → compose
failure     discard unsubmitted batch → frame error
reuse       same recording / accepted batch / submitted context
state       command-list execution restores immediate-context bindings

output      BGRA8Unorm · straight alpha · encoded-sRGB SDR
coverage    native border/path mask; host clip/corners/opacity
cache key   retained context identity + extent (+ logical size for WGSL)
cache       64 MiB live output/snapshot; separate coverage cache
recovery    reject old device/context caches
```

[Recording details](paint-contract-hardening.md#implemented-discardable-recording-transactions).

## Windows checks

```powershell
cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui-windows/Cargo.toml --features test-support --lib dx11_custom
cargo test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --features wgpu,portable-shaders
# Mesh execution currently returns UnsupportedCapability on D3D11.
# Run mesh acceptance on Metal; restore native D3D11 acceptance with its portable executor.
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab
```

Cross-compile, HLSL generation and extension Clippy pass. Windows driver/WARP,
interactive rendering, hardware performance and device-loss runtime acceptance
remain outstanding. WARP/readback timing is not hardware GPU timing.

[wgpu adapter](portable-paints.md) · [WGSL](portable-wgsl-v1.md)
