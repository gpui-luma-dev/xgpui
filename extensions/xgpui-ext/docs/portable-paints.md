# Portable paint backend checkpoint

The Metal submission validation checkpoint is committed as `20c7e11`.
This slice adds a wgpu adapter and portable WGSL effects, without changing any
public GPUI core types, methods or native primitive layouts.

## Implemented

Enable `xgpui-shader`'s opt-in `wgpu` feature. The existing `SolidColor`,
`Gradient` and `MeshGradient` types dispatch on the backend context. The same
immutable snapshots and styled paint calls work with either Metal or wgpu.
Linear, elliptical radial, conic, diamond and Hermite mesh implementations remain
in the extension. The wgpu mesh evaluates Hermite vertices directly in its vertex
shader, using the same patch-local samples and triangle order as Metal's compute
pipeline. It uses no storage buffers or compute shaders.

`gpui_wgpu::custom_draw::WgpuCustomContext` supplies a device, a host recording
closure, and an output allocator. Producers record into one host-owned encoder;
they do not return independently submitted command buffers. Output construction
is private to the context and fixes device, queue identity, texture format, extent
and usages. The host submits preparation before ordered composition. Shader
validation remains wgpu's responsibility; producer errors still fail the frame.
No failure isolation or fallback policy is implemented.

A cloned `WgpuCustomOutput` retains its queue and recording state. It can be reused
in the same recording or after submission on that queue, without waiting for
CPU-observed completion. Dropping an unsubmitted context marks its outputs
abandoned. Extension caches check `can_reuse` so an aborted frame cannot leave an
uninitialized cached texture eligible for the next frame. Cache keys include
queue identity and exact raster extent. The extension keeps multiple extents
within a 64 MiB live-output budget; in-flight resource retention remains outside
that budget. Related gradient and mesh snapshots share compiled pipelines.

The wgpu compositor owns rounded coverage, inherited rectangular clipping and
opacity. Native border/path masks preserve the existing white-red-channel
coverage convention. The mask cache has a separate 64 MiB budget. Non-solid
native border sources still use the legacy native-background adapter and undo
premultiplication before composition. RGBA8Unorm output contains straight-alpha
encoded-sRGB channel values; this does not add high gamut or new color semantics.

GPUI Window painting retains logical source size/scale through the preparation
adapter. Low-level scene records retain the documented 1x default. No paint field
is remapped by clipping or border coverage.

## Run

From the repository root:

```sh
just --justfile apps/paint-lab/justfile wgpu-test
just --justfile apps/paint-lab/justfile wgpu-headless --effect mesh --frames 60 --png /tmp/wgpu-mesh.png
cargo test --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml --features wgpu --target-dir target
cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui-wgpu/Cargo.toml --lib wgpu_custom --target-dir target -- --nocapture
```

`WGPU_BACKEND=vulkan`, `metal`, or `dx12` forces backend selection. An unavailable
backend fails explicitly rather than falling back and claiming a successful
platform test. The process-wide headless device is selected once; use separate
processes to test different backends. Default macOS GPUI windows still use the
native Metal renderer; the portable paint-lab option is headless only.

The wgpu test dependency uses GPUI test-support rather than bench-support, which
previously pulled in the broken standalone bench_metrics dependency path. The
explicit bench-support feature remains available for benchmark consumers.

## Evidence and remaining platform work

Local macOS runs use wgpu's Metal backend. Tests cover all analytic gradient
reference pixels, hard stops, transparent interpolation, native/custom border and
path coverage, opacity, clipping without remapping, replay, sharing and resize.
Independent Metal/WGSL mesh comparisons sample three animation states. Adapter
tests cover recording readiness, abandoned output, queue ownership tokens,
texture extent and one-shot submission. Naga tests validate both effect shaders
and generate SPIR-V plus HLSL shader model 5.0 for each entry point. These are
code-generation tests, not driver execution on Windows or Vulkan.

The manual `Portable paint GPU checks` workflow provides Linux/Vulkan (Mesa) and
Windows/wgpu-DX12 jobs. It has not been dispatched from this local task. Software
Mesa checks validate correctness, not hardware performance.

GPUI's native Windows renderer is DirectX 11. It still rejects Custom batches.
The next native Windows slice needs a D3D11 preparation context/output, execution
and resource-lifetime rules, native geometry masks and compositor, then HLSL
producer pipelines in xgpui-shader. Reuse the validated WGSL math through HLSL
generation where practical; establish explicit constant-buffer bindings before
connecting it to D3D11. Test it on a Windows host before claiming native Windows
support. Do not switch the Windows window renderer to wgpu as an incidental part
of this work. Vulkan device/driver runs and native Windows rendering remain
separate acceptance gates.
