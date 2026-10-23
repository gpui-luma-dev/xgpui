# Native Windows DirectX 11 paints

The native Windows adapter is implemented without changing GPUI core's public
API. Apps continue to construct `MeshGradient`, `Gradient`, `SolidColor`, or an
experimental `ShaderProgram` paint. No Windows feature flag, binding declaration,
or OS conditional is needed in app code. The normal Windows window renderer
remains D3D11. The optional wgpu path remains a separate backend.

## Division of responsibilities

`gpui-windows::custom_draw` supplies a device, a host-owned deferred recording
context, immutable-after-submission BGRA8Unorm outputs, and an HLSL pipeline
helper. The native renderer validates output context/device identity, readiness
and extent, submits the preparation list, then composes custom batches in scene
order. Native border/path masks preserve GPUI's existing geometry semantics.
Clip rectangles, corner coverage and element opacity apply during composition.
Native background-adapter sources are unpremultiplied before custom composition.

`xgpui-shader` owns the effects and WGSL-to-HLSL translation through Naga, with
explicit constant-buffer register bindings and reflected entry names. Mesh uses
the same Hermite vertex evaluation, uniform packing and triangle order as the
wgpu implementation. Analytic gradients share the same uniform packing too.
There is no second Windows-specific copy of the effect math.

Preparations record on a deferred context and only the host executes them. A
submitted output can be reused without a CPU wait on the same immediate context;
outputs from abandoned recordings cannot be reused. Context identity is retained
with each output, preventing pointer reuse. Each snapshot retains outputs within
a 64 MiB budget; related snapshots share the compiled pipeline on a device.
Coverage has a separate bounded cache. Device recovery drops the compositor;
retained effect caches reject old device/context identities on subsequent use.
Command lists restore immediate-context state so following native batches retain
their shaders, buffers, render targets and other bindings.

The current output remains straight-alpha encoded-sRGB SDR. D3D feature level
11.0 or later is required for custom paints. This does not add high gamut,
producer failure isolation, or an application fallback policy. Preparation,
translation, driver compilation and submission errors still fail the frame.

## Validation

From a Windows checkout, use native D3D11/WARP for deterministic offscreen checks:

```powershell
cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui-windows/Cargo.toml --features test-support --lib dx11_custom -- --nocapture
cargo test --locked --config .cargo/validation.toml --manifest-path apps/paint-lab/Cargo.toml --features wgpu,portable-shaders -- --nocapture
cargo run --locked --config .cargo/validation.toml --manifest-path apps/paint-lab/Cargo.toml -- --headless --effect mesh --frames 3 --png native-dx11-mesh.png
cargo run --locked --config .cargo/validation.toml --manifest-path apps/shader-lab/Cargo.toml
```

`gpui-platform::current_headless_renderer()` now supplies a native WARP renderer
on Windows when test/bench support is enabled. It reuses the window renderer's
scene encoding, preparation, mask and composition paths, with an offscreen target
instead of a swap chain. This is correctness/readback timing, not hardware GPU
performance. Interactive apps use their normal hardware device selection.

Backend tests cover submission, pending/abandoned/foreign output rejection,
extent limits, alpha/clipping and restoration of immediate-context bindings.
Paint-lab checks native border/path coverage, analytic reference pixels, shader
coordinates/parameters, sharing/replay/resize, and mesh parity against wgpu at
three animation states. The manual `Portable paint GPU checks` workflow includes
a Windows native job and uploads a mesh capture.

Local evidence is Windows-target cross-compilation of both apps and backend
tests, HLSL generation tests, and macOS Metal/wgpu regression execution. Native
Windows shader compilation, WARP pixel tests, real window rendering, hardware
performance and device-loss stress still require execution on Windows. The
workflow has not been dispatched from this task.

Windows-target extension Clippy passes with warnings denied. A whole-backend
Clippy run with warnings denied is blocked by pre-existing warnings in Windows
clipboard, event, window and renderer code; those unrelated fixes are not part
of this adapter.
