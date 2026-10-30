# xgpui-shader

Shader and paint extension for **xgpui**. Add gradients, procedural brushes,
custom WGSL shaders and composited effects to ordinary styled UI elements.
GPUI continues to own layout, input and scene composition.

## Dependencies

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.0.0" }
xgpui-shader = { version = "0.2.1", features = ["portable-shaders"] }
```

## Brushes and gradients

Use `painted()` as a styled container with brush backgrounds and borders:

```rust,ignore
use gpui::{prelude::*, *};
use xgpui_shader::{painted, Brush, BrushStop};

let fill = Brush::linear(45., vec![
    BrushStop::new(0., [0.1, 0.3, 1., 1.]),
    BrushStop::new(1., [1., 0.5, 0.1, 1.]),
])?;

painted()
    .w(px(240.)).h(px(160.))
    .rounded(px(24.))
    .bg_brush(fill)
    .child("Ordinary xgpui children")
```

Brushes support solid, linear, radial, freeform and Hermite mesh fills, plus
optional noise and blend modes. Retain brushes in view state and reuse them
across renders. Ordinary brush colors use encoded-sRGB SDR values; wide-gamut
output has a separate explicit contract.

See the [brush guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/brushes.md)
for stop limits, coordinates, interpolation, noise and mesh examples.

## Text fills

`painted().text_brush(brush)` fills monochrome glyphs with a brush. Emoji retain
their native color rendering. Background, border and text fills share the same
brush types.

## Backdrop and layer effects

Choose the effect according to what should be filtered:

| API | Applies to |
| --- | --- |
| `backdrop_blur(BackdropBlur::new(px(16.))?)` | Content behind the surface; its children stay sharp. |
| `layer_effect(LayerEffect::opacity(0.5)?)` | The completed subtree, including overlapping children. |
| `LayerEffect::grayscale`, `saturation`, `brightness`, `contrast`, `invert` | Color adjustment of the captured subtree. |
| `LayerEffect::blend` | Blending the subtree with the destination. |
| `LayerEffect::blur` | Bounded blur of the subtree. |

Layer filters can be combined. Blur is bounded and clipped; it is not an
unlimited image-processing surface. See the
[paint and effect guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/extension-paints.md)
for composition order, bounds and caching behavior.

## Path rendering

`brush.prepare_path(&path)?` creates a retained `DirectPath` for repeated drawing.
`paint_path` uses direct coverage where supported and falls back to native
coverage. `paint_path_direct` requests the strict direct route; its geometry
and complexity limits are documented in the
[path contract](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/brushes.md).

## Custom shaders, shadows and borders

The `instanced` module draws typed WGSL programs in the host render pass.
`ShadowPainter` adds programmable rounded-box shadows; `AnimatedBorder` provides
reusable conic border animation. The application drives animation and reduced
motion behavior. See [instanced WGSL](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/instanced-wgsl-v2.md)
for complete programs, parameter layouts and backend limits.

## Selectable typography

`typography::SelectableText` supports retained, read-only single-line LTR text,
selection, copy, accessibility, and typewriter/wave/scramble effects. It works
without `portable-shaders`; brush fills and glyph shadows require that feature.
See the [typography guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/typography.md)
for its supported Unicode subset and editing limits.

## Features and examples

| Feature | Purpose |
| --- | --- |
| `portable-shaders` | Brushes, layer effects, typed instanced shaders and portable shader paints. |
| `wgpu` | Optional wgpu backend adapters. |

Backend support varies by API; consult the guides for Metal, D3D11 and wgpu
coverage. From the repository root:

```sh
just paints
just shaders
```

The [paint lab](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/paint-lab/README.md) and
[shader lab](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/shader-examples/README.md)
cover native demos and the browser companion builds.

## GPUI public API changes: additions and compatibility

Use this crate with xgpui's matching framework packages. Its integration depends
on [xgpui's GPUI API changes](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md#altered-existing-public-types--source-compatibility-risks).
See the [compatibility guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/extension-paints.md#gpui-public-api-changes-additions-and-compatibility)
for additions, affected public types and migration requirements.

## Project

[Repository](https://github.com/gpui-luma-dev/xgpui) · [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md) ·
[Website](https://xgpui.dev/). Licensed under Apache-2.0.

This extension uses its own `0.2.0` version, separate from xgpui framework
`1.0.0` and the upstream GPUI baseline.
