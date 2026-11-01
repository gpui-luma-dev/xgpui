# gamut-lab

`xgpui-gamut` lab for macOS Metal and Windows D3D11: wide P3 surfaces and normal sRGB controls in one window.

```sh
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab
```

[Controls, captures, tests and Windows CI builds](../../../docs/extensions/high-gamut-windows.md#run-gamut-lab).


## Instanced comparison

Run `cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab -- --instanced`
for direct main-pass solids, or add `--gradient` for mapped OkLab gradients.
Without these flags the lab uses the portable WideColor content path (currently
Metal). Windows should use `--instanced` until its portable executor is implemented.
Both paths use the same overlays, opacity, backdrop, ordering and presentation
controls. `--sdr` selects the existing solid sRGB baseline. Headless capture works
with either path on supported backends; it reports encode + readback time, not
isolated GPU timing. Pending portable snapshots yield and retry with a 30-second
deadline, so a placeholder cannot be saved as the requested final image.

## Native color comparison

On macOS (Metal) or Windows (D3D11), run:

```sh
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-lab
```

Move the window onto your P3 display. Confirm `Effective: WideGamutSdr` and
`Display wide gamut: Some(true)` in the header. Each interpolation model has a
mapped sRGB row and a native Display P3 row, using 32 discrete reference samples
shared with the Wasm demo. Click the colors, mapping, and alpha controls to compare.
The default red/green endpoints make the gamut difference especially visible.
Display capability and effective mode report negotiation, not measured calibration.

Portable migration check (macOS):

```sh
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin gamut-lab --features runtime-shaders -- \
  --headless --frames 3 --png /tmp/gamut-portable.png
```

Look for `3 ready UI frames` and a saved image containing the red/green rounded
surfaces, intervening sRGB overlay and native tooltip. Missing/failed portable
content must fail capture rather than silently produce a fallback screenshot.
