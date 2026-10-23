# Experimental portable WGSL paint v1

This is an experimental vertical slice in `xgpui-shader`, behind the
`portable-shaders` feature. It adds no GPUI core API. It proves one user-authored
fragment paint through native Metal and wgpu before stabilizing an authoring API.
The native Windows DirectX 11 adapter and actual Vulkan/Windows driver acceptance
remain separate work; successful code generation is not platform rendering proof.

## Application API

```rust
use xgpui_shader::experimental::{ShaderProgram, ShaderParameters};

let program = ShaderProgram::new_v1(include_str!("wash.wgsl"))?;
let paint = program.paint(
    ShaderParameters::new()
        .float("progress", 0.15)
        .vec3("tint", [0.95, 0.15, 0.35])
        .float("alpha", 1.0),
)?;

// Identical app code for either implemented backend.
let element = div().bg_paint(paint);
```

A paint also implements `CustomContent`, and converts directly into `Paint` for
border/path entry points. The program owns immutable source, reflection metadata
and shared backend pipelines. Each `paint` call creates an immutable parameter
snapshot. Cloning a paint shares that snapshot and its output caches. Keep the
program in app state; animate by supplying new parameter snapshots. No clock or
backend handles appear in the authoring API.

Enable `portable-shaders` for native Metal. Also enable `wgpu` when using that
renderer. These are dependency features, not app-side OS branches. Backend dispatch
and translation live inside the extension. Existing built-in producer APIs and
GPUI paint setters remain unchanged.

## User source

The complete app example is `apps/paint-lab/src/shaders/portable-wash.wgsl`:

```wgsl
struct Parameters {
    progress: f32,
    tint: vec3<f32>,
    alpha: f32,
}

fn paint(input: PaintInput) -> vec4<f32> {
    let blend = clamp(input.uv.x + parameters.progress, 0.0, 1.0);
    let accent = vec3(0.1, clamp(input.position.y / 160.0, 0.0, 1.0), input.size.x / 400.0);
    return vec4(mix(parameters.tint, accent, blend), parameters.alpha);
}
```

The extension supplies the `PaintInput` type, `parameters` value, fullscreen
geometry, resource bindings and entry points. User source contains no binding
indices, shader-stage annotations or backend code.

| Input | Meaning |
| --- | --- |
| `uv: vec2<f32>` | Normalized coordinates across the full source rectangle |
| `position: vec2<f32>` | Local position in logical GPUI pixels |
| `size: vec2<f32>` | Full logical source size |

Origin is top-left; x increases right and y increases down. Raster pixel centers
supply UVs, and logical position is UV multiplied by logical size. Preparation
keeps exact raster dimensions separate internally; raster size and window scale
are not public shader inputs in this slice.

Clipping and border coverage do not remap the field. Backgrounds and borders use
the full outer element rectangle; paths use their original bounds. GPUI applies
coverage, clip and inherited opacity after the source is prepared. Low-level
scene records retain the preparation adapter's documented 1x default unless a
producer explicitly supplies other logical geometry.

## Parameters and subset

`Parameters` must contain 1..=32 fields of f32 or vec2/3/4<f32>, occupying at most
1024 bytes. The extension reflects member offsets and packs bytes, including
padding. The app never reproduces WGSL layouts with Rust casts. All fields must
be supplied, including unused fields. Unknown names, wrong vector lengths and
non-finite values fail before creating the snapshot. Repeated parameter setters
replace the previous value. Arrays, matrices and integer/bool parameters are
outside this first slice; typed Rust parameter derives are deferred.

User source is limited to 64 KiB. Math, helper functions, local values and
conditional control flow are supported. V1 rejects user resources/global
variables, overrides, custom entry points, textures, storage/compute, derivatives,
discard and loops. The private geometry binding is not a user escape hatch; use
PaintInput. This is a deliberately narrow paint subset, not arbitrary WGSL.

## Output and versioning

`new_v1` explicitly selects straight-alpha, encoded-sRGB SDR RGBA, matching the
existing source/compositor convention. The wrapper clamps output channels to
0..=1; shader arithmetic must produce finite values. User code must not apply
shape coverage or inherited opacity itself. There is no high-gamut negotiation
or linear-light filtering change. A later color contract must get an explicit
version rather than silently changing v1 paints.

## Compilation, caching and errors

Construction parses and validates WGSL, checks the subset, reflects parameters,
and verifies MSL, HLSL shader model 5.0 and SPIR-V translation. Metal uses the
translated MSL and explicit reflected entry-point names; wgpu consumes the
wrapped WGSL with extension-owned uniform bindings. Even unused uniforms have a
valid host layout. Device pipelines compile lazily and are shared by snapshots.

Output cache identity includes backend queue, exact raster extent and logical
source size. Changing logical size at the same raster extent must regenerate
output. Parameter snapshots have independent output caches; cloning a snapshot
reuses its cache. Each backend's live output cache is bounded at 64 MiB, excluding
resources retained by in-flight GPU work. Metal retains pending command buffers;
wgpu uses the existing host-owned encoder/readiness contract. Neither adapter
adds a GPU completion wait during normal preparation.

Parse and WGSL validation errors include an original user-source line where a
source span is available, plus the wrapped-source diagnostic. Parameter errors
name the field. Native driver compilation errors still refer to generated MSL;
a complete cross-language source map is deferred. Preparation/device errors
retain existing frame-error propagation. Failure isolation is not implemented.

## Run and evidence

From the repository root:

```sh
just --justfile apps/paint-lab/justfile portable-test
just --justfile apps/paint-lab/justfile portable-headless --frames 2 --png /tmp/portable-metal.png
just --justfile apps/paint-lab/justfile portable-wgpu --frames 2 --png /tmp/portable-wgpu.png
cargo test --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml --features wgpu,portable-shaders --target-dir target
```

The app example in `src/portable.rs` has no OS/backend conditionals. Both captures
use the same source, parameters and shape scene. Pixel checks on native Metal and
wgpu cover alpha/opacity, partial clipping, logical coordinate input, logical-size
cache invalidation at a fixed raster extent, immutable parameter snapshots and
unused uniforms. CPU tests cover reflected vec3 padding, parameter errors,
subset rejection, source diagnostics and target translation. Existing gradient,
mesh and coverage reference checks run alongside the experiment.

Local wgpu execution uses its Metal backend. The manual portable-paints workflow
can run the reference tests on Linux/Vulkan and Windows/wgpu-DX12; it has not been
dispatched from this task. Native DirectX 11 integration, texture parameters,
compute/custom geometry, full source maps, GPU timing and stable API guarantees
remain outside this experiment.
