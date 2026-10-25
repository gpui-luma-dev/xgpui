# gamut-lab

`xgpui-gamut` lab for macOS Metal and Windows D3D11: wide P3 surfaces and normal sRGB controls in one window.

```sh
cargo run --locked --config .cargo/validation.toml --manifest-path apps/gamut-lab/Cargo.toml
```

[Controls, captures, tests and Windows CI builds](../../docs/extensions/high-gamut-windows.md#run-gamut-lab).


## Instanced comparison

Run `cargo run --locked --config .cargo/validation.toml --manifest-path apps/gamut-lab/Cargo.toml -- --instanced`
for direct main-pass solids, or add `--gradient` for mapped OkLab gradients.
Without these flags the lab retains the existing offscreen P3 producer path.
Both paths use the same overlays, opacity, backdrop, ordering and presentation
controls. `--sdr` selects the existing solid sRGB baseline. Headless capture works
with either flag; it reports encode + readback time, not isolated GPU timing.

## Native color comparison

On macOS (Metal) or Windows (D3D11), run:

```sh
cargo run --locked --config .cargo/validation.toml --manifest-path apps/gamut-lab/Cargo.toml --bin color-lab
```

Move the window onto your P3 display. Confirm `Effective: WideGamutSdr` and
`Display wide gamut: Some(true)` in the header. Each interpolation model has a
mapped sRGB row and a native Display P3 row, using 32 discrete reference samples
shared with the Wasm demo. Click the colors, mapping, and alpha controls to compare.
The default red/green endpoints make the gamut difference especially visible.
Display capability and effective mode report negotiation, not measured calibration.
