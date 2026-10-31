# xgpui-color

Color science extension for **xgpui**, also usable independently. Convert RGB
spaces, map colors into gamut, sample gradients and reuse the same color math
in WGSL. The runtime uses only Rust's standard library: no GPUI, window system,
GPU or shader compiler is required.

## Dependency

```toml
[dependencies]
xgpui-color = "0.2.1"
```

Rust imports use `xgpui_color`.

## Convert a color

```rust
use xgpui_color::{EncodedRgb, RgbSpace};

let encoded = EncodedRgb::new(RgbSpace::Srgb, [0.2, 0.5, 0.8])?;
let p3 = encoded.decode()?.convert(RgbSpace::DisplayP3)?;
let perceptual = p3.to_oklab()?.to_oklch()?;
# Ok::<(), xgpui_color::ColorError>(())
```

| Type | Represents |
| --- | --- |
| `EncodedRgb`, `LinearRgb` | Encoded or linear RGB with explicit primaries. |
| `RgbSpace` | sRGB, Display P3, Rec.2020 or Adobe RGB (1998). |
| `XyzD65`, `Oklab`, `Oklch` | Intermediate and perceptual color spaces. |
| `LinearRgba` | Linear color with validated straight alpha. |

CPU calculations use `f64`, D65 and finite extended RGB/XYZ values. Conversion
does not automatically clip or map gamut. Rec.2020 uses its SDR transfer
function. Adobe RGB (1998) uses the exact 563/256 decoding exponent and
256/563 encoding exponent, including sign-preserving extended channels; it does
not use the sRGB transfer curve. PQ, HLG and white-point adaptation are outside
this API.

## Adobe RGB (1998)

```rust
use xgpui_color::{EncodedRgb, GamutMapping, RgbSpace};

let adobe = EncodedRgb::new(RgbSpace::AdobeRgb1998, [0., 1., 1.])?.decode()?;
let extended_p3 = adobe.convert(RgbSpace::DisplayP3)?;
assert!(extended_p3.components()[0] < 0.); // This cyan is outside P3.
let display_p3 = adobe.map_gamut(RgbSpace::DisplayP3, GamutMapping::ReduceChroma)?;
# Ok::<(), xgpui_color::ColorError>(())
```

Conversion and gradients retain extended values until explicit mapping. Adobe
RGB and P3 do not fully contain one another. Native image/shader producers still
use bounded linear P3; this math support does not widen those storage or output
contracts. See the media supplied-pixel API for explicit mapping into P3.

## Gamut mapping and gradients

`LinearRgb::map_gamut` explicitly chooses clipping or chroma reduction.
`LinearGradient` samples two stops in linear RGB, OkLab or OkLCh, with explicit
hue interpolation. Map the sampled color before associating alpha for output.

```rust
use xgpui_color::{
    ColorInterpolation, GamutMapping, LinearGradient, LinearRgb, LinearRgba, RgbSpace,
};

let red = LinearRgba::new(LinearRgb::new(RgbSpace::DisplayP3, [1., 0., 0.])?, 1.)?;
let blue = LinearRgba::new(LinearRgb::new(RgbSpace::DisplayP3, [0., 0., 1.])?, 0.5)?;
let gradient = LinearGradient::new(
    [0., 0.], [320., 0.], [red, blue], RgbSpace::DisplayP3,
    ColorInterpolation::Oklab,
)?;
let midpoint = gradient.sample([160., 0.])?;
let output = midpoint
    .map_gamut(RgbSpace::DisplayP3, GamutMapping::ReduceChroma)?
    .premultiplied();
# Ok::<(), xgpui_color::ColorError>(())
```

This is a deterministic mapping policy, not ICC relative colorimetric or CSS
local MINDE. Invalid or nonfinite values return `ColorError`.

## WGSL helpers

`WGSL_COLOR_MATH` contains resource-free WGSL helpers with the `gamut_` prefix.
Prepend it once to your shader source; it declares no entry point or bindings.
The GPU contract uses finite `f32` inputs and validated space/policy IDs.
Space IDs are stable: 0 = sRGB, 1 = Display P3, 2 = Rec.2020, 3 = Adobe RGB (1998).
Mapping IDs are 0 = clip and 1 = fixed-lightness/hue chroma reduction.

These functions do not change a renderer's output color space. Use the explicit
linear-P3 contract when integrating wide output with `xgpui-shader` or
`xgpui-gamut`.

