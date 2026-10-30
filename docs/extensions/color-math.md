# xgpui-color: standalone color math

Use `xgpui-color` for CPU color conversions, [gamut mapping ↗](https://en.wikipedia.org/wiki/Gamut), two-stop gradient
math and reusable WGSL helpers. Its default configuration has zero runtime dependencies and needs no GPUI,
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

For integration, start with [color tasks](color-workflows.md) and [pixels + metadata](pixels-and-metadata.md). Optional [ICC parsing](icc-profiles.md) adds a parser dependency without adding a pixel transform engine.

## Types and contracts

`EncodedRgb` and `LinearRgb` distinguish transfer-encoded values from linear
light. `RgbSpace` supplies [sRGB ↗](https://en.wikipedia.org/wiki/SRGB), [Display P3 ↗](https://en.wikipedia.org/wiki/DCI-P3), [Adobe RGB (1998) ↗](https://en.wikipedia.org/wiki/Adobe_RGB_color_space) and [SDR ↗](https://en.wikipedia.org/wiki/Standard-dynamic-range_video) Rec.2020 primaries and
[transfer functions ↗](https://en.wikipedia.org/wiki/Transfer_functions_in_imaging). `XyzD65`, `Oklab` and `Oklch` provide conversion coordinates.
RGB and perceptual conversion retain extended values; gamut mapping is explicit.
CPU calculations use f64 and reject non-finite inputs/results. All named RGB
spaces use [D65 ↗](https://en.wikipedia.org/wiki/Standard_illuminant), so these conversions need no white-point adaptation.

`LinearRgba` validates [straight alpha ↗](https://en.wikipedia.org/wiki/Alpha_compositing). `LinearGradient` provides two fixed stops,
linear RGB/OkLab/OkLCh interpolation, alpha-aware components and four hue paths.
Sampling does not map gamut automatically. Map the interpolated result before
final output association. These APIs describe color math; they do not draw a
GPUI element or enable wide-gamut window presentation.

`WGSL_COLOR_MATH` supplies resource-free, prefixed f32 helpers for conversions,
mapping and interpolation. Host code must validate input ranges and IDs. The
crate README documents IDs, tolerances, hue handling and bounded search behavior.

## Custom RGB definitions

Use `RgbDefinition` for caller-supplied primaries, transfer curves and a reference white. Neutral-space constructors validate a neutral ramp; `from_relative_xyz_matrix` preserves an explicit relative XYZ matrix whose column sum can differ from its declared reference white. Conversion adapts that reference white to D65 once. The [ICC guide](icc-profiles.md) explains the supported profile path.

## Adobe RGB input and gamut preservation

Use `RgbSpace::AdobeRgb1998` for [Adobe RGB (1998) ↗](https://en.wikipedia.org/wiki/Adobe_RGB_color_space)
(`a98-rgb` in CSS Color 4). The source has D65 primaries and a power-law
transfer: decode with 563/256 and encode with 256/563. CPU and WGSL conversions,
OkLab/OkLCh interpolation and explicit mapping all accept it. WGSL IDs remain
0=sRGB, 1=P3, 2=Rec.2020; Adobe RGB appends ID 3.

```rust
let adobe = EncodedRgb::new(RgbSpace::AdobeRgb1998, [0., 1., 0.])?.decode()?;
let extended_p3 = adobe.convert(RgbSpace::DisplayP3)?; // negative red retained
let mapped_p3 = adobe.map_gamut(RgbSpace::DisplayP3, GamutMapping::ReduceChroma)?;
```

The [supplied-pixel media API](media-pipeline.md#supplied-rgba16f-images)
accepts `ImageColor::ADOBE_RGB_1998`. Its primaries and transfer are independent:
declaring `TransferFunction::Srgb` always applies that curve even with Adobe RGB
primaries. GPU image adjustments still require sRGB/P3 working matrices and
explicitly reject an Adobe RGB working matrix. CPU color matrices support Adobe
RGB. Input interpretation does not infer or load an ICC profile.

For the practical input contract, see [pixels + metadata](pixels-and-metadata.md).
The existing P3 loaders map saturated Adobe green/cyan into P3. To retain Adobe
RGB, use explicit bounded linear Rec.2020 storage. To retain the entire P3 gamut,
keep P3 storage: P3 red converts to Rec.2020 blue **-0.0012103403545183245**, so
bounded Rec.2020 does not quite contain P3. Both formats use associated linear
mipmaps and eight bytes per texel. Mapping policies remain explicit at ingestion.

### Preserve both source gamuts

Native wide placement accepts both P3 and Rec.2020 textures. Metal converts
Rec.2020 to **extended linear P3**, then retains negative and above-one channels
through source-over blending, Gaussian blur, capture isolation and linear color
filters. Existing P3 producers retain their meaning. The native layer remains
extended linear P3; its conversion from the composition target is the identity.
Windows converts to extended linear sRGB/scRGB. Neither path implies HDR,
increased brightness, monitor coverage or calibration.

`ShaderProgram::new_linear_rec2020` declares bounded straight-alpha linear
Rec.2020/D65 producer output. The wrapper clamps in that source space, converts
to native composition primaries and associates alpha. Existing
`new_linear_p3` output remains P3. GPU image adjustments continue to use explicit
sRGB/P3 working matrices; wide placement limits each affine row's absolute sum
to 4 to keep the supported four nested filters within finite FP16 storage.

| Boundary | Contract |
| --- | --- |
| Image storage | Existing bounded P3, plus opt-in bounded Rec.2020 for Adobe RGB; explicit mapping precedes association |
| Source sampling | Convert to extended native composition primaries without an intermediate gamut clamp |
| Metal captures | Associated extended linear P3; normal blending, blur and saturation/brightness/contrast/inversion retain extended channels |
| Artistic blend modes | Existing bounded channel definitions clamp blend operands; they are deliberate color operations and can discard extended values |
| Cache | Extended capture domain has a distinct context revision; mode switches rebuild capture storage |
| Metal presentation | Extended-linear P3 FP16 layer, no extra gamut clamp; native display mapping belongs to the OS |
| Windows presentation | Extended linear sRGB/scRGB FP16; existing capture/path-mask restrictions remain |
| Preview | Explicit clipped encoded-sRGB readback; this is not a wide-gamut export |

Metal runtime tests cover independent FP16 matrix oracles, near-black values,
negative channels, alpha, nested cached captures, blur, grayscale and mixed P3 /
Rec.2020 blending. macOS wgpu tests verify sRGB previews and rejection of native
wide output. HLSL translation is checked separately; Windows/Linux native runtime
is unverified. Tiled FP16 canvas pages remain P3. Supplied RGB matrix/TRC profiles are supported by the optional [ICC adapter](icc-profiles.md); automatic file metadata inference is separate.

## Dependency boundaries

- `xgpui-color`: standalone CPU math and WGSL source, zero runtime dependencies by default; optional `icc` parsing uses moxcms.
- `xgpui-gamut`: depends on color and re-exports it as `xgpui_gamut::color`;
  owns native wide-SDR producers. The `instanced` feature adds an optional shader
  adapter; comparison tests also use shader as a development dependency.
- `xgpui-shader`: general rendering infrastructure; no dependency on color or gamut.

Wide shader output uses an explicit [instanced rendering contract](instanced-wgsl-v2.md).
Window negotiation and Metal P3 / Windows [scRGB ↗](https://en.wikipedia.org/wiki/ScRGB) composition belong to the
[native presentation backends](high-gamut-windows.md). The optional [ICC adapter](icc-profiles.md) supplies RGB definitions; calibration and [HDR ↗](https://en.wikipedia.org/wiki/High_dynamic_range) are separate work.

## Validation

Adobe-specific numerical tests use reproducible 120-digit Decimal oracles,
independent chromaticity-derived matrices, a full 16-bit ramp and scale-aware
error budgets. Native transfer tests exercise Metal fast math both on and off;
media tests check exact FP16 patterns near black. See the
[numerical regression criteria](https://github.com/gpui-luma-dev/xgpui/blob/main/extensions/crates/xgpui-color/README.md#adobe-rgb-numerical-regression-criteria)
for tolerances, covered domains and fixture regeneration.


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
