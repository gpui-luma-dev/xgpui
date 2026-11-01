# Portable wgpu paints

SolidColor, Gradient, MeshGradient and procedural ShaderPaint describe portable
content. Metal and wgpu execute those graphs inside the framework; producers and
applications use the same API on both. D3D11 is implemented and Windows-cross-checked;
its GPU runtime acceptance remains pending. See [D3D11 checks](native-dx11-paints.md).

```rust
use gpui::{prelude::*, *, content::{ContentDraw, PresentationSlotId}};
use xgpui_shader::{painted, MeshGradient, MeshGrid};

let paint = MeshGradient::new(MeshGrid::regular(2, 2, vec![
    [1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [1.; 3],
])?, 18)?;
let slot = PresentationSlotId::new(); // retain per changing destination
painted().w(px(320.0)).h(px(240.0))
    .bg_paint(ContentDraw::new(paint.content()).slot(slot));
```

The `wgpu` feature on xgpui-shader enables comparison tests; it does not choose a
producer backend. Procedural shaders require `portable-shaders`.

## Execution and presentation

The host lowers solids, image/buffer uploads and copies, fullscreen/mesh render
passes and multi-output compute passes to wgpu. Resources are initialized before
use; graph dependencies use queue ordering. A serial worker on native hosts and
an asynchronous task on the browser's owning event loop admit at most two
submissions at once. Browser work yields without waiting for the GPU.

Completed GPU textures go directly to the compositor. Stable presentation slots
retain the previous image during parameter changes, and completion schedules a
new frame. Normal window drawing performs no producer readback or GPU wait.
Strict headless snapshots report `Pending` until their exact generation is ready.
Scene removal releases subscriptions and cached outputs; submission leases retain
resources until execution and compositor sampling complete. Workers are shared
per device/queue, including the exported-image budget.

Default limits are eight waiting jobs, 64 cached pipeline variants, 128 MiB of
charged GPU resources and 64 MiB of packed validation readback. Readback staging
headroom and retained exports count against GPU admission. wgpu hides driver
allocation sizes: textures are charged in 64 KiB granules after row padding,
buffers in 4 KiB granules. These are bounded host reservations, not measured
physical VRAM. Pipeline/driver bookkeeping and ordinary compositor coverage
allocations are separate from graph resource reservations.

sRGB, linear-sRGB and coverage images compose with explicit alpha metadata.
Wide P3/Rec.2020 presentation returns `UnsupportedCapability` on the current sRGB
wgpu compositor. Offscreen FP16 graphs can still execute and be read back for
validation. Device capabilities also gate storage buffers, compute and storage
textures. Use WebGPU for the browser Gradient/MeshGradient demos; WebGL2 lacks
the required storage/compute capabilities and receives a typed capability error.

## Checks

From the repository root:

```sh
just --justfile apps/xgpui-ext-lab/paint-lab/justfile wgpu-test
cargo run --offline --config .cargo/validation.toml \
  --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab \
  --features runtime-shaders,portable-shaders,wgpu -- \
  --headless --wgpu --portable-shader --frames 60 --png /tmp/paint-wgpu.png

cargo test --offline --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-wgpu/Cargo.toml \
  --features test-support --test portable_content_wgpu --target-dir target \
  -- --test-threads=1

bash apps/xgpui-ext-lab/extensions-web/build.sh --offline
python3 apps/xgpui-ext-lab/extensions-web/serve.py
```

Open `http://localhost:8081/?demo=shader&backend=webgpu`, then the color and
Hermite demos. Animate mesh/WGSL, change opacity, edit mesh points, scroll and
resize. Look for advancing images without flashing to transparent, smooth
parameter updates, correct rounded/border/path coverage and transparency.
Headless checks must pass and write a PNG with the three painted shapes.
Graph acceptance checks include pixel-exact operations, queue limit one,
failure/cancellation recovery and a 512-root subprocess with a timeout.

Native runtime acceptance is exercised with wgpu on Metal. Linux/Vulkan and
Windows/DX12 runtime acceptance remain platform QA tasks; D3D11 is a separate
executor with native runtime acceptance pending. Browser runtime results are recorded in the design's implementation log.

[Design and lifecycle](portable-content-api-design.md) ·
[Native Metal](native-metal-paints.md) · [Native D3D11](native-dx11-paints.md)


Pipeline reuse acceptance includes `hot_pipeline_survives_a_cold_pipeline_cache_miss`:
with two cache entries, introducing a cold third program must preserve the recently
used program without recompilation. Every generation also checks its rendered
color. The cache evicts one least-recently-used entry; active graphs retain their
own pipeline references. This test is included in the graph command above.
