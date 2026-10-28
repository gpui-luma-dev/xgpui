# xgpui-color

Standalone CPU color science and reusable WGSL math. No runtime dependencies,
GPUI, window system, native backend, or shader compiler required.

[Web documentation and dependency boundaries](../../../docs/extensions/color-math.md).

## CPU color core

`xgpui_color` provides `EncodedRgb`, `LinearRgb`, `RgbSpace`,
`XyzD65`, `Oklab` and `Oklch`. It uses only the Rust standard library and
requires no GPUI or GPU dependencies. Native Metal and Naga are test-only dependencies.

```rust
use xgpui_color::{EncodedRgb, RgbSpace};

let encoded = EncodedRgb::new(RgbSpace::Srgb, [0.2, 0.5, 0.8])?;
let p3 = encoded.decode()?.convert(RgbSpace::DisplayP3)?;
let perceptual = p3.to_oklab()?.to_oklch()?;
# Ok::<(), xgpui_color::ColorError>(())
```

All calculations use f64. RGB/XYZ/OkLab components accept finite extended
values; conversion never clips or maps gamut. Arithmetic producing non-finite
results returns `ColorError`. RGB transfer functions extend by sign symmetry.
sRGB and Display P3 share a transfer function; Rec.2020 uses its SDR transfer,
not PQ/HLG. XYZ uses D65 and relative Y=1 reference white. Matrices are row-major,
multiplying column vectors, and `linear_to_linear_matrix` operates only on
already-decoded channels. No white-point adaptation is needed between these
three D65 spaces.

OkLab has nominal SDR lightness 0..1. OkLCh chroma must be nonnegative; hue is
in degrees normalized to [0,360). Exact zero chroma canonicalizes hue to zero;
near-neutral hue is retained, without an implicit interpolation threshold.
Alpha, gamut mapping and interpolation use the explicit APIs below.
The existing bounded, straight-alpha `xgpui_gamut::WideColor::linear_p3` API
requires explicit output mapping and validation of extended conversion results.

RGB primary matrices and transfer functions follow the
[CSS Color 4 reference conversions](https://www.w3.org/TR/css-color-4/#color-conversion-code).
OkLab uses the published linear-sRGB coefficients from
[Björn Ottosson's OkLab definition](https://bottosson.github.io/posts/oklab/).
Tests use 2e-14 RGB/XYZ round-trip tolerance, 4e-8 transfer tolerance (including
the rounded sRGB branch discontinuity), and 5e-7 OkLab round-trip tolerance
for representative extended SDR values. These are CPU test tolerances, not
claims about unbounded numerical conditioning or Float16 GPU output.

Rendering integration must retain the backend distinction: Metal composes in
linear P3; Windows converts bounded P3 into linear scRGB, where valid output can
have negative and above-one components. Float16 storage does not define a color
space. Existing instanced shaders remain encoded-sRGB until an explicit wide
output contract is implemented.


## Explicit gamut mapping and CPU gradients

`LinearRgb::map_gamut(destination, policy)` first converts primaries, then applies
`GamutMapping::Clip` or `ReduceChroma`. Both preserve already-in-gamut destination
values. `ReduceChroma` searches the neutral-connected OkLCh chroma interval at
fixed lightness/hue, using an absolute chroma tolerance of 1e-7 and at most 64
iterations. It returns the last strictly in-gamut candidate. Out-of-range
lightness maps to black/white. A clipped neutral fallback absorbs rounded matrix
error; exceptionally large extended inputs may exhaust the iteration budget and
return that fallback, or fail if intermediate arithmetic is non-finite. This is
a simple deterministic policy, not ICC relative colorimetric or CSS local MINDE.

`LinearRgba` adds validated straight alpha in [0,1]. `LinearGradient` samples two
fixed stops at normalized positions 0 and 1, with caller-defined start/end
coordinates. Points project onto that axis; outside samples extend endpoint
colors, and coincident endpoints select the second stop. Arbitrary stop ordering
and duplicate positions do not arise in this two-stop API. Non-finite geometry,
sample positions and arithmetic are rejected.

Choose linear RGB (in output primaries), OkLab, or OkLCh interpolation. Components
are alpha-premultiplied for interpolation and divided by the interpolated alpha;
OkLCh hue is interpolated separately, without alpha weighting. Zero-alpha results
canonicalize RGB to zero. Exact opaque/nonzero-alpha endpoints retain their RGB
without a perceptual round trip. Near-neutral chroma <=1e-7 and fully transparent
stops borrow the other stop's hue; if both are powerless, hue stays zero. This
threshold is local to interpolation and does not change stored OkLCh values.
Hue policies follow the shorter, longer, increasing and decreasing arcs; exactly
180-degree ties retain the original direction, and equal meaningful hues under
the longer policy take a full turn. Missing CSS components are not supported.

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
let bounded = midpoint.map_gamut(RgbSpace::DisplayP3, GamutMapping::ReduceChroma)?;
let associated_output = bounded.premultiplied();
# Ok::<(), xgpui_color::ColorError>(())
```

Sampling returns straight linear RGB without automatic mapping. Map after
interpolation and before final output association. These are CPU reference APIs;
GPU shader integration and a drawable gradient element are subsequent work.

## Reusable WGSL helpers

`WGSL_COLOR_MATH` is a resource-free WGSL source string. Prepend it once to
a shader module; all helper names use the `gamut_` prefix. It declares no entry
points, bindings or `Parameters` type, so the fragment-authoring API can include
it with the caller's own shader source. It adds no runtime compiler dependency;
Naga is a development dependency for validation.

The helper contract uses finite f32 inputs and these IDs (validate IDs before
upload; other values are outside the contract):

| Parameter | IDs |
| --- | --- |
| RGB space | 0=sRGB, 1=Display P3, 2=Rec.2020 |
| Mapping policy | 0=clip, 1=reduce chroma |
| Interpolation model | 0=linear RGB, 1=OkLab, 2=OkLCh |
| Hue policy | 0=shorter, 1=longer, 2=increasing, 3=decreasing |

`gamut_decode`/`gamut_encode` apply transfer functions;
`gamut_convert` changes linear RGB primaries. `gamut_to_xyz`/`gamut_from_xyz`,
`gamut_to_oklab`/`gamut_from_oklab`, and `gamut_to_oklch`/`gamut_from_oklch`
provide intermediate representations. WGSL matrix constructors list columns,
matching the CPU row-major matrices' column-vector multiplication semantics.

`gamut_map` expects linear RGB already in the selected destination space.
`gamut_interpolate` accepts straight-alpha linear RGB stops in that same space
and a normalized sample position. Project geometry on the host or in caller
code; the helper handles interpolation, endpoint extension and zero alpha. It
returns straight linear RGB with no automatic mapping. All IDs and alpha values
must satisfy the CPU contracts. GPU helpers cannot return `ColorError`; validate
host inputs and keep intermediate arithmetic within f32 range. Arbitrary f64
extended values cannot necessarily be uploaded as finite f32.

Native Metal compute tests compare 666 conversion/mapping/interpolation cases
against the CPU reference with absolute tolerance 3e-5, using shader fast math
disabled. Naga validation also translates the test module to MSL, HLSL and
SPIR-V. Translation does not establish native D3D11 execution. Float16 render
target behavior remains a separate rendering-integration check.

These helpers do not change the existing instanced shader output contract:
returning linear P3 from an ordinary encoded-sRGB program is incorrect.
Use `xgpui_shader::instanced::ShaderProgram::new_linear_p3` for the explicit
native wide-output contract described in the instanced shader guide.
