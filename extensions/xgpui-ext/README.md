# xgpui-ext

Independently usable, opt-in extension crates. Rendering extensions depend on
GPUI; the shared `xgpui-color` math crate has no runtime dependencies. GPUI core
never depends on extension crates. Keep renderer integration and API changes small.

- [`xgpui-color`](crates/xgpui-color/README.md): CPU color conversions, gamut mapping, gradient math and reusable WGSL, usable without GPUI.
- [`xgpui-shader`](crates/xgpui-shader/README.md): mesh, gradients and portable shaders.
- [Paint integration](../../docs/extensions/extension-paints.md) · [GPUI API changes](../../docs/extensions/gpui-public-api.md)
- [Producer reference](../../docs/extensions/shader-producers.md) · [preparation and recording](../../docs/extensions/paint-contract-hardening.md)
- [`xgpui-media`](crates/xgpui-media/README.md): bounded native image loading and dedicated mipmapped SDR textures.
- [`xgpui-svg`](crates/xgpui-svg/README.md): a reusable full-color SVG element, immutable documents, bounded progressive rendering, and shared job/image-memory scheduling.
- SVG scaling and native path comparison: [svg-lab](../../apps/svg-lab/README.md).
- Local test/performance apps: [paint-lab](../../apps/paint-lab/README.md)
- [`xgpui-gamut`](crates/xgpui-gamut/README.md): linear-P3 SDR producers; [presentation contract](../../docs/extensions/high-gamut-windows.md).
- [`xgpui-parley`](crates/xgpui-parley/README.md): opt-in Parley text backend: flat LTR, BiDi paragraphs, shared font identity and grayscale rasterization.
- [Build](../../docs/BUILDING.md) · [web docs generation](../../docs/WEBSITE.md)

[#7: Custom shaders](https://github.com/gpui-luma-dev/xgpui/issues/7) and
[#8: high-gamut windows](https://github.com/gpui-luma-dev/xgpui/issues/8) are separate capabilities.

Browser demos for shader paints and Parley paragraphs: [extensions-web](../../apps/extensions-web/README.md).

For interactive image comparisons and upload diagnostics, see
[Media lab](../../apps/media-lab/README.md).
