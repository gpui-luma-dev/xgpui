# xgpui-gamut

Explicit linear-P3 SDR producers, separate from `xgpui-shader`.
Native composition and window negotiation remain in GPUI backends.

[Usage and backend contract](../../../../docs/extensions/high-gamut-windows.md) ·
[GPUI API changes](../../../../docs/extensions/gpui-public-api.md) ·
[gamut-lab](../../../../apps/xgpui-ext-lab/gamut-lab/README.md)

## GPUI public API changes: additions and compatibility

[GPUI public API changes: additions and compatibility](../../../../docs/extensions/gpui-public-api.md#added-opt-in-window-presentation).
See the [crate guide](../../../../docs/extensions/high-gamut-windows.md#gpui-public-api-changes-additions-and-compatibility) for required additions and downstream compatibility impact.

## Shared color math

CPU conversions, gamut mapping, gradient references and WGSL helpers live in
[`xgpui-color`](../xgpui-color/README.md), a dependency-free runtime crate that
can be used without GPUI. `xgpui_gamut::color` re-exports it for convenience.
The existing bounded `WideColor::linear_p3` producer retains its behavior.

`xgpui-gamut` has no default runtime dependency on `xgpui-shader`. The comparison tests
in `tests/instanced_output.rs` use shader as a development dependency to validate
wide-output routing against typed producers. Enable the opt-in `instanced` feature for `WidePainter` and `WideGradient`;
the existing producer remains usable without it.


## Optional instanced fills

```toml
xgpui-gamut = { path = "../xgpui-gamut", features = ["instanced"] }
```

`instanced::WidePainter::new()` shares a process-wide immutable shader program.
Call `paint_color(window, bounds, radius, &wide_color)` or
`paint_gradient(window, bounds, radius, &gradient)` from a GPUI paint callback.
Enable `WideGamutSdr` outside painting first. The methods validate finite bounds
and nonnegative uniform corner radii; inherited clipping/opacity use GPUI's SPI.

`WideGradient::linear(start, end, colors, interpolation, mapping)` accepts two
`color::LinearRgba` stops, normalized fill coordinates, and explicit policies.
Stops convert to extended linear P3 on the CPU. The GPU interpolates, maps into
bounded P3, applies rounded coverage, and lets the host associate alpha and
convert to native composition primaries. Coordinates outside the axis extend
endpoint colors; coincident endpoints select the second stop. Gradient upload
channels and projection coefficients are limited to finite magnitudes <=65504;
very short or numerically extreme axes return an error instead of uploading
unstable coefficients. This is an adapter limit, not a limit of CPU color types.

There is no per-instance producer texture or offscreen producer pass. Solids
and gradients use one 80-byte instance layout and shared program identity;
compatible consecutive draws can batch. Legacy UI conversion and presentation
passes remain. Existing `WideColor`/`CustomContent` calls retain their texture path.

See the [instanced guide](../../../../docs/extensions/instanced-wgsl-v2.md) and run:

```sh
cargo test --locked --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-gamut/Cargo.toml --features instanced
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab -- --instanced --gradient
```
