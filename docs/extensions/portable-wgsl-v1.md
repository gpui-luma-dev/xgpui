# Portable procedural shader paints

`xgpui-shader/portable-shaders` supplies a CPU-only procedural paint convenience
API over GPUI portable content. The host owns backend execution on Metal and
wgpu, and D3D11. The D3D11 implementation is cross-checked; native Windows GPU
acceptance remains pending. See [Windows tests](native-dx11-paints.md).
The historical guide filename does not name a separate or compatibility API.

## Source contract

Declare a `Parameters` struct and `fn paint(input: PaintInput) -> vec4<f32>`:

```wgsl
struct Parameters { progress: f32, tint: vec3<f32>, alpha: f32 }
fn paint(input: PaintInput) -> vec4<f32> {
    let blend = clamp(input.uv.x + parameters.progress, 0.0, 1.0);
    let accent = vec3(0.1, clamp(input.position.y / 160.0, 0.0, 1.0), input.size.x / 400.0);
    return vec4(mix(parameters.tint, accent, blend), parameters.alpha);
}
```

The wrapper supplies `parameters`, entry points, fullscreen geometry and:

```wgsl
struct PaintInput {
    uv: vec2<f32>,        // Raster pixel centers over the full source rectangle.
    position: vec2<f32>,  // Local logical pixels: uv * size.
    size: vec2<f32>,      // Full logical source size, independent of raster scale.
}
```

Origin is top-left. Clipping, rounded corners, border/path coverage and inherited
opacity are applied by the compositor without remapping the source. The wrapper
clamps output to straight-alpha encoded-sRGB SDR in RGBA8. This ShaderProgram is
separate from `instanced::ShaderProgram<T>` and its explicit wide-SDR contracts.

## Retained portable content

```rust
use gpui::{prelude::*, *};
use gpui::content::{ContentDraw, PresentationSlotId, UniformValues};
use xgpui_shader::{painted, WindowPaintExt};
use xgpui_shader::shader_paint::ShaderProgram;

let program = ShaderProgram::new(include_str!("wash.wgsl"))?;
let parameters_at = |progress| UniformValues::new()
    .float("progress", progress).vec3("tint", [0.95, 0.15, 0.35]).float("alpha", 1.0);
let paint = program.paint(parameters_at(0.15))?;
let slot = PresentationSlotId::new(); // retain per destination across edits
painted().w(px(320.)).h(px(240.))
    .bg_paint(ContentDraw::new(paint.content()).slot(slot));

// A different destination needs its own retained slot.
let path_slot = PresentationSlotId::new();
window.paint_path_with_paint(path, ContentDraw::new(paint.content()).slot(path_slot));
let next = program.paint(parameters_at(0.75))?; // fresh immutable snapshot
let same = paint.clone(); // same content identity
```

ShaderPaint implements ContentSource and converts into Content/Paint; it does not
implement CustomContent. Use `content()` for retained graphs or window paints.
The duplicate ShaderParameters API and Shader/shader compatibility aliases are
removed. Paint accepts native backgrounds or portable ContentDraw; it no longer
accepts arbitrary CustomContent through Paint::Custom/Arc conversions.
Core external-video/CustomContent facilities are separate framework hooks.

## Parameters and WGSL validation

ShaderProgram::parameter_layout() exposes the declared field names, types and
packing layout, including unused fields. Parameters use the core `UniformValues`
and `UniformLayout` implementation:

- `float`, `vec2`, `vec3`, `vec4` for f32 values.
- `ints` and `uints` for 32-bit signed/unsigned scalar/vector members.
- `matrix` for column-major f32 matrices, packed with reflected padding.

All names, exact types/arity and finite float values are validated at paint()
time. Every declared member must be supplied, even if unused. Padding is zeroed;
negative zero is canonicalized. Unused uniform declarations remain reflected via
PortableProgram::uniform_layout but do not become execution bindings.
Nested uniform structs/arrays remain unsupported by the core packing contract.

User source is capped at 64 KiB. Loops, derivatives and discard are accepted
subject to portable WGSL validation; discarded pixels keep the transparent clear.
The wrapper reserves resources, globals and entry points; user helpers/constants
are allowed. Use ContentSource/PortableProgram directly for multi-resource,
texture, compute or custom-geometry graphs. Parse diagnostics include the user
line when available. Shader/parameter errors are typed ContentError values.

No extension-owned MSL/HLSL/SPIR-V compilation, native caches, preparation calls or
backend adapters remain. A source describes one portable render pass using its
logical/raster request and typed uniforms. The host shares equivalent realizations,
compiles native pipelines, schedules GPU work and retains allocations through
sampling completion. Pending frames keep the last completed destination image;
strict snapshots return Pending until the requested generation is ready.

## Test and capture

```sh
MTL_DEBUG_LAYER=1 cargo test \
  --manifest-path extensions/crates/xgpui-shader/Cargo.toml \
  --test portable_shader_paint --features portable-shaders,gpui-apple/runtime_shaders \
  -- --test-threads=1 --nocapture
cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab \
  --features runtime-shaders,portable-shaders -- --portable-shader
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin paint-lab --features runtime-shaders,portable-shaders -- \
  --headless --portable-shader --frames 3 --png /tmp/paint-portable-wgsl.png
```

Expect five passing native tests and one ignored subprocess helper: exact source
pixels, logical-size/raster independence, alpha/clamping, integer/matrix/loop/
derivative/discard pixels, pending-frame retention, pipeline reuse, 60 completed
updates and bounded 512→1→512 draws with zero producer readbacks. Stream timings
measure producer work, not visible frame pacing.

In the app select User WGSL (also selected by --portable-shader). Angle drives
progress; the selected stop's RGB/alpha drives tint/transparency. Drag continuously,
animate/pause, change opacity/clip and resize. All three shapes should update
without flashes, sticking or catching up after mouse release. Source is in
`apps/xgpui-ext-lab/paint-lab/src/portable.rs` and `src/shaders/portable-wash.wgsl`.
The browser WebGPU producer page renders the same source. See
[wgpu acceptance](portable-paints.md) for build commands and capabilities.
