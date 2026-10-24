# xgpui-ext

Independent, opt-in GPUI extension crates. Extensions depend on GPUI; GPUI core
never depends on extension crates. Keep renderer integration and API changes small.

- [`xgpui-shader`](crates/xgpui-shader/README.md): mesh, gradients and portable shaders.
- [Paint integration](../../docs/extensions/extension-paints.md) · [GPUI API changes](../../docs/extensions/gpui-public-api.md)
- [Producer reference](../../docs/extensions/shader-producers.md) · [preparation and recording](../../docs/extensions/paint-contract-hardening.md)
- Local test/performance apps: [paint-lab](../../apps/paint-lab/README.md)
- [`xgpui-gamut`](crates/xgpui-gamut/README.md): linear-P3 SDR producers; [presentation contract](../../docs/extensions/high-gamut-windows.md).
- [`xgpui-parley`](crates/xgpui-parley/README.md): opt-in Parley text backend: flat LTR, BiDi paragraphs, shared font identity and grayscale rasterization.
- [Build](../../docs/BUILDING.md) · [web docs generation](../../docs/WEBSITE.md)

[#7: Custom shaders](https://github.com/gpui-luma-dev/xgpui/issues/7) and
[#8: high-gamut windows](https://github.com/gpui-luma-dev/xgpui/issues/8) are separate capabilities.
