# xgpui-gamut

Opt-in **macOS Metal** presentation: wide SDR surfaces and normal sRGB controls in
one window. HDR, transparent wide windows and other presentation backends remain deferred.

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

The crates are independent: `xgpui-gamut` produces explicit P3 content;
`xgpui-shader` supplies optional `painted()` ergonomics here. Neither depends on
the other. `WideColor` allocates on preparation; pooling/caching is deferred.

| Capability | Owner | Status |
| --- | --- | --- |
| Window mode and cached status | `gpui` | Public, opt-in |
| Typed P3 output and ordered compositor | `gpui-apple` / `gpui-macos` | Native macOS Metal |
| Reusable producer | `xgpui-gamut::WideColor` | Metal + typed D3D11 preparation |
| Test/control app | `apps/gamut-lab` | Uses `xgpui-gamut` |

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
composition     ordered legacy UI runs + linear P3 source-over
presentation    tagged linear P3 RGBA16Float, SDR white = 1, EDR off
```

The report describes negotiated configuration, not proof of physical monitor
output. macOS retains the P3-tagged target and delegates display mapping to
ColorSync, including SDR monitors; it does not report a fictitious framebuffer
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

## Compositing contract

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

## Run gamut-lab

```sh
just --justfile apps/gamut-lab/justfile ui
just --justfile apps/gamut-lab/justfile baseline
just --justfile apps/gamut-lab/justfile headless --frames 60
just --justfile apps/gamut-lab/justfile capture
just --justfile apps/gamut-lab/justfile metal-test
just --justfile apps/gamut-lab/justfile test
just --justfile apps/gamut-lab/justfile lint
just --justfile apps/gamut-lab/justfile ui --ui-png /tmp/gamut-lab-ui.png
```

```text
controls    overlays, opacity, light/dark backdrop, reverse surface order
content     two P3 patches; native labels/buttons/panels
headless    renderer.set_color_mode(WideGamutSdr); no global activation
capture     explicit clipped sRGB preview; not a wide-gamut export
```

`MetalRenderer` supports the same negotiation for headless checks. Raw Float16
pixel diagnostics remain in backend tests; no generic headless trait or image
API expansion. Backend tests cover queued scratch reuse, P3 preservation, low-alpha
edges, masks/order/resize, rejection before submission, mode switches, legacy
outputs and queue-scoped pending/submitted reuse. Native tests create separate
AppKit windows and verify independent requests and transparency rejection. Compile
checks preserve complete window-option literals and unchanged platform defaults.

## Limits and acceptance

- Full-viewport conversion per UI run and an extra Float16 presentation copy.
  Lab patch producers allocate on preparation; pooling/GPU timing remain future work.
- Scratch + composition targets are capped at 256 MiB per live pair. This excludes
  drawable, producer, path/atlas and in-flight retention. Float16 is eight bytes/pixel.
- Physical display/profile comparisons and device-loss stress remain outstanding.
  An sRGB screenshot cannot prove physical wide-gamut output.
- Source/image profiles, RAW/video, HDR/EDR and Windows/Linux wide presentation are
  separate milestones. Other backends report unsupported opt-in; legacy UI works.

[GPUI API delta](gpui-public-api.md) · [Paint integration](extension-paints.md) ·
[Preparation/recording](paint-contract-hardening.md) ·
[Apple Display P3](https://developer.apple.com/documentation/coregraphics/cgcolorspace/displayp3) ·
[Layer color-space configuration](https://developer.apple.com/documentation/quartzcore/cametallayer/colorspace)

## Windows headless checkpoint

```sh
# Native Windows: compiles HLSL at runtime and runs WARP pixel assertions.
cargo test --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-windows/Cargo.toml \
  --features test-support --lib directx_renderer::wide_sdr

# Portable CPU oracle only; does not execute HLSL or D3D11.
rustc --edition 2024 --test \
  gpui/crates/gpui-windows/src/directx_renderer/wide_sdr/reference.rs \
  -o /tmp/xgpui-wide-sdr-reference-tests
/tmp/xgpui-wide-sdr-reference-tests
```

```text
private test-only layer list:
  native Scene run → associated encoded-sRGB BGRA8 scratch → decode → scRGB
  typed P3 producer → validate/stage/submit → P3-to-scRGB → associated source-over
  repeat in supplied order → offscreen RGBA16Float readback
```

| Checkpoint | Scope |
| --- | --- |
| Color | Piecewise sRGB decode; P3 primaries/neutral; preserve negative and above-one scRGB channels |
| Alpha | Associated source-over, opacity once, zero/tiny alpha, light/dark backgrounds |
| Ordering | UI between surfaces, tooltip above both, scratch clearing, repeated/reversed frames |
| Text | Native grayscale tiles; cached LCD RGB coverage collapsed to scalar alpha before correction/blending |
| Isolation | Validate extent/budget/device/opacity before encoding; restore legacy text pipeline |

Cached LCD coverage conversion is a compatibility path, not fresh grayscale glyph
rasterization. Native scenes retain GPUI batch order, masks and transforms. The
private caller supplies UI/surface boundaries; mixed-scene partitioning,
filtering beyond nearest texels, swap-chain color
negotiation, display/SDR-white handling and window integration remain deferred.
The accepting compositor is compiled only for backend tests; Windows window requests remain
unsupported. CPU tests and cross-compilation do not establish GPU pixel results;
native WARP execution is required.

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
| Legacy | `Dx11CustomOutput` remains straight-alpha BGRA8/sRGB; types are never inferred from format |

Raw D3D handles and pixel values remain a cooperative in-process contract. Typed
output is available in the backend SDK; only the private test compositor accepts
it currently. Default Windows scene rendering rejects it and discards its
recording. No window, swap-chain, or GPUI core API change is included.

```sh
# Native Windows recording lifecycle and pixel tests:
cargo test --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-windows/Cargo.toml \
  --features test-support --lib custom_draw::wide_output
```
