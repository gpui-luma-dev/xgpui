# Portable WGSL paint v1

Experimental `xgpui-shader/portable-shaders`. Backend selection stays inside the extension.

## Dependency

From an app under `apps/`:

```toml
[dependencies]
xgpui-shader = { path = "../../extensions/xgpui-ext/crates/xgpui-shader", features = ["portable-shaders"] }
# Add "wgpu" for the optional wgpu renderer.
```

## wash.wgsl

```wgsl
struct Parameters {
    progress: f32,
    tint: vec3<f32>,
    alpha: f32,
}

fn paint(input: PaintInput) -> vec4<f32> {
    let blend = clamp(input.uv.x + parameters.progress, 0.0, 1.0);
    let accent = vec3(
        0.1,
        clamp(input.position.y / 160.0, 0.0, 1.0),
        input.size.x / 400.0,
    );
    return vec4(mix(parameters.tint, accent, blend), parameters.alpha);
}
```

Extension-supplied type (do not redeclare it in user source):

```wgsl
struct PaintInput {
    uv: vec2<f32>,        // Full source rectangle, normalized.
    position: vec2<f32>,  // Local logical GPUI pixels: uv * size.
    size: vec2<f32>,      // Full logical source size.
}
// Also supplied: parameters, bindings, fullscreen geometry and stage entry points.
```

Origin: top-left; +x right, +y down. UV uses raster pixel centers. Clip and border
coverage never remap the field; the host applies coverage/opacity.

## Rust

```rust
use gpui::{prelude::*, *};
use xgpui_shader::{painted, WindowPaintExt};
use xgpui_shader::experimental::{ShaderProgram, ShaderParameters};

let program = ShaderProgram::new_v1(include_str!("wash.wgsl"))?;

let parameters_at = |progress| {
    ShaderParameters::new()
        .float("progress", progress)
        .vec3("tint", [0.95, 0.15, 0.35])
        .float("alpha", 1.0)
};

let paint = program.paint(parameters_at(0.15))?;
painted().w(px(320.0)).h(px(240.0)).bg_paint(paint.clone());
painted().border_8().border_paint(paint.clone());
window.paint_path_with_paint(path, paint.clone());

// Keep program in view state. Animate by creating immutable snapshots.
let next = program.paint(parameters_at(0.75))?;
let same = paint.clone(); // Shares parameters + cached output.
```

## Parameter API

Signature summary:

```rust
impl ShaderParameters {
    pub fn new() -> Self;
    pub fn float(self, name: impl Into<String>, value: f32) -> Self;
    pub fn vec2(self, name: impl Into<String>, value: [f32; 2]) -> Self;
    pub fn vec3(self, name: impl Into<String>, value: [f32; 3]) -> Self;
    pub fn vec4(self, name: impl Into<String>, value: [f32; 4]) -> Self;
}
// All members required, even unused ones. Repeated setter: last value wins.
// Reflected layout/padding is packed by the extension; no Rust layout casts.
```

| Constraint | v1 |
| --- | --- |
| Source size | ≤ 64 KiB |
| Parameter members | 1–32 |
| Packed parameter size | ≤ 1024 bytes |
| Types | `f32`, `vec2/3/4<f32>` |
| Values | Finite; exact reflected names/types |
| Allowed | Math, helpers, local values, conditionals |
| Rejected | Resources/globals, overrides, entry points, textures, storage/compute |
| Rejected | Derivatives, discard, loops, integer/bool parameters, arrays, matrices |
| Output | Straight-alpha encoded-sRGB SDR; channels clamped to `0..=1` |
| Shader obligation | Finite arithmetic; no shape coverage/opacity multiplication |
| Versioning | Color-contract changes require a new version |

## Compilation and cache

```text
new_v1(source)
  → parse + validate restricted WGSL
  → reflect parameters/offsets
  → verify MSL + HLSL SM5 + SPIR-V translation

paint(parameters)
  → validate names/types/finite values
  → pack reflected layout
  → immutable snapshot

prepare(snapshot, request)
  → backend pipeline (lazy; shared across related snapshots)
  → cached output OR record draw
  → host validation/submission/composition

output key = backend queue/context + exact raster extent + logical source size
budget     = 64 MiB live output/backend; excludes in-flight resource retention
```

| Failure | Result |
| --- | --- |
| Parse/validation | Original user line when span available + wrapped diagnostic |
| Parameter set | Field-specific error before snapshot creation |
| Driver compilation | Generated-source diagnostic; full cross-language map deferred |
| Preparation/device | Frame error; no paint skipping |
| Rejected recording | Unsubmitted work discarded on wgpu/D3D11 |

Raw Metal commands retain the compatibility submission contract. No new normal-path
GPU completion waits. Texture parameters, compute/custom geometry, HDR and stable
API guarantees are outside v1.

## Test and capture

```sh
just --justfile apps/xgpui-ext-lab/paint-lab/justfile portable-test
just --justfile apps/xgpui-ext-lab/paint-lab/justfile portable-headless --frames 2 --png /tmp/portable-metal.png
just --justfile apps/xgpui-ext-lab/paint-lab/justfile portable-wgpu --frames 2 --png /tmp/portable-wgpu.png
cargo test --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --target-dir target
```

| Test group | Coverage |
| --- | --- |
| CPU/Naga | vec3 padding, bad parameters, subset rejection, source diagnostics, target translation |
| Metal + wgpu/Metal | Alpha, opacity, clipping, logical coordinates, logical-size cache invalidation |
| Snapshots | Parameter immutability, sharing, unused uniforms |
| Windows/Vulkan | Runtime acceptance outstanding; translation ≠ driver execution |

App source: `apps/xgpui-ext-lab/paint-lab/src/portable.rs`, `src/shaders/portable-wash.wgsl`.
[Native D3D11](native-dx11-paints.md) · [wgpu adapter](portable-paints.md)
