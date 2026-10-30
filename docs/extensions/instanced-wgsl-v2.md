# Instanced shaders (experimental)

See [choosing a shader rendering model and migrating old names](shader-models.md).

Issue [#21](https://github.com/gpui-luma-dev/xgpui/issues/21). Enable
`xgpui-shader/portable-shaders`. `instanced` is an API alongside shader paints; existing texture producers keep their existing behavior.

`WindowShaderExt::paint_custom_shader(bounds, &program, instance)` records an
immutable typed instance. GPUI clips and applies inherited opacity, preserves
scene draw order, and combines compatible shader runs in the main render pass.
A native primitive or another program can split a run. Programs are never globally
sorted by identity. GPUI's explicit `paint_layer` contract still requires
non-overlapping geometry within a layer.

## Implementation status

As of 2026-10-25, the first experimental slice is implemented locally. It is an
opt-in SDR API, not the complete unrestricted shader system proposed in the RFC.

| Area | Status |
| --- | --- |
| Scene recording, compatible ordered runs and retained paint replay | Implemented |
| Typed parameter metadata, WGSL reflection and layout checks | Implemented; f32 scalars/vectors, 128-byte payload |
| Generated vertex/fragment wrappers; clipping and inherited opacity | Implemented |
| WGSL validation and MSL, HLSL SM5 and SPIR-V translation | Implemented |
| Native Metal and wgpu rendering | Implemented; macOS pixel checks passed |
| Native D3D11 dispatch | Implemented and Windows cross-compiled; Windows driver execution pending |
| Browser WebGPU and WebGL2 | Implemented; local Chromium smoke checks passed |
| Rounded-box, pulse-glow and soft particle presets | Implemented |
| Local native demo and shared WASM companion | Implemented; same Rust example views and shader sources |
| Explicit linear-P3 shader output and optional gamut fills/gradients (#25) | Implemented; Metal reference checks passed, Windows cross-compiled |

The shader regression suite passed, including Metal/wgpu pixel checks, ABI and
invalid-input rejection, native/shader ordering, replay, and a 5,000-instance
single-run case. Native app launch and controls were checked on macOS. The
optimized browser particle demo showed roughly 16.7 ms redraw intervals at
5,000 instances in one local session; this includes scheduling and is not GPU
timing or a 120 FPS guarantee.

Remaining acceptance: execute and compare on Windows/D3D11; capture GPU draw
counts, allocations and timings on representative hardware; broaden runtime
coverage for device recovery, buffer growth and wide-SDR composition. Arbitrary
resource bindings, integer/array/matrix/nested parameter records, HDR output,
the larger preset library and mesh-gradient migration are outside this first slice.

## Rust

```rust
use gpui::*;
use xgpui_shader::{WindowShaderExt, instanced::{ShaderProgram, presets::*}};

// Construct once, outside paint. Clone shares the program identity.
let program = ShaderProgram::<EffectInstance>::new(ROUNDED_BOX)?;
// During an element's paint callback:
window.paint_custom_shader(bounds, &program, EffectInstance {
    color: [0.25, 0.65, 1.0, 1.0],
    effect: [12.0, 0.0, 1.0, 0.0],
})?;
```

For custom Rust records, derive `Copy`, `Clone`, `bytemuck::Pod` and
`bytemuck::Zeroable`, use `#[repr(C)]`, and implement `ShaderInstance::FIELDS`.
Each `InstanceField` describes the exact name, `offset_of!` byte offset and f32
component count of the corresponding WGSL member, including explicit padding.
The author is responsible for truthful field metadata and f32 representations;
`Pod` alone does not establish WGSL layout compatibility. Construction checks
all reflected members, offsets, types, total size and WGSL array stride. There
is no unchecked automatic cast based solely on `repr(C)` or `Pod`.

## WGSL

```wgsl
struct Parameters { color: vec4<f32>, effect: vec4<f32>, }
fn shade(input: ShaderInput, parameters: Parameters) -> vec4<f32> {
    return vec4(parameters.color.rgb, parameters.color.a * input.uv.x);
}
```

The host supplies `ShaderInput`: `uv` (full effect rectangle), `position` and
`size` (logical GPUI pixels), and `scale_factor`. Origin is top-left, positive y
down. The generated vertex stage transports flat instance parameters; the
fragment wrapper clips against the content mask and applies opacity once.
User source supplies functions, constants and types, without entry points,
global variables, overrides or resource bindings. Derivatives and loops are
subject to Naga validation rather than the restricted shader-paint subset.

Output is finite, straight-alpha encoded-sRGB SDR; the wrapper clamps channels
to 0..1. The shader author must avoid non-finite arithmetic. Intensity above one
saturates; this API does not imply HDR or a bloom postprocess. The Metal/D3D11
wide-SDR compositors treat these instances as ordinary SDR UI and convert their
composed scratch image using the existing color path.

## Richer shadows

The opt-in `instanced::ShadowPainter` adds rounded-box shadows with one to four
color stops, a gradient angle, configurable falloff, blur, spread, offset and
inset/outset mode. Native `BoxShadow` retains its existing behavior. Start with:

```sh
just --justfile apps/xgpui-ext-lab/shader-examples/justfile shadows
```

```rust
use xgpui_shader::{painted, instanced::{ShadowPainter, ShadowStyle, ShadowStop}};

// Construct once and retain alongside the view state.
let shadows = ShadowPainter::new()?;
let style = ShadowStyle {
    stops: vec![
        ShadowStop::new(0.0, [0.95, 0.15, 0.45, 0.8]),
        ShadowStop::new(1.0, [0.05, 0.65, 1.0, 0.8]),
    ],
    angle: 90.0,
    blur_radius: px(24.0),
    falloff: 2.0,
    ..Default::default()
};
let layer = shadows.layer(style)?; // Validate once; retain and clone as needed.
let card = painted()
    .rounded(px(16.0))
    .bg(rgb(0xf8fafd))
    .shadow_paint(layer)
    .p_4()
    .child("Normal styled children");
```

`painted().shadow_paint(layer)` handles receiver bounds and paint order:
outsets precede the background; insets follow it and precede children. Repeat
`.shadow_paint(...)` to stack layers. Outsets escape the card's own overflow mask,
while ancestor masks still apply. Card and inherited opacity each apply once.
Normal layout, padding, custom backgrounds, borders and interactive children stay
on the same container. This is an extension API; no GPUI API changes are needed.

Use uniform `.rounded(...)` corners. The helper uses the container's base style
geometry and opacity; state-dependent corner/opacity refinements are not supported.
For custom canvas code, `shadows.paint(window, receiver_bounds, radius, &style)?`
remains available. It expands outset bounds automatically; place it before the
background for outsets, or after the background and before children for insets.

The gradient runs across the receiver: zero degrees points up, 90 degrees right.
Stops are nondecreasing in 0..1; duplicate positions form hard stops. Colors are
straight-alpha sRGB at the API boundary and interpolate in premultiplied sRGB to
avoid transparent-color fringes. One stop gives a solid shadow.

Softness uses a rounded-box signed-distance smoothstep band, followed by a
coverage power (`falloff` 0.25..8; default 1). `blur_radius` is the logical
half-width of that band, not the CSS Gaussian blur radius. Zero blur retains
antialiasing. This provides controllable shadow aesthetics, **not pixel-identical
CSS box-shadow rendering**. Uniform rounded rectangles, four stops and SDR output
are the initial scope; arbitrary silhouettes, per-corner radii, CSS interpolation
spaces and general blur filters are not provided by this helper.

The 128-byte instance uses the same batched main-pass path without a shadow
texture/pass per layer. Tests validate layout and automatic extents, reject
invalid inputs, and check gradient edges, clear centers, inset containment and
falloff changes through both native Metal and wgpu on macOS. The native editor
and WASM companion share the same shadow API and source shader.

## Animated borders

Enable `portable-shaders` and retain an `AnimatedBorder` in view state. Supply
phase in turns from the host clock; the shader draws a rounded ring with two
opposite conic highlights, matching the rotating-gradient technique in CSS.

```rust
use xgpui_shader::{painted, instanced::{AnimatedBorder, AnimatedBorderStyle}};

let border = AnimatedBorder::new()?; // Once, outside render.
// During render: request frames while playing, freeze phase for reduced motion.
window.request_animation_frame();
let layer = border.layer(AnimatedBorderStyle::default(), elapsed_seconds / 2.5)?;
let button_content = painted().rounded(px(999.)).bg(rgb(0xffffff))
    .animated_border(layer).px_6().py_3().child("Generate");
// Wrap in an ordinary div().id(...).on_click(...) for button interaction.
```

Configure logical-pixel width, highlight `softness` (default 3px; zero is crisp),
base track color, two straight-alpha sRGB highlight colors and trail length (fraction of a turn, greater than zero and at most 0.5).
Finite phases wrap; negative phase supports reverse rotation. Colors interpolate
premultiplied to avoid transparent fringes. The neutral track occupies the inside
edge; a Gaussian-shaped highlight halo extends around it (analytic softness, not a general CSS blur filter). Effect
bounds expand by three softness radii; ancestor and container clips still apply.
It paints after the background/inset shadows, before children. Reserve padding for
content; the effect does not change layout. Use uniform base corner radii.
Ancestor clipping and opacity apply through the normal container paint path.
Native hover corner refinements are not supported; rebuild the style explicitly.

The helper does not own a timer or simulate physics. The view supplies phase and
requests animation frames only while active; keep phase constant and stop frame
requests for pause/reduced motion. A spring or pointer simulation can supply the
same phase parameter. No GPUI API changes are required.

The separate local Borders demo (`just --justfile apps/xgpui-ext-lab/shader-examples/justfile borders`)
includes an interactive Generate pill on a light panel and Pause/Resume;
the companion uses the same Rust view and WGSL on WebGPU and WebGL2.

## Limits and transport

- Source: at most 64 KiB. Parameters: at most 128 bytes, only `f32` and
  `vec2/3/4<f32>` members. Rust instances must match the reflected WGSL stride.
- Eleven instance attributes: three host float4 slots and eight payload slots.
  Flat varyings keep the fragment data independent of quad interpolation.
- Transport: 176 bytes/instance. WebGPU/WebGL2 and D3D11 use a reusable growing
  vertex buffer with a 64 MiB cap. Metal uses the existing host instance-buffer
  pool and its frame limits. No per-instance texture or render pass is created.
- Six generated vertices per instance. Batch buffer slices start at instance
  zero, including WebGL2, which lacks base-instance draws.
- Pipelines are device/renderer-owned and retained while referenced by the
  last shader-bearing scene (native-only coverage subpasses do not evict them). Program identity includes the immutable generated sources.
  Replacing a device creates a new renderer cache.
- Effect bounds must include the entire visual. The pulse preset reserves a
  12px inset so glow can extend beyond its inner shape. Clipping does not remap
  local coordinates. Rounded/path masks are not part of this rectangle API;
  implement analytic shape coverage in the shader.

## Examples and validation

Start with the local native app. From the repository root:

```sh
just --justfile apps/xgpui-ext-lab/shader-examples/justfile ui
just --justfile apps/xgpui-ext-lab/shader-examples/justfile composition
just --justfile apps/xgpui-ext-lab/shader-examples/justfile --set profile release particles
```

Inside `apps/xgpui-ext-lab/shader-examples`, use `just ui`; bare `just` lists recipes. The
in-window selector opens all six examples:

1. **Minimal:** a custom WGSL fragment function and typed Rust instance.
2. **Glow:** 100 independently animated badges sharing a program.
3. **Particles:** 100–10,000 instances, count selection, pause and opacity controls.
4. **Composition:** numbered red shader → gold native quad → green shader → blue
   shader legend. An inset cyan frame identifies the clip boundary. **Clipping ON/OFF**
   reveals the full shapes without changing their positions; the opacity toggle
   shows inherited opacity on the same scene. Full shapes fit inside the outer
   canvas, so clipping reads as an explicit comparison rather than a broken layout.
5. **Shadows:** side-by-side native and shader shadow cards with blur, spread,
   offset, custom falloff, solid/gradient color and inset/outset controls.
6. **Borders:** soft orbiting highlights on a light panel, with Generate clicks and pause/resume.

The companion WASM build uses these same Rust views and WGSL sources, including
the selector and improved composition controls. Only platform startup and the
GPU backend differ: native Metal/D3D11 versus browser WebGPU/WebGL2. Rebuild WASM
after source changes and reload; native and browser binaries do not update each
other automatically. The existing extensions-web gallery remains separate.

```sh
bash apps/xgpui-ext-lab/shader-examples/build.sh --release
python3 apps/xgpui-ext-lab/shader-examples/serve.py
```

Open `http://localhost:8083/`. The build writes
`target/web/shaders/`; `just docs-serve` and the Pages workflow stage
that companion at `target/site/examples/shaders/` with the website. See the
[demo source and run guide](https://github.com/gpui-luma-dev/xgpui/tree/main/apps/xgpui-ext-lab/shader-examples).

```sh
cargo test --offline --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --target-dir target instanced::
```

Tests cover preset translation, ABI rejection, invalid resources/data, compatible
batch runs, program/native boundaries, and replay. On macOS, a Metal and wgpu
pixel test exercises multiple instances, clipping, opacity and interleaved native
geometry. Shader translation and cross-compilation do not establish D3D11 runtime
correctness. Redraw intervals in the demo include scheduling and are not GPU
execution timings; use platform GPU captures for draw-count/timing acceptance.

## Explicit wide-SDR output

`ShaderProgram::new` retains straight-alpha encoded-sRGB SDR semantics.
`ShaderProgram::new_linear_p3` instead declares bounded straight-alpha linear
Display P3/D65 producer output; `new_linear_rec2020` declares bounded linear
Rec.2020/D65 SDR output. All wrappers clamp producer RGBA to [0,1]; use
explicit gamut mapping inside the producer when clipping is unsuitable.

Enable `WindowColorMode::WideGamutSdr` outside painting before recording a wide
program. `paint_custom_shader` rejects a wide program in an ordinary window;
renderers also reject unsupported scenes before preparation/submission. wgpu
and non-macOS Apple rendering currently reject wide programs explicitly.

The generated wrapper applies inherited opacity and associates RGB once. On
Metal, wide instances convert to an extended linear-P3 Float16 target with
premultiplied blending. Rec.2020 can produce negative P3 channels, which survive
captures, linear filters and normal blending. On D3D11, the host converts the
declared producer primaries to
linear scRGB before association; negative and above-one scRGB survive because
producer clamping happens before that conversion. No per-instance offscreen
producer texture is allocated. Legacy UI runs still require conversion, and
presentation still has its existing overhead.

Output meaning lives in immutable `SceneShaderProgram` metadata. Unique program
IDs include that choice by construction; cached Metal pipelines therefore keep
compatible pixel formats and blend state across mode changes. Wide instances
batch by program identity within scene ordering rules. The compositor flushes
pending legacy UI before a wide shader run and resumes legacy composition in
order afterward. Clip bounds, per-instance opacity and paint replay retain
existing behavior. Analytical shape coverage remains the shader's responsibility.

The gamut-owned native regression (`xgpui-gamut/tests/instanced_output.rs`) compares instanced P3 fills against typed `WideColor`
producers with translucent overlapping fills, ordinary shaders, native controls,
clipping, resize, replay and mode changes. It executes on Metal locally; the
same test is wired into the Windows WARP workflow. Windows cross-compilation
alone does not validate native HLSL execution or physical display fidelity.


Color conversion, mapping and interpolation math live independently in
[`xgpui-color`](color-math.md).
`xgpui-shader` has no runtime or development dependency on gamut or color.
Cross-extension comparisons belong to gamut's development tests. Its existing
producer has no default runtime shader dependency; the `instanced` feature adds
optional wide fill/gradient adapters.

For low-level scene adapters, `ShaderProgram::scene_program()` returns an `Arc`
to its immutable compiled `SceneShaderProgram`. Raw `ShaderDraw` parameter slots
must obey the program's validated instance ABI; ordinary applications should
continue using `WindowShaderExt::paint_custom_shader` for validated packing.


## Reusable wide fills and gradients

Enable `xgpui-gamut`'s optional `instanced` feature. `WidePainter` compiles a shared
program once and paints bounded `WideColor` solids or `WideGradient` two-stop
linear gradients. No new dependency is added to `xgpui-shader`.

```rust
use xgpui_gamut::{color::*, instanced::{WideGradient, WidePainter}};

let painter = WidePainter::new()?;
let red = LinearRgba::new(LinearRgb::new(RgbSpace::DisplayP3, [1., 0., 0.])?, 1.)?;
let blue = LinearRgba::new(LinearRgb::new(RgbSpace::DisplayP3, [0., 0., 1.])?, 0.5)?;
let gradient = WideGradient::linear(
    [0., 0.], [1., 0.], [red, blue],
    ColorInterpolation::Oklab, GamutMapping::ReduceChroma,
)?;
// During a canvas/element paint callback in an enabled WideGamutSdr window:
painter.paint_gradient(window, bounds, gpui::px(16.), &gradient)?;
```

Start/end coordinates are normalized to fill bounds. Radius is a uniform logical
pixel value clamped to half the shorter dimension. Host clipping/opacity and
analytic antialiasing combine once. Gamut mapping follows interpolation before
coverage/output association. Gradient uploads are bounded to finite magnitudes
<=65504, including the precomputed axis projection coefficients. Unrepresentable
or extremely short axes return errors; coincident endpoints select the second stop.
The shared 80-byte parameter ABI fits the existing 128-byte transport.

The native gradient reference test covers six interpolation/hue choices, both
mapping policies, endpoint extension, degenerate axes, rounded coverage, masks and
opacity against CPU-derived expected pixels. A scene test verifies 5,000 instances
form one ordered shader run with no custom producer draws.

Run `gamut-lab --instanced` for solids, or `gamut-lab --instanced --gradient` for
OkLab gradients. Omit the flags to compare the existing offscreen producer path.
`--sdr` retains the solid sRGB baseline. Captures are clipped sRGB previews.

A manual allocation/timing test is available:

```sh
cargo test --locked --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-gamut/Cargo.toml --features instanced --lib compare_producer_allocations_and_frame_readback -- --ignored --nocapture --test-threads=1
```

This test counts actual producer texture allocations, warms the renderer, and
reports three-frame averages at 512×512 for 1 and 16 8×8 solid fills on both
paths, plus 1,000 instanced fills. The larger legacy preparation run stalled on
this Metal host, so the baseline is capped rather than assigned a timing.
Producer pass/fill draw counts follow the recorded path; timing includes CPU
encoding, GPU completion and readback, not isolated GPU timestamps. Run it alone
so the test-only allocation counter is scoped. Physical display measurements and
native Windows execution remain separate acceptance work.


Local debug measurement on Apple M4 Max, 512×512 target, three measured frames
after warmup (milliseconds include encode, completion and readback):

| Fills | Path | Producer textures/frame | Fill draws/frame | Mean frame ms |
| --- | --- | --- | --- | --- |
| 1 | Offscreen | 1 | 1 | 11.531 |
| 1 | Instanced | 0 | 1 | 11.359 |
| 16 | Offscreen | 16 | 16 | 11.997 |
| 16 | Instanced | 0 | 1 | 11.326 |
| 1,000 | Instanced | 0 | 1 | 11.948 |

The allocation/draw reduction is directly exercised. Readback dominates these
small frames; this sample does not establish isolated GPU speed or a frame-rate
guarantee. The legacy 100-fill run stalled during preparation and was stopped. The
renderer creates its Metal queue with the default capacity of 64 outstanding
command buffers. Each legacy WideColor preparation records one uncommitted
buffer, while the host submits only after all producers are prepared; allocation
of the next buffer can block before submission is reached. See
[Apple's queue capacity contract](https://developer.apple.com/documentation/metal/mtldevice/makecommandqueue%28%29?changes=_11&language=objc).
The instanced path records no per-producer buffers and avoids this cycle.
Changing legacy preparation/submission semantics is deferred; no timing is
inferred for the stalled case.


### Native P3 comparison companion

Use [color-lab](high-gamut-windows.md#compare-srgb-and-native-display-p3) to compare
`WidePainter` P3 swatches with ordinary sRGB UI colors using the same CPU reference
samples as the browser Color / Gamut demo. The native header reports effective
window mode and display capability. The browser version maps to sRGB and reports
its actual canvas color space separately from advertised display gamut; see
[runtime diagnostics](color-math.md#runtime-output-diagnostics-and-native-comparison).
