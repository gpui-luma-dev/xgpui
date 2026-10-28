# xgpui-gamut

Opt-in **macOS Metal / Windows D3D11** presentation: wide [SDR ↗](https://en.wikipedia.org/wiki/Standard-dynamic-range_video) surfaces and normal
[sRGB ↗](https://en.wikipedia.org/wiki/SRGB) controls in one window. Windows integration is cross-compiled here; native
WARP/desktop execution and physical display checks remain required. Linux, [HDR ↗](https://en.wikipedia.org/wiki/High_dynamic_range)
content and transparent wide windows remain deferred.

## GPUI public API changes: additions and compatibility

This crate requires per-window color request/status APIs and typed wide-SDR
backend outputs. `PlatformWindow` supplies default request/report methods, so
existing platform implementations need no required overrides. Default windows
and legacy sRGB paints retain their behavior; additive method names can still
collide with downstream extension methods.

It reuses the custom-content recording changes: complete `Scene` literals must
include `custom_draws`, and exhaustive `Primitive` / `PrimitiveBatch` matches
must handle `Custom`. Window-option structs and existing ordinary output types
retain their contracts. New color configuration/fallback enums are non-exhaustive.

**[GPUI public API changes: additions and compatibility](gpui-public-api.md#added-opt-in-window-presentation).**
See also [shared structural compatibility risks](gpui-public-api.md#altered-existing-public-types--source-compatibility-risks)
and [backend APIs](gpui-public-api.md#added-backend-crate-public-apis).

```toml
# App under apps/:
[dependencies]
xgpui-gamut = { path = "../../extensions/xgpui-ext/crates/xgpui-gamut" }
xgpui-shader = { path = "../../extensions/xgpui-ext/crates/xgpui-shader" }
```

```rust
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_gamut::WideColor;
use xgpui_shader::painted;

// During window construction/update, outside drawing:
window.set_color_mode(WindowColorMode::WideGamutSdr)?;

// Linear Display P3/D65, straight input alpha; producer stores associated alpha.
let red = Arc::new(WideColor::linear_p3([1.0, 0.0, 0.0, 0.5])?);
painted().w(px(320.0)).h(px(240.0))
    .bg_paint(red)
    .child(div().p_4().child("Normal sRGB controls"));
```

The default `xgpui-gamut` producer is usable without `xgpui-shader`;
`xgpui-shader` supplies optional `painted()` ergonomics here. Enabling gamut's
`instanced` feature adds a shader dependency for `WidePainter`/`WideGradient`.
Shared CPU math lives in the standalone `xgpui-color` crate. `WideColor` retains
its texture preparation path; instanced fills avoid producer textures/passes.
See [wide fills and gradients](instanced-wgsl-v2.md#reusable-wide-fills-and-gradients).

| Capability | Owner | Status |
| --- | --- | --- |
| Window mode and cached status | `gpui` | Public, opt-in |
| Typed P3 output and ordered compositor | `gpui-apple` / `gpui-macos`, `gpui-windows` | Metal + native D3D11 |
| Reusable producer | `xgpui-gamut::WideColor` | Metal + typed D3D11 preparation |
| Test/control app | `apps/xgpui-ext-lab/gamut-lab` | Uses `xgpui-gamut` |

Independent of the [paint/shader extension](extension-paints.md). Enabling this
mode does not reinterpret existing gradients, meshes, colors or controls as P3.

## Configure one window

```rust
use gpui::{Window, WindowColorMode};

fn enable_wide_content(window: &mut Window) -> anyhow::Result<()> {
    let status = window.set_color_mode(WindowColorMode::WideGamutSdr)?;
    assert_eq!(status.requested(), WindowColorMode::WideGamutSdr);
    Ok(())
}

// Inspect a cheap cached snapshot:
let status = window.color_status();
let requested = status.requested();
let effective = status.effective();
let supported = status.supports_wide_gamut_sdr();
let display = status.display_wide_gamut(); // Option<bool>: None means unknown.
let fallback = status.fallback();

// Disable before switching to a transparent top-level window:
window.set_color_mode(WindowColorMode::Default)?;
```

Call during `open_window` construction or an event/update outside drawing.
Unsupported requests fail with the previous configuration intact. Each window
starts in `Default`; enabling one window never changes another.

```text
controls        Rgba / Hsla / Style retain encoded-sRGB meaning
wide content    explicit linear Display P3/D65, associated alpha
composition     ordered legacy UI runs; Metal linear P3 / Windows linear scRGB
presentation    Metal tagged P3 / Windows tagged scRGB; bounded SDR content
```

The report describes negotiated configuration, not proof of physical monitor
output. macOS retains the P3-tagged target and delegates display mapping to
ColorSync, including SDR monitors; it does not report a fictitious [framebuffer ↗](https://en.wikipedia.org/wiki/Framebuffer)
fallback. Display-profile evidence is sampled at frame boundaries, with screen and
same-screen profile notifications scheduling refreshes. A window spanning displays
cannot promise identical physical output everywhere.

| Operation | Behavior |
| --- | --- |
| `set_color_mode(WideGamutSdr)` | Stage compatible pipelines/layer configuration, then refresh. Size-dependent allocation remains fallible at draw/resize. |
| Same mode again | Reuse compatible pipelines. |
| Setter during drawing | Error before changing platform state. |
| Unsupported backend or transparent window | Error; previous configuration/status stays valid. |
| `Default` | Restore legacy presentation; queued commands retain their resources. Subsequent wide outputs are rejected. |
| Transparency requested while wide | The existing infallible background setter preserves the opaque window and logs a warning. Disable wide mode first. |

## Metal backend contract

`WideColor` implements the existing `CustomContent` protocol. Its Metal adapter
uses the typed constructor below; custom producers can use the same contract.

```rust
use gpui_apple::custom_draw::{MetalCustomContext, MetalWideSdrOutput};

// Backend-owned context, RGBA16Float texture and unsubmitted commands:
let output = MetalWideSdrOutput::from_linear_p3_premultiplied(
    context, texture, commands,
)?;
```

```text
texture          RGBA16Float, 2D, single-sample, shader-readable
color            linear Display P3/D65; straight SDR RGB and alpha in 0..1
association      stored RGB = straight RGB * alpha; zero RGB at zero alpha
device / queue   retained host identities; host alone submits/presents
geometry         full local source, exact requested raster extent
host             applies coverage, clipping and inherited opacity once
```

Constructor validation checks texture/device/queue/command structure; the host
revalidates the requested extent and complete batch before submission. Pixel color
and alpha correctness remain an in-process producer contract; validating handles
cannot prove generated pixel values.

Cache immutable output by retained queue/device, exact extent and content revision:

```rust
// `cached` is a retained MetalWideSdrOutput.
let output = cached.reusable_for(context)?;
Ok(Box::new(output))
```

| Cached command state | Reuse |
| --- | --- |
| Pending | Same texture and command identities; host deduplicates submission. |
| Committed/scheduled/completed | Same texture, no pending commands; same-queue ordering supplies readiness. |
| Enqueued/error, different queue/device | Error. |

No CPU wait. Like the legacy protocol, host-only submission is cooperative:
command status cannot identify who committed a same-queue buffer. Empty commands
at construction require content already ready/submitted on the host queue.
Display moves change host mapping, not canonical producer content. Future
source/profile decoding and additional `xgpui-gamut` producers are separate work.

## Existing producer compatibility

```text
Background               → existing native adapter             → sRGB UI run
MetalCustomOutput        → existing straight sRGB / BGRA8Unorm → sRGB UI run
MetalWideSdrOutput        → associated linear P3 / Float16      → wide composition
unknown output           → explicit preparation error
```

Existing producers implement no new methods and keep their current output
interpretation. Existing mesh/gradient/WGSL content does not become P3 implicitly.
Wide output requires an accepted `WideGamutSdr` request; no texture-format guesses,
process switch or tuple tag. Recording validation, batch submission and scene
ordering are shared with legacy producers.

## Metal compositing contract

```text
scene: UI background → wide A → controls → wide B → tooltip

1. legacy renderer → transparent sRGB scratch → convert/composite
2. wide A         → linear P3 composition target
3. legacy renderer → cleared scratch         → convert/composite
4. wide B         → linear P3 composition target
5. legacy renderer → cleared scratch         → convert/composite
6. tagged window presentation
```

A UI run is a maximal consecutive sequence of legacy draws between wide draws.
Scratch is consumed before clearing/reusing it, in GPU command order. Do not
flatten all UI into a top overlay. Native overlays, surface children, paths,
masking and replay retain scene order.

```text
// UI scratch: associated encoded sRGB, aligned texel reads.
A       = sampled alpha
C       = A > 0 ? sampled RGB / A : 0
L       = srgb_decode_piecewise(C)
P       = linear_srgb_to_linear_p3(L)
source  = (A * P, A)

// Wide output is already associated linear P3; filter in that representation.
k       = host coverage * inherited opacity
src     = (k * source.rgb, k * source.a)
out.rgb = src.rgb + dst.rgb * (1 - src.a)
out.a   = src.a   + dst.a   * (1 - src.a)
```

Opaque panels retain legacy blending within each run. Translucent controls over
wide content use linear composition; exact legacy blending across that boundary
is not promised. Flattening a transparent run loses individual backdrop
interactions. Grayscale text coverage is supported; backdrop-reading effects and
LCD/subpixel text require another contract. macOS Metal's subpixel batch remains
unsupported. Low-alpha pixels are tested without an arbitrary epsilon cutoff.

## API surface

```rust
// GPUI core: additive methods; no new window-option fields.
impl Window {
    pub fn set_color_mode(&mut self, mode: WindowColorMode)
        -> anyhow::Result<WindowColorStatus>;
    pub fn color_status(&self) -> WindowColorStatus;
}

// PlatformWindow: same pair, with existing-mode/unsupported defaults.
// MetalRenderer: same pair; required by the gpui-macos / gpui-apple crate split.
```

| Type | Public contract |
| --- | --- |
| `WindowColorMode` | Non-exhaustive `Default` / `WideGamutSdr`; `Default` preserves existing behavior. |
| `WindowColorStatus` | Private fields; constructor for backends; requested/effective/support/display/fallback getters. Copyable snapshot. |
| `WindowColorFallback` | Non-exhaustive `SdrDisplay` / `PlatformPolicy`; current Metal output remains OS-managed P3. |
| `MetalWideSdrOutput` | Opaque output; explicit constructor and `reusable_for`. |

`WindowOptions`, `WindowParams`, native colors/styling, `CustomContent`,
`PreparationRequest`, `CustomDraw` and primitive enums are unchanged by this
milestone. Default trait methods preserve platforms without overrides. Additive
method names can still collide with downstream extension methods.

## Compare sRGB and native Display P3

The desktop color companion uses `WidePainter` to display native P3 swatches
beside ordinary mapped sRGB swatches. Both use the same 32 CPU reference samples
as the browser Color / Gamut demo: encoded sRGB, linear RGB, OkLab and shorter-hue
OkLCh. Change endpoint presets, gamut mapping and end alpha interactively.

```sh
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-lab
```

Move the window to your P3 monitor and check `Effective: WideGamutSdr` and
`Display wide gamut: Some(true)`. `None` means display capability is unknown.
macOS Metal runtime presentation was checked; Windows cross-compilation passes,
while Windows native execution and physical display acceptance remain separate.
The status reports negotiation and capability, not calibrated color accuracy.

The [browser comparison and runtime diagnostics](color-math.md#browser-color--gamut-lab)
remain an sRGB preview. A P3-capable display alone does not change its output:
WebGL reads `drawingBufferColorSpace`, and WebGPU queries its canvas configuration
where supported. Unavailable queries report unknown. Screenshots cannot prove
physical P3 output.

## Run gamut-lab

```sh
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile ui
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile ui --instanced
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile ui --instanced --gradient
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile baseline
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile headless --frames 60
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile capture
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile metal-test
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile test
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile lint
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile ui --ui-png /tmp/gamut-lab-ui.png
```

On Windows (PowerShell), no `just` installation required:

```powershell
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab -- --sdr
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab -- --headless --frames 3 --png gamut-lab-preview.png
cargo test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab --test compatibility
cargo test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab --test native_windows
```

`--headless` now captures the full lab UI through a hidden native window on both
platforms. It requires a desktop session and measures encoding plus readback on
every iteration. Backend WARP tests remain independent of a visible desktop.
`--sdr` and the presentation button compare the native sRGB baseline with P3
surfaces. Overlay, opacity, backdrop and ordering controls are shared.

```sh
# After pushing the branch: build on GitHub-hosted Windows runners.
gh workflow run windows-apps.yml --ref codex/high-gamut-windows
gh run list --workflow windows-apps.yml
gh run download RUN_ID --name gamut-lab-windows-x64
# On Windows, launch the downloaded artifact:
# .\gamut-lab.exe
```

The **Windows lab applications** workflow also builds `paint-lab`,
compiles their tests and uploads separate x64 release executables. It runs on
relevant pull requests or manual dispatch. Download builds from the Actions run's
artifacts. It does not launch desktop windows or establish physical gamut fidelity.
The separate **Portable paint GPU checks** workflow runs the offscreen WARP suite.
Windows app builds have passed; interactive Windows behavior has also been checked locally by the app developer. Physical color measurement remains separate.

```text
controls    overlays, opacity, light/dark backdrop, reverse surface order
content     two P3 patches; native labels/buttons/panels
headless    hidden native window; same UI and producers; encode + readback
capture     explicit clipped sRGB preview; not a wide-gamut export
```

`MetalRenderer` supports the same negotiation for headless checks. Raw Float16
pixel diagnostics remain in backend tests; no generic headless trait or image
API expansion. Backend tests cover queued scratch reuse, P3 preservation, low-alpha
edges, masks/order/resize, rejection before submission, mode switches, legacy
outputs and queue-scoped pending/submitted reuse. Native tests create separate
native platform windows and verify independent requests and transparency rejection. Compile
checks preserve complete window-option literals and unchanged platform defaults.

## Limits and acceptance

- Full-viewport conversion per UI run and an extra Float16 presentation copy.
  Lab patch producers allocate on preparation; pooling/GPU timing remain future work.
- Scratch + composition targets are capped at 256 MiB per live pair. This excludes
  drawable, producer, path/atlas and in-flight retention. Float16 is eight bytes/pixel.
- Physical display/profile comparisons and device-loss stress remain outstanding.
  An sRGB screenshot cannot prove physical wide-gamut output.
- Source/image profiles, RAW/video, HDR/EDR and Linux presentation remain separate
  milestones. Windows native execution and physical display validation remain
  outstanding; cross-compilation alone is not acceptance.

[GPUI API delta](gpui-public-api.md) · [Paint integration](extension-paints.md) ·
[Preparation/recording](paint-contract-hardening.md) ·
[Apple Display P3](https://developer.apple.com/documentation/coregraphics/cgcolorspace/displayp3) ·
[Layer color-space configuration](https://developer.apple.com/documentation/quartzcore/cametallayer/colorspace)

## Windows swap-chain presentation

```rust
// Existing GPUI API; no new window options, color types or scene variants.
window.set_color_mode(WindowColorMode::WideGamutSdr)?;
let status = window.color_status(); // cached; does not query DXGI
let patch = Arc::new(WideColor::linear_p3([1.0, 0.0, 0.0, 0.5])?);
painted().bg_paint(patch).child(div().child("Native sRGB controls"));

// Disable before requesting transparency, blur or a Mica backdrop:
window.set_color_mode(WindowColorMode::Default)?;
```

```text
real Scene::batches(), including each custom draw within a batch:
  native UI + BGRA8/sRGB custom paints → transparent BGRA8 scratch
  flush run → unpremultiply encoded RGB → sRGB decode → reassociate
  typed P3 draw → P3-to-scRGB → coverage/opacity once → source-over
  repeat in scene order; clear scratch only after consuming it
  final FP16 scRGB target → RGB * SDR-white scale, alpha unchanged → Present
```

| Resource / policy | Windows |
| --- | --- |
| Flip swap chain | `R16G16B16A16_FLOAT`, `RGB_FULL_G10_NONE_P709`; DirectComposition and HWND paths |
| Producer color | Linear Display P3/D65, associated alpha; identical to Metal input |
| Composition | Linear [scRGB ↗](https://en.wikipedia.org/wiki/ScRGB); retain negative and above-one RGB for saturated SDR colors |
| UI | Existing colors, themes and legacy paints retain encoded-sRGB meaning |
| Text | Existing capability hook requests grayscale; cached LCD tiles collapse to scalar coverage |
| Coverage | Native border/path masks, corners, clipping and inherited opacity applied once |
| White | HDR desktop: `DISPLAYCONFIG_SDR_WHITE_LEVEL / 1000`; SDR desktop: scale 1 |
| Capture | Explicit clipped-sRGB preview of the canonical target, without display-white scaling |
| Allocation | FP16 + [BGRA ↗](https://en.wikipedia.org/wiki/RGBA_color_model) frame targets ≤256 MiB; producer outputs separately ≤64 MiB; excludes paths/atlas/replacement peaks/in-flight resources |

```text
linear P3 → scRGB:
R =  1.22494018 * r - 0.22494018 * g
G = -0.04205695 * r + 1.04205695 * g
B = -0.01963755 * r - 0.07863605 * g + 1.09827360 * b
```

| Lifecycle | Behavior |
| --- | --- |
| Enable | Preflight FP16 blend/sample support, pipelines, targets and display white; release back-buffer references; change format and set [color space ↗](https://en.wikipedia.org/wiki/Color_space) |
| Same mode | Preserve compatible pipelines |
| Shared device | Serialize mode/resize updates against cross-window drawing; defer re-entrant resize messages to the next frame |
| Recoverable transition failure | Restore previous configuration; return error |
| Rollback also fails | Preserve prior intent/status, pause drawing and request existing GPU recovery |
| Resize | Preserve active format; reacquire views and reapply scRGB tag |
| Device replacement | Drop device-bound outputs/pipelines, restore requested mode and pending size, force scene/atlas rebuild |
| Transparent/backdrop window | Reject enabling; while wide, preserve opaque appearance and log a warning |
| Profile/monitor/white changes | Dirty on move/size/DPI/display/settings messages; resample at frame boundaries, with a one-second fallback on requested frames |
| Missing HDR white evidence | Defer presentation until a valid snapshot is available; no guessed brightness |

Choose the output with the largest window intersection. Report only conservative
positive wide-gamut evidence from display colorimetry; ambiguous metadata stays
`None`. Negotiated scRGB does not prove physical delivery. DWM may clip output on
an ordinary display; no host framebuffer fallback is reported unless performed.
A spanning window uses one SDR-white reference and cannot promise identical
physical output across displays. No HDR mode is enabled by the app.

```sh
# Native Windows: WARP pixels, producer transactions and presentation policy.
cargo test --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-windows/Cargo.toml \
  --features test-support --lib

# Requires an interactive Windows desktop: both flip-chain paths,
# toggles/resize/isolation, retained-buffer failure and device replacement.
cargo test --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-windows/Cargo.toml \
  --features test-support --lib directx_renderer::color_presentation \
  -- --ignored --test-threads=1

# Portable CPU oracle only; does not execute HLSL or D3D11.
rustc --edition 2024 --test \
  gpui/crates/gpui-windows/src/directx_renderer/wide_sdr/reference.rs \
  -o /tmp/xgpui-wide-sdr-reference-tests
/tmp/xgpui-wide-sdr-reference-tests
rustc --edition 2024 --test \
  gpui/crates/gpui-windows/src/directx_renderer/color_presentation/policy.rs \
  -o /tmp/xgpui-color-policy-tests
/tmp/xgpui-color-policy-tests
```

The manual **Portable paint GPU checks** workflow includes WARP composition,
transaction/policy tests and `xgpui-gamut` checks. Desktop fixtures remain explicitly
ignored in unattended runs. HLSL compiles during native execution, not Rust
cross-compilation. `gamut-lab` shares its UI and capture path across macOS and Windows; the backend fixtures
exercise the renderer directly. Physical HDR/WCG/ordinary-display comparisons,
monitor moves, live SDR-white changes and display-loss stress remain outstanding.

## Typed Windows output

```rust
use anyhow::{anyhow, Result};
use gpui::{CustomContent, DevicePixels, Size};
use gpui_windows::custom_draw::Dx11CustomContext;
use std::any::Any;

#[derive(Debug)]
struct WideRed;
impl CustomContent for WideRed {
    fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>)
        -> Result<Box<dyn Any>>
    {
        let ctx = backend.downcast_ref::<Dx11CustomContext>()
            .ok_or_else(|| anyhow!("native D3D11 required"))?;
        let output = ctx.create_wide_sdr_output(extent)?;
        ctx.record_wide_sdr(&output, |deferred, target| {
            // Associated linear P3: red at 50% alpha, not encoded sRGB.
            unsafe { deferred.ClearRenderTargetView(target, &[0.5, 0.0, 0.0, 0.5]); }
            Ok(())
        })?;
        Ok(Box::new(output)) // host alone finishes and executes the recording
    }
}
```

```rust
// In a producer cache, scoped by retained host context, extent and revision:
if let Some(output) = cached.as_ref().filter(|o| ctx.can_reuse_wide_sdr(o)) {
    return Ok(Box::new(output.clone()));
}
```

| Contract | Windows output |
| --- | --- |
| Type | Opaque, cloneable `Dx11WideSdrOutput`; `texture()` exposes the host texture |
| Storage | Host-allocated `R16G16B16A16_FLOAT`, one mip/sample, 2D, sampleable/renderable |
| Color | Finite linear Display P3/D65; straight RGB/alpha in 0..1, stored associated; RGB zero at alpha zero |
| Allocation | 1..16384 each dimension; 64 MiB per output, accounting for eight bytes/pixel |
| Reuse | Local pending, same-batch staged, or submitted on the same retained host context |
| Writes | `record_wide_sdr` accepts only a fresh output from the current recording |
| Failure | Recording/validation failure or dropped batch abandons escaped outputs and discards unsubmitted work |
| Host | Existing `PreparationBatch`; exact extent/device/type/texture validation before submission |
| Legacy | `Dx11CustomOutput` remains [straight-alpha ↗](https://en.wikipedia.org/wiki/Alpha_compositing) BGRA8/sRGB; types are never inferred from format |

Raw D3D handles and pixel values remain a cooperative in-process contract. Typed
output is available in the backend SDK and accepted by the native Windows renderer
when `WideGamutSdr` is active. Default Windows rendering rejects it and discards
its recording. GPUI core APIs and existing producer signatures are unchanged.

```sh
# Native Windows recording lifecycle and pixel tests:
cargo test --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-windows/Cargo.toml \
  --features test-support --lib custom_draw::wide_output
```
