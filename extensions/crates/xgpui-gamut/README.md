# xgpui-gamut

Wide-gamut color extension for **xgpui**. Draw explicit linear Display P3 SDR
content alongside ordinary sRGB controls, without changing the color meaning
of existing UI paints.

## Dependencies

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.0.0" }
xgpui-gamut = "0.2.1"
```

## Enable wide content

Request wide-gamut SDR on the window outside drawing, then use a `WideColor`
producer. `xgpui-shader` supplies optional `painted()` container ergonomics:

```rust,ignore
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_gamut::WideColor;
use xgpui_shader::painted;

window.set_color_mode(WindowColorMode::WideGamutSdr)?;
let red = Arc::new(WideColor::linear_p3([1., 0., 0., 0.5])?);

painted()
    .w(px(320.)).h(px(240.))
    .bg_paint(red)
    .child("Normal sRGB controls")
```

Add `xgpui-shader = "0.2.0"` for that container example. The default producer can
also be used without the shader extension. Inspect the window's reported color
status; requesting wide output is not proof that the display is presenting it.

## Instanced fills and gradients

Enable `instanced` for `instanced::WidePainter` and `WideGradient`. This feature
adds a dependency on `xgpui-shader` and draws supported solids/gradients directly
in the host render pass. The default `WideColor` producer retains its texture
preparation path.

CPU conversions, gamut mapping and gradient sampling come from `xgpui-color`.
GPU adapters have their own finite-value and geometry limits; CPU color values
are not automatically valid upload parameters.

## Platform limits

The native integration targets macOS Metal and Windows D3D11. Windows native
execution and physical-display validation remain separate from cross-compilation.
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
