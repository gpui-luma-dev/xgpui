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
