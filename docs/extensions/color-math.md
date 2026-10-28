# xgpui-color: standalone color math

Use `xgpui-color` for CPU color conversions, [gamut mapping ↗](https://en.wikipedia.org/wiki/Gamut), two-stop gradient
math and reusable WGSL helpers. It has no runtime dependencies and needs no GPUI,
window, GPU context or shader compiler. Naga and native Metal are test-only dependencies.

```toml
# App under apps/:
[dependencies]
xgpui-color = { path = "../../extensions/crates/xgpui-color" }
```

```rust
use xgpui_color::{EncodedRgb, GamutMapping, RgbSpace};

let srgb = EncodedRgb::new(RgbSpace::Srgb, [0.2, 0.5, 0.8])?;
let linear_p3 = srgb.decode()?.convert(RgbSpace::DisplayP3)?;
let perceptual = linear_p3.to_oklab()?.to_oklch()?;
let bounded = linear_p3.map_gamut(RgbSpace::DisplayP3, GamutMapping::ReduceChroma)?;
```

## Types and contracts

`EncodedRgb` and `LinearRgb` distinguish transfer-encoded values from linear
light. `RgbSpace` supplies [sRGB ↗](https://en.wikipedia.org/wiki/SRGB), [Display P3 ↗](https://en.wikipedia.org/wiki/DCI-P3) and [SDR ↗](https://en.wikipedia.org/wiki/Standard-dynamic-range_video) Rec.2020 primaries and
[transfer functions ↗](https://en.wikipedia.org/wiki/Transfer_functions_in_imaging). `XyzD65`, `Oklab` and `Oklch` provide conversion coordinates.
RGB and perceptual conversion retain extended values; gamut mapping is explicit.
CPU calculations use f64 and reject non-finite inputs/results. All working RGB
spaces use [D65 ↗](https://en.wikipedia.org/wiki/Standard_illuminant), so these conversions need no white-point adaptation.

`LinearRgba` validates [straight alpha ↗](https://en.wikipedia.org/wiki/Alpha_compositing). `LinearGradient` provides two fixed stops,
linear RGB/OkLab/OkLCh interpolation, alpha-aware components and four hue paths.
Sampling does not map gamut automatically. Map the interpolated result before
final output association. These APIs describe color math; they do not draw a
GPUI element or enable wide-gamut window presentation.

`WGSL_COLOR_MATH` supplies resource-free, prefixed f32 helpers for conversions,
mapping and interpolation. Host code must validate input ranges and IDs. The
crate README documents IDs, tolerances, hue handling and bounded search behavior.

## Dependency boundaries

- `xgpui-color`: standalone CPU math and WGSL source, zero runtime dependencies.
- `xgpui-gamut`: depends on color and re-exports it as `xgpui_gamut::color`;
  owns native wide-SDR producers. The `instanced` feature adds an optional shader
  adapter; comparison tests also use shader as a development dependency.
- `xgpui-shader`: general rendering infrastructure; no dependency on color or gamut.

Wide shader output uses an explicit [instanced rendering contract](instanced-wgsl-v2.md).
Window negotiation and Metal P3 / Windows [scRGB ↗](https://en.wikipedia.org/wiki/ScRGB) composition belong to the
[native presentation backends](high-gamut-windows.md). [ICC ↗](https://en.wikipedia.org/wiki/ICC_profile) profiles, calibration
and [HDR ↗](https://en.wikipedia.org/wiki/High_dynamic_range) are separate work.

## Validation

```sh
cargo test --locked --manifest-path extensions/crates/xgpui-color/Cargo.toml
cargo check --locked --manifest-path extensions/crates/xgpui-color/Cargo.toml --lib --target wasm32-unknown-unknown
```

Tests cover CPU reference vectors, mapping/interpolation boundaries, shader
translation and, on macOS, native Metal comparison. WASM checks compile the
standalone library; they do not establish browser GPU presentation support.

## GPUI API compatibility

This crate introduces no GPUI API changes. Applications can depend on it without
GPUI. Rendering integrations have their own [public API contracts](gpui-public-api.md).


## Browser Color / Gamut lab

The WebAssembly sample app includes `?demo=color`, with automatic, forced WebGPU
and forced WebGL2 backends. Open the browser demo from this crate's Use page.
Compare encoded sRGB, linear RGB, OkLab and shorter-hue OkLCh interpolation;
cycle three Display P3 endpoint pairs, toggle clipping/chroma reduction, and
change the ending alpha between 100% and 25%.

Every row uses 32 CPU-generated samples from `xgpui-color`, mapped into sRGB
before ordinary `xgpui-shader` paints interpolate between them. Amber strips
mark samples outside sRGB before mapping (ignoring 1e-7 numerical boundary noise).
The encoded-sRGB row interpolates extended encoded endpoints before mapping;
all rows use the same source colors. The preview does not enable native P3
programs or prove physical wide-gamut display output.

Build and serve from the repository root:

```sh
bash apps/xgpui-ext-lab/extensions-web/build.sh
python3 apps/xgpui-ext-lab/extensions-web/serve.py
```

Then open `http://localhost:8081/?demo=color`. Backend links on the gallery select
`&backend=webgpu` or `&backend=webgl` for comparison.

### Runtime output diagnostics and native comparison

The browser demo reports source space, preview mapping, the active renderer's
canvas [color space ↗](https://en.wikipedia.org/wiki/Color_space), and `(color-gamut: p3)` display capability. WebGL reads
`drawingBufferColorSpace`; WebGPU reads `getConfiguration().colorSpace` when
available and otherwise reports unknown. These diagnostics observe the renderer's
own context without creating or reconfiguring a context. Display capability does
not turn the mapped sRGB demo into P3 output or verify physical calibration.

For actual native Display P3 output on macOS/Metal or Windows/D3D11:

```sh
cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-lab
```

This companion displays paired sRGB and P3 rows using the browser's same 32 CPU
reference samples, drawn as discrete swatches. Check the header for effective
`WideGamutSdr` and display wide-gamut support, then move the window to your P3
monitor. Colors, mapping policy, and end alpha can be changed interactively.
