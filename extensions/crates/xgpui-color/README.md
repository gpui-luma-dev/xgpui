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

## Project

[Repository](https://github.com/gpui-luma-dev/xgpui) · [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md) ·
[Website](https://xgpui.dev/). Licensed under Apache-2.0.

This extension uses its own `0.2.0` version, separate from xgpui framework
`1.0.0` and the upstream GPUI baseline.
