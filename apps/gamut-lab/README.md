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
