# Color task guides and Wasm demos — 2026-10-30

The task guides distinguish source interpretation, bounded storage, native composition and physical display capability. The original ICC investigation is archived in `icc-import-probe/design-history.md`; current use is documented in `docs/extensions/icc-profiles.md`. No site deployment or PR merge is part of this change.

## Checks

- Regenerated the website with the locked/offline crate-roadmap tool; all `docs/verify.mjs` checks pass, including four new routes, navigation, section links, 23 gallery entries and bundled color notices.
- Release Wasm bundle builds offline with the locked application dependencies.
- Strict extensions-web Wasm Clippy passes. The current toolchain also required replacing an existing canvas modulo check with `is_multiple_of`; no behavior change intended.
- Both `color-tasks` model tests pass without default native features. They cover the published sampled-profile gray XYZ, exact associated FP16 words, transparent canonicalization, metadata differences, the CIE locus, simulated-gamut bounds and bounded preview channels. The profile core has separate independent numerical-oracle tests; these demo tests do not replace them.

## Browser runtime

Checked locally in the macOS in-app Chromium browser, using the release bundle and forced backend URLs:

| Route | WebGPU | WebGL2 |
| --- | --- | --- |
| `?demo=chromaticity` | Renders chart and controls; source and mapping changes repaint | Renders chart and fill |
| `?demo=pixel-metadata` | Renders preview, controls and calculated values | Renders; cycles all eight interpretations into sampled ICC mode |

For sampled sRGB2014 Mid gray, Rec.2020 storage and alpha 0.5, WebGL2 displays source XYZ `[0.0258687606, 0.0272139801, 0.0296451330]` and actual CPU-packed words `[22f8, 22f7, 22f8, 3800]`, matching the model test. Storage and alpha controls repaint correctly. The canvas reports `srgb` for both backends while the browser independently reports display gamut at least P3.

The documentation task table renders, and its storage link reaches `#pixels-and-metadata&section=choose-storage-deliberately`. CIE and ICC credits are linked from the browser runtime and packaged beside the bundle.

These are sRGB visualizations of Rust/Wasm computations and CPU-packed bytes. They do not establish browser-native wide-gamut composition, GPU FP16 readback, physical output accuracy, or other browsers/platforms' runtime acceptance. The desktop lab's separate Metal checks remain the native composition evidence.

## Commands

```sh
cargo run --locked --offline --manifest-path tools/crate-roadmap/Cargo.toml
node docs/verify.mjs
cargo test --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --no-default-features --features web --test color-tasks
cargo clippy --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --no-default-features --features web --bin extensions-web --target wasm32-unknown-unknown -- -D warnings
bash apps/xgpui-ext-lab/extensions-web/build.sh --release --offline
```

## Guided metadata follow-up

Added Red, three deterministic lesson presets and a same-pixels named-sRGB reference swatch. The embedded tutorial explains the expected observation and when to read XYZ/FP16 rather than expect a visible difference. Gray Adobe is slightly darker than named sRGB, saturated Adobe green can clip to nearly the same preview, and sampled sRGB differences are numerical. A third model test checks those claims and the red sample. All three tests pass; website verification, release Wasm build and strict Wasm Clippy pass. Live WebGPU checks exercise all lesson presets and Red; sampled-profile half-alpha words remain unchanged. These changes do not add browser wide output.
