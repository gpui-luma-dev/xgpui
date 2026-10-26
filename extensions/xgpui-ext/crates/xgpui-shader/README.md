# xgpui-shader

Opt-in GPUI extension for solid colors, mesh and analytic gradients, and portable
WGSL paints. Effects and backend adapters live here; GPUI owns scene composition.

- [Use paints and interactive containers](../../../../docs/extensions/extension-paints.md)
- [Producer reference and caches](../../../../docs/extensions/shader-producers.md)
- [Instanced WGSL v2](../../../../docs/extensions/instanced-wgsl-v2.md) · [new WASM examples](../../../../apps/shader-examples/README.md)
- [Portable WGSL](../../../../docs/extensions/portable-wgsl-v1.md)
- [wgpu](../../../../docs/extensions/portable-paints.md) · [native D3D11](../../../../docs/extensions/native-dx11-paints.md)
- [GPUI API changes](../../../../docs/extensions/gpui-public-api.md) · [preparation and recording](../../../../docs/extensions/paint-contract-hardening.md)
- Run/test: [paint-lab](../../../../apps/paint-lab/README.md)

## GPUI public API changes: additions and compatibility

[GPUI public API changes: additions and compatibility](../../../../docs/extensions/gpui-public-api.md#altered-existing-public-types--source-compatibility-risks).
See the [crate guide](../../../../docs/extensions/extension-paints.md#gpui-public-api-changes-additions-and-compatibility) for required additions and downstream compatibility impact.

## Run the instanced shader demo

From the repository root:

```sh
just --justfile apps/shader-examples/justfile ui
```

Or run `just ui` inside `apps/shader-examples`. Named recipes include `minimal`,
`glow`, `particles` and `composition`. Use `just --set profile release ui` there
for the optimized native app. WASM is a companion build; see the
[demo guide](../../../../apps/shader-examples/README.md).

Programmable rounded-box shadows: `just --justfile apps/shader-examples/justfile shadows`.
[`painted().shadow_paint(layer)`, `ShadowPainter` and `ShadowStyle`](../../../../docs/extensions/instanced-wgsl-v2.md#richer-shadows)
provide up to four color stops, adjustable falloff, blur/spread/offset and inset/outset modes.

Reusable rotating conic borders: `AnimatedBorder::layer(style, phase_in_turns)` and
`painted().animated_border(layer)`. See [animated borders](../../../../docs/extensions/instanced-wgsl-v2.md#animated-borders)
for host-driven animation, pause/reduced motion and style options.

## Main-pass brushes

With `portable-shaders`, `Brush` and `BrushStop` provide solid, linear and radial
backgrounds and solid border rings through `painted().bg_brush(...).border_brush(...)`.
Use `brush.with_noise(Noise::simplex(0.04, 3, 0.35), BlendMode::Overlay)` for
bounded procedural modulation; Simplex, Worley F1/F2, blue-noise dithering, and seven
brush-local blend modes are available.
Use `painted().text_brush(brush).child("Wrapped gradient text ✨")` for
main-pass grayscale glyph masking; emoji remain native color.
Use `Brush::freeform(points)` for up to three IDW color points and
`Brush::mesh(corners)` for a regular four-corner Hermite field, across the same
background, border and text APIs. Distorted grids still use `MeshGradient`.
`painted().backdrop_blur(BackdropBlur::new(px(16.))?)` adds an explicit bounded
SDR framebuffer filter before the surface and its sharp children. It reuses
working textures and reuses filtered results when preceding retained commands match.
`painted().layer_effect(LayerEffect::opacity(0.5)?)` captures a bounded subtree
and fades its completed image once, including overlaps. Retained child commands
reuse the captured result; live external content refreshes conservatively.
`LayerEffect::grayscale(1.)` and `LayerEffect::saturation(2.)` filter the completed
subtree before opacity. Combine with `LayerEffect::opacity(0.7)?.with_saturation(0.35)?`;
retained raw child pixels can be reused when only the filter changes.
`LayerEffect::brightness(0.55)` and `LayerEffect::contrast(1.6)` adjust captured
color after saturation. Combine setters with `with_brightness` and `with_contrast`;
all color operations preserve alpha and reuse retained raw child pixels.
`LayerEffect::invert(1.)` complements completed subtree RGB after contrast;
`with_invert(0.5)` produces mid-gray while preserving coverage.
`LayerEffect::blend(BlendMode::Multiply)?` blends the completed subtree against
saved destination pixels. `with_blend_mode` combines all seven modes with group
filters and opacity, reusing capture textures and retained raw pixels.
`LayerEffect::blur(px(8.))?` or `with_blur(px(8.))?` softens the completed subtree
before color filters. Radius is bounded to 16 logical / 64 physical pixels and
the finished blur is clipped after sampling surrounding subtree content. Real
content edges still fade, and sampling is bounded by the viewport. Unchanged source/radius reuse filtered pixels;
radius edits reuse raw source pixels and rerun two separable Gaussian passes.
Adaptive blur uses half/quarter-resolution working textures at larger physical
radii, combining area downsampling with horizontal filtering and reconstructing
in the existing composition pass. Use `with_blur_quality(LayerBlurQuality::FullResolution)`
for the full-resolution reference; quality changes keep raw source captures.
`brush.prepare_path(&path)?` returns a retained `DirectPath` for texture-free,
zero-coverage-pass fills and strokes, including tessellated curves and holes.
Use `prepared.paint(window, offset)?` for repeated placement. The strict direct
route supports non-overlapping solid triangle meshes within 512 triangles and
256 uncancelled edges. `paint_path_direct` is the immediate strict API;
`paint_path` tries direct coverage and uses native coverage for unsupported input.
`paint_path_native` preserves the explicit native MSAA reference/fallback.
Freeform brushes accept up to 16 points; sets above three specialize a full-precision
program and should be retained rather than rebuilt for per-frame animation.
`Brush::hermite_patch(points)` and `Brush::hermite_mesh(width, height, points)`
accept bounded distorted grids up to 3x3, with positions and U/V tangents. A
certified global inverse evaluates connected patches without fading their joins.
See [brush contracts and examples](../../../../docs/extensions/brushes.md).
