# Shader paints and instanced shaders

Both rendering models are experimental and require `portable-shaders`. Choose by
integration and workload; neither name implies stabilization or broader backend support.

| Choice | Shader paints | Instanced shaders |
| --- | --- | --- |
| Rust entry | `shader_paint::ShaderProgram::new(source)` | `instanced::ShaderProgram::<T>::new(source)` |
| Use when | A reusable paint should fill a styled background, border or path | Typed effects should draw directly in scene order, including many repeated instances |
| Rendering | Cached offscreen output, integrated through `Paint` | Main-pass draws; compatible consecutive instances can batch |
| Parameters | Core `UniformValues`; reflected packing into immutable `ShaderPaint` snapshots | POD `ShaderInstance` records with explicit field metadata, checked against WGSL layout |
| Parameter limits | Core reflected 32-bit scalar/vector and f32 matrix layouts; at most 64 KiB | At most 128 bytes; f32 scalars/vectors with verified offsets and stride |
| WGSL entry | `fn paint(input: PaintInput) -> vec4<f32>`; supplied global `parameters` | `fn shade(input: ShaderInput, parameters: Parameters) -> vec4<f32>` |
| Coordinates | Full source UV, local logical position and size | Full effect UV, local logical position and size, plus scale factor |
| Clipping and opacity | Host applies coverage and opacity without remapping the paint field | Host applies content-mask clipping and inherited opacity; shader supplies analytical shape coverage |
| Loops and derivatives | Accepted with discard, subject to portable WGSL validation | Subject to Naga validation |
| Resources | No user textures, resource bindings or compute | No user resource bindings or compute |
| Output | Straight-alpha encoded-sRGB SDR | Encoded-sRGB SDR by default; explicit `new_linear_p3` requires an active native `WideGamutSdr` window |
| Backends | Portable content executor (currently Metal); D3D11/wgpu execution pending | Metal, D3D11, wgpu, browser WebGPU and WebGL2 |

Backend implementation does not establish driver acceptance on every platform.
The [shader-paint guide](portable-wgsl-v1.md) and
[instanced shader guide](instanced-wgsl-v2.md) record runtime validation and
remaining platform checks. Main-pass batching preserves draw order; it does not
sort all matching programs together. Reuse compiled programs in either model.

## Producer lifecycle

Shader paints are immutable CPU-only ContentSource snapshots. Their program and
parameter validation use GPUI portable reflection; native work and caching belong
to the host. Reuse a program and retain a presentation slot per changing paint
role. Use the same API regardless of backend. Unsupported executors return typed
capability errors. There is no alternate preparation API or compatibility adapter.

```rust,ignore
use gpui::content::{ContentDraw, PresentationSlotId, UniformValues};
use xgpui_shader::shader_paint::ShaderProgram;
let program = ShaderProgram::new(source)?;
let paint = program.paint(UniformValues::new().float("gain", 0.5))?;
let slot = PresentationSlotId::new();
painted().bg_paint(ContentDraw::new(paint.content()).slot(slot));
```

Instanced shaders remain a distinct main-pass workload, used by brushes and text.
They are not a legacy producer API. Main-pass batching and offscreen portable
content have different geometry/presentation lifecycles. Historical v1/v2 guide
filenames remain links to these two current rendering models.