## Documentation and validation

The [color guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/color-math.md)
covers types, dependency boundaries and the color/gamut lab. Refer to the
[source](https://github.com/gpui-luma-dev/xgpui/blob/main/extensions/crates/xgpui-color/src/lib.rs)
for the public API and numerical contracts.

Conversions follow [CSS Color 4 reference conversions](https://www.w3.org/TR/css-color-4/#color-conversion-code)
and [Björn Ottosson's OkLab definition](https://bottosson.github.io/posts/oklab/).
Tests cover CPU round trips, Naga WGSL validation/translation and native Metal
comparisons. Shader translation alone does not verify native D3D11 execution.
Metal and Naga are development dependencies only.

### Adobe RGB numerical regression criteria

Committed oracles use 120-digit Python Decimal arithmetic on exact binary inputs.
Primary matrices are independently solved from xy chromaticities and D65 using
pivoted Gaussian elimination. Transfer oracles use exact rational exponents;
they are independent of the Rust and WGSL implementation constants.

- CPU tests cover all 65,536 unsigned 16-bit transfer codes, signed zero,
  subnormals, underflow, overflow rejection, adjacent floats at gamut boundaries,
  and 21,296 scaled primary-conversion vectors.
- Scalar error budgets account for binary64 rounding of the reciprocal exponent,
  `powf` evaluation and underflow. Matrix budgets scale with the matrix norm and
  input magnitude, including cancellation.
- Native Metal checks 1,060 independent transfer cases per compiler mode.
  Acceptance is relative: eight binary32 epsilons (0.96 ppm) with fast math off,
  64 epsilons (7.63 ppm) with fast math on, plus an eight-subnormal rounding floor.
  Transfer endpoints must be exact. All nonzero GPU reference results are normal
  binary32 values; GPU subnormal behavior is outside these vectors.
- Media tests check exact binary16 patterns near black and at midtones, including
  intentional storage underflow and fractional alpha. Adobe primary/mip checks
  permit at most one binary16 representable step.

Reproduce both CPU/GPU and media FP16 oracle fixtures from the repository root:

```sh
python3 scripts/generate_adobe_rgb_references.py --check
```

The generator requires Python 3.11+ and rustfmt. Runtime evidence is tracked per
backend; numerical test thresholds are acceptance criteria for the covered
domains rather than a cross-device bit-identity promise.

## Project

[Repository](https://github.com/gpui-luma-dev/xgpui) · [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md) ·
[Website](https://xgpui.dev/). Licensed under Apache-2.0.

This extension uses its own `0.2.0` version, separate from xgpui framework
`1.0.0` and the upstream GPUI baseline.

## Custom RGB metadata

`RgbDefinition::new_d65(matrix, curves)` validates a row-major linear RGB →
relative XYZ/D65 matrix and three independent `RgbCurve` values (`Linear`,
`Srgb`, or positive `Gamma`). `decode_to_xyz_d65` retains finite extended RGB
and rejects conversion overflow. Matrices must be nonsingular, reasonably
conditioned and normalized to D65 white (Y=1); white admission permits at most
3/65536 absolute error per XYZ channel for quantized colorants. This metadata
admission bound is not the numerical accuracy promised by conversions.

`new_d65` accepts already-adapted colorants. Alternatively,
`RgbDefinition::new_with_white_point(matrix, curves, WhitePoint::D50)` explicitly
Bradford-adapts D50 colorants to D65 once. The matrix must sum to the declared
white within the colorant quantization bound. That bound is propagated through
the adaptation matrix; colorants are never silently renormalized.

`BradfordAdaptation::new(source, destination)` also adapts relative XYZ or colorant
matrices directly between `WhitePoint::D50` and `WhitePoint::D65`. Same-white
adaptation preserves finite input exactly, including signed zero. Other routes
retain finite extended coordinates and reject overflow. The white chromaticities
and Bradford convention follow [CSS Color 4](https://www.w3.org/TR/css-color-4/#color-conversion-code):
D50 xy=(0.3457,0.3585), D65 xy=(0.3127,0.3290), both normalized to Y=1.

This is not ICC rendering-intent handling. A future adapter must resolve profile
PCS normalization, encoded illuminant values and existing adaptation tags before
using this API; a profile's media-white label alone is insufficient. These two
reference whites do not represent every custom or encoded ICC white. Sampled
and ICC parametric curves, ICC parsing, alpha and file decoding remain outside
this color API.
