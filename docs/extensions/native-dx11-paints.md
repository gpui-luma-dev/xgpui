# Native D3D11 portable content

Windows, feature level 11.0 or later. SolidColor, WideColor, Gradient,
MeshGradient and ShaderPaint use the same portable ContentSource API as Metal
and wgpu. Native handles, HLSL bindings and backend selection belong to GPUI.
Earlier feature levels retain ordinary window rendering but report unsupported
capabilities for portable content.

```rust
use gpui::{prelude::*, *, content::{ContentDraw, PresentationSlotId}};
use xgpui_shader::{painted, MeshGradient, MeshGrid};

let grid = MeshGrid::regular(2, 2, vec![
    [1.0, 0.1, 0.2], [1.0, 0.8, 0.1],
    [0.2, 0.3, 1.0], [0.1, 0.9, 0.7],
])?;
let mesh = MeshGradient::new(grid, 18)?;
let slot = PresentationSlotId::new(); // retain for this changing destination
painted().w(px(960.0)).h(px(640.0))
    .bg_paint(ContentDraw::new(mesh.content()).slot(slot));
```

## Execution and lifecycle

A per-device/immediate-context service records portable graph operations on a
serial worker's deferred context. It translates WGSL to HLSL shader model 5,
compiles private pipelines with D3DCompile and submits bounded command lists.
Native context protection serializes immediate-context calls; command-list
execution restores the window renderer's bindings. Event queries establish GPU
completion, including intermediate resources and compositor sampling leases.
All polling/wait loops live on the worker or in explicit headless validation.

Execution includes initialized images/buffers, all mip uploads/copies, unaligned
GPU byte copies, reflected uniforms/storage/samplers, fullscreen/indexed/instanced
rendering, load/blend and multi-output compute. Vertex streams are copied into
bounded GPU buffers so an index cannot expose vertices beyond the declared draw.
Draw offsets and dispatch dimensions are supplied through private constants to
preserve portable builtin semantics; see the [D3D11 generated-ID rules](https://microsoft.github.io/DirectX-Specs/d3d/archive/D3D11_3_FunctionalSpec.htm#8.16%20VertexID). Individual samplers are lowered from Naga's
generated DX12 aliases into direct D3D11 sampler registers.

Presentation samples completed textures directly, with no producer readback.
A running graph finishes while newer inputs coalesce; completed slot history
remains visible until its replacement is ready. Completion posts a window redraw.
Strict snapshots return Pending until their exact requested generation is ready.
Scene removal cancels unfinished work and retires submitted resources safely.
Sampling reservations survive renderer teardown; device recovery uses a fresh
service and rejects the old device's resources. A completion timeout quarantines
its resources and rejects further admission until that GPU work retires.

Default bounds: two in-flight submissions, eight waiting jobs, 64 pipeline
variants, 128 MiB of charged GPU resources and 64 MiB of packed validation readback.
Retained exports, bounded vertex copies, uniforms and readback staging headroom
count against admission. D3D11 hides physical allocation sizes: padded textures
are charged in 64 KiB granules and buffers in 4 KiB granules. These reservations
exclude driver/pipeline bookkeeping and the existing compositor coverage cache.

sRGB, linear-sRGB and R8 coverage use explicit alpha metadata. Linear P3 and
Rec.2020 content require the existing WideGamutSdr compositor and FP16 storage;
normal SDR presentation reports UnsupportedCapability for them. Extended linear
values remain unclamped through wide composition. Typed storage images support
write-only access at the baseline feature level; use immutable sampled images
for reads. Backend limits remain typed capability errors.

## How to test on Windows

From the repository root, in PowerShell:

```powershell
cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui-windows/Cargo.toml --features test-support --test portable_content_dx11 -- --test-threads=1 --nocapture
cargo test --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-shader/Cargo.toml --features portable-shaders --test portable_dx11 -- --test-threads=1 --nocapture
cargo test --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --features portable-shaders -- --test-threads=1

cargo run --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin portable-content
cargo run --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --features portable-shaders -- --portable-shader
```

The graph suite should report **17 passed and one ignored subprocess helper**;
the helper is exercised by the bounded 512-root parent test. The producer suite
should report **three passed**. Graphics Tools/debug-layer installations enable
the existing debug-device setup in debug builds. Neither suite substitutes wgpu
or DX12 for D3D11.

What to look for:

- The 512-cell portable-content grid appears without another click. Replace warm/
  cool, switch 512 → 1 → 512, clip, change opacity and resize rapidly. Controls
  must remain responsive; completed pixels must advance without blank flashes.
- In paint-lab, select each analytic gradient and mesh, animate, and drag angle/
  geometry controls continuously. Then select User WGSL and vary progress, tint,
  alpha, clipping and opacity. All three shapes must update together, retain
  correct rounded/border/path coverage and stop changing when paused.
- Tests must verify exact pixels, mip levels, queue limit one, cancellation and
  failure recovery, builtin offsets, zero producer readbacks, bounded cache size
  and eventual resource release after scene removal. The explicit validation
  target is the only readback used for presentation acceptance.

For a headless PNG:

```powershell
cargo run --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --features portable-shaders -- --headless --portable-shader --frames 60 --png paint-dx11.png
```

Look for three correctly clipped/painted shapes; coverage/alpha/reference checks
must pass before the file is written. This timing includes validation readback,
so it is not a window animation frame-rate measurement.

## Validation recorded here

Windows cross-checks cover the backend, its unit/integration tests, all portable
producer tests and both native demo binaries. Four CPU HLSL translation tests run
on macOS, including the real gradient and mesh shaders, sampled inputs, runtime
storage lengths, barriers, unused uniforms and draw/dispatch constants.
D3DCompile, D3D11 GPU pixels, native interactive performance and wide-display
presentation still require Windows runtime acceptance: the available local
Windows VM is marked invalid. Passing cross-checks is not a GPU runtime claim.

[Portable design and lifecycle](portable-content-api-design.md) ·
[wgpu](portable-paints.md) · [Metal](native-metal-paints.md)
