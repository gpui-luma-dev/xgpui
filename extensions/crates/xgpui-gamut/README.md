# xgpui-gamut

Wide-gamut color extension for **xgpui**. Draw explicit linear Display P3 SDR
content alongside ordinary sRGB controls, without changing the color meaning
of existing UI paints.

## Dependencies

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.2.0" }
xgpui-gamut = "0.2.2"
```

## Enable wide content

Request wide-gamut SDR on the window outside drawing, then use a `WideColor`
producer. `xgpui-shader` supplies optional `painted()` container ergonomics:

```rust,ignore
use gpui::{prelude::*, *};
use xgpui_gamut::WideColor;
use xgpui_shader::painted;

window.set_color_mode(WindowColorMode::WideGamutSdr)?;
let red = WideColor::linear_p3([1., 0., 0., 0.5])?;

painted()
    .w(px(320.)).h(px(240.))
    .bg_paint(red.content())
    .child("Normal sRGB controls")
```

Add `xgpui-shader = "0.3.0"` for that container example. The default producer can
also be used without the shader extension. Inspect the window's reported color
status; requesting wide output is not proof that the display is presenting it.

## Instanced fills and gradients

Enable `instanced` for `instanced::WidePainter` and `WideGradient`. This feature
adds a dependency on `xgpui-shader` and draws supported solids/gradients directly
in the host render pass. `WideColor` is a CPU-only `ContentSource`; `content()` returns a retained portable
`Content`. The framework owns materialization, caching and completion. Clones
preserve content identity; equivalent solids can share one realization. Input is
straight linear P3; the description associates RGB exactly once and declares
premultiplied RGBA16Float output. There are no native device/queue dependencies
in the default producer.

Without the shader extension, use `window.paint_content(geometry,
ContentDraw::new(red.content()).slot(slot))`. Retain a `PresentationSlotId` per
logical paint role to preserve previous valid content during replacement.
`painted().bg_paint(...)` also accepts `ContentDraw` when an explicit slot/policy
is needed. Portable graphs can use `Content::new(red)` directly.

CPU conversions, gamut mapping and gradient sampling come from `xgpui-color`.
GPU adapters have their own finite-value and geometry limits; CPU color values
are not automatically valid upload parameters.

## Platform limits

Portable `WideColor` presentation is currently implemented on macOS Metal.
Its former Metal/D3D11-specific `CustomContent::prepare` implementations are
removed. D3D11/wgpu portable execution is pending; unsupported renderers report
an explicit error. The separate instanced `WidePainter` path remains available
on Metal/D3D11. Physical-display validation remains separate from compilation.
Linux wide presentation, HDR content and transparent wide windows are deferred.
This API does not reinterpret existing shaders or sRGB paints as Display P3.

## Guides and examples

- [Window color requests, status and backend contract](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/high-gamut-windows.md)
- [Instanced wide fills and gradients](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/instanced-wgsl-v2.md#reusable-wide-fills-and-gradients)
- [Gamut lab and validation](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/gamut-lab/README.md)

From the repository root, run `just gamut` for the native comparison app.

## GPUI public API changes: additions and compatibility

Use this crate with xgpui's matching framework packages. Its integration depends
on [xgpui's GPUI API changes](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md#added-opt-in-window-presentation).
See the [compatibility guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/high-gamut-windows.md#gpui-public-api-changes-additions-and-compatibility)
for additions, affected public types and migration requirements.

## Project

[Repository](https://github.com/gpui-luma-dev/xgpui) · [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md) ·
[Website](https://xgpui.dev/). Licensed under Apache-2.0.

This extension uses its own `0.2.0` version, separate from xgpui framework
`1.0.0` and the upstream GPUI baseline.

## Validate the migration

```sh
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin metal-producer-regression --features runtime-shaders
```

Expect **PASS** for the control, original repeated-color scene, and distinct-output
dense scene. Every frame checks exact FP16 P3 pixels, cached replay and
`producer_readbacks=0`. See the [regression guide](../../../apps/xgpui-ext-lab/metal-producer-regression/README.md)
for stress options. Native alpha/coverage/parity checks:

```sh
MTL_DEBUG_LAYER=1 cargo test --manifest-path extensions/crates/xgpui-gamut/Cargo.toml \
  --features instanced,gpui-apple/runtime_shaders -- --test-threads=1
```

Expect 7 library tests and 2 native integration tests to pass, with one optional
performance comparison ignored. No producer texture allocations happen inside
`WideColor`; new graph realizations and replay are owned by the host executor.
