# Shader paints and instanced shaders

Both rendering models are experimental and require `portable-shaders`. Choose by
integration and workload; neither name implies stabilization or broader backend support.

| Choice | Shader paints | Instanced shaders |
| --- | --- | --- |
| Rust entry | `shader_paint::ShaderProgram::new(source)` | `instanced::ShaderProgram::<T>::new(source)` |
| Use when | A reusable paint should fill a styled background, border or path | Typed effects should draw directly in scene order, including many repeated instances |
| Rendering | Cached offscreen output, integrated through `Paint` | Main-pass draws; compatible consecutive instances can batch |
| Parameters | Named `ShaderParameters`; reflected packing into immutable `ShaderPaint` snapshots | POD `ShaderInstance` records with explicit field metadata, checked against WGSL layout |
| Parameter limits | 1–32 f32 scalar/vector members; at most 1,024 packed bytes | At most 128 bytes; f32 scalars/vectors with verified offsets and stride |
| WGSL entry | `fn paint(input: PaintInput) -> vec4<f32>`; supplied global `parameters` | `fn shade(input: ShaderInput, parameters: Parameters) -> vec4<f32>` |
| Coordinates | Full source UV, local logical position and size | Full effect UV, local logical position and size, plus scale factor |
| Clipping and opacity | Host applies coverage and opacity without remapping the paint field | Host applies content-mask clipping and inherited opacity; shader supplies analytical shape coverage |
| Loops and derivatives | Rejected, as is discard | Subject to Naga validation |
| Resources | No user textures, resource bindings or compute | No user resource bindings or compute |
| Output | Straight-alpha encoded-sRGB SDR | Encoded-sRGB SDR by default; explicit `new_linear_p3` requires an active native `WideGamutSdr` window |
| Backends | Metal, D3D11 and optional wgpu adapters | Metal, D3D11, wgpu, browser WebGPU and WebGL2 |

Backend implementation does not establish driver acceptance on every platform.
The [shader-paint guide](portable-wgsl-v1.md) and
[instanced shader guide](instanced-wgsl-v2.md) record runtime validation and
remaining platform checks. Main-pass batching preserves draw order; it does not
sort all matching programs together. Reuse compiled programs in either model.

## Migration from v1/v2 terminology

“v1” referred to shader paints; “v2” referred to instanced shaders. These are
parallel rendering models, not successive releases of one API. Existing guide
filenames remain available to preserve incoming links.

```rust,ignore
// Before: removed beta API.
use xgpui_shader::experimental::{ShaderParameters, ShaderProgram};
let program = ShaderProgram::new_v1(source)?;
```

```rust,ignore
// After: identical source and output contracts.
use xgpui_shader::shader_paint::{ShaderParameters, ShaderProgram};
let program = ShaderProgram::new(source)?;
let paint = program.paint(ShaderParameters::new().float("gain", 0.5))?;
```

`ShaderPaint` is also exported from `shader_paint`. The old `experimental` module
and `new_v1` constructor have been removed as a breaking beta API change; update
all callers to the new names. No aliases or deprecated forwarding APIs remain.
`instanced::ShaderProgram<T>::new` and typed instance terminology are unchanged.

Version numbers are reserved for actual source-contract or ABI revisions within
each rendering model. Naming changes do not relax validation, change color
semantics, or add backend support.
