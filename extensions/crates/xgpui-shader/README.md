# xgpui-shader

Shader and paint extension for **xgpui**. Add gradients, procedural brushes,
custom WGSL shaders and composited effects to ordinary styled UI elements.
GPUI continues to own layout, input and scene composition.

## Dependencies

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.2.0" }
xgpui-shader = { version = "0.3.0", features = ["portable-shaders"] }
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
output has a separate explicit contract. `ShaderProgram::new_linear_p3` and
`new_linear_rec2020` declare bounded linear SDR producer primaries; native wrappers
convert into extended composition primaries after source clamping. Metal captures
and normal blending retain extended channels. Artistic blend modes retain bounded
operand definitions. Native wide output is rejected by wgpu/browser hosts.

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

Use `shader_paint::ShaderProgram::new` for reflected shader paints integrated
with backgrounds, borders and paths. Use `instanced::ShaderProgram<T>::new` for
typed WGSL programs drawn in the host render pass. Both APIs are experimental.
See [choosing a rendering model and migrating old names](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/shader-models.md)
for parameter limits, backend coverage and migration from the removed beta API names.

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

## Portable SolidColor

`SolidColor` is now a CPU-only `ContentSource`, available without a backend feature:

```rust,ignore
use gpui::content::{ContentDraw, PresentationSlotId};
use xgpui_shader::{SolidColor, painted};

let solid = SolidColor::new([0.15, 0.65, 0.9, 0.75])?;
let slot = PresentationSlotId::new(); // retain per logical paint role
painted().bg_paint(ContentDraw::new(solid.content()).slot(slot));
```

Input and graph output are straight-alpha encoded-sRGB; materialized storage is
RGBA8. `content()` and clones retain source identity. `Content::new(solid)` also
works inside portable graphs. Native allocation, structural output sharing,
replay, resize, and completion belong to the host. Producer-side `preparations()`
and `cached_texture_info()` diagnostics are removed. Meshes and experimental shader paints retain their current execution paths for now.

Portable execution currently supports Metal. D3D11/wgpu portable execution remains
pending; unsupported hosts report an explicit capability error. Use ordinary
native colors or `Brush::solid` for existing main-pass UI solids on those hosts.
The browser demo uses an explicit native solid reference. No application-side
Metal/D3D11/wgpu branches are required by the portable source API.

From the repository root on macOS:

```sh
MTL_DEBUG_LAYER=1 cargo test \
  --manifest-path extensions/crates/xgpui-shader/Cargo.toml \
  --test portable_solid --features gpui-apple/runtime_shaders \
  -- --test-threads=1 --nocapture
```

What to look for: **4 passed**, correct straight-alpha/opacity/rounded clip pixels,
one shared realization for equal source requests, cached replay without new jobs,
a new realization after resize, `producer readbacks == 0`, and typed Pending or
UnsupportedCapability results rather than successful fallback snapshots.


## Portable analytic gradients

`Gradient` is a CPU-only `ContentSource` on all targets. Use `gradient.content()`
with `painted().bg_paint(...)`, border paints, paths, or `ContentDraw`. Owned
`Gradient` also converts directly into `Content` or `Paint`. Clones retain identity;
`with_kind` and `with_stops` create immutable snapshots with new identities.

Linear, radial, conic and diamond geometry, 2–32 sorted stops, duplicate-offset
hard edges, endpoint clamping and premultiplied encoded-sRGB interpolation are
preserved. Output is straight-alpha RGBA8 sRGB. One shared WGSL program uses
reflected uniforms and an immutable 1024-byte stop buffer; the host owns native
pipelines, resource sharing, asynchronous completion and replay caching.
Producer preparation/pipeline counters and native adapters have been removed.

Execution currently requires the Metal portable executor; other hosts return
UnsupportedCapability. The browser lab keeps its analytic previews through an
app-local adapter to the public experimental shader-paint API until portable wgpu
execution is implemented.

```sh
MTL_DEBUG_LAYER=1 cargo test \
  --manifest-path extensions/crates/xgpui-shader/Cargo.toml \
  --test portable_gradient --features portable-shaders,wgpu,gpui-apple/runtime_shaders \
  -- --test-threads=1 --nocapture
```

What to look for: five passing tests and one ignored subprocess helper; independent
pixel references for all four geometries, transparent/hard/32-stop palettes, one
shared pipeline, cached replay, edit/resize updates, and repeated 512→1→512 draws
finishing with zero producer readbacks and at most two in-flight command buffers.

For changing sources, retain a `PresentationSlotId` per destination paint role
and use `ContentDraw::new(gradient.content()).slot(slot)`. Source identity changes
on edits; the presentation identity stays stable so pending frames can retain the
last completed image. Anonymous draws have no previous-image history. The native
gradient suite deliberately pauses producer work to verify all four kinds keep
their previous pixels visible during updates.
