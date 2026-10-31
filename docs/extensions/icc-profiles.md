# Use a supplied RGB ICC profile

Enable the optional parser when your application already has profile bytes.
The parser supplies validated colorants and curves to the f64 color core. It
does not decode image files, select a monitor profile or run a general ICC
transform engine.

```toml
[dependencies]
xgpui-color = { path = "../../extensions/crates/xgpui-color", features = ["icc"] }
```

## Parse once, then attach the definition to pixels

```rust
use std::sync::Arc;
use xgpui_color::icc::IccRgbProfile;
use xgpui_media::pipeline::{AlphaMode, PixelColor};

fn interpretation(profile_bytes: &[u8]) -> anyhow::Result<PixelColor> {
    let profile = IccRgbProfile::parse(profile_bytes)?;
    Ok(PixelColor::Custom {
        rgb: Arc::new(profile.into_rgb_definition()),
        alpha: AlphaMode::Straight,
    })
}
```

Pass that interpretation to `MediaLoader::load_pixel_buffer` with the decoded
RGBA samples, an explicit storage space and mapping policy. Reuse the immutable
definition across pixels; parse outside rendering and keep admission of profile
bytes/work on the application's bounded workers. Source alpha remains with the
caller. [Pixels and metadata](pixels-and-metadata.md) covers the full flow.

## Supported subset

| Item | Current contract |
| --- | --- |
| Profiles | ICC v2/v4 RGB input/display matrix/TRC, XYZ PCS with standard encoded D50 header illuminant |
| Curves | Linear, gamma, monotone normalized u16 samples, supported parametric types 0/3/4 |
| Evaluation | f64 color core; sampled values interpolate continuously |
| Source domain | Finite encoded RGB in 0–1; alpha is supplied separately |
| Matrix | Preserve serialized colorants, including non-neutral matrix sums; require finite/invertible/well-conditioned data |
| Adaptation | Header PCS reference to D65 once; do not apply chad or media-white tags again |
| Errors | Limit, unsupported, malformed or invalid color; no silent sRGB fallback |

Limits include 512 KiB/profile, 32 tags, 65,536 samples/curve and 4 KiB per
admitted informational tag. Exact shared tag ranges are allowed; partial overlap
is rejected. Large parametric coefficients outside the parser's exact admitted
range are rejected. The adapter rejects CICP combinations and unsupported
transform tags instead of guessing precedence.

LUT profiles, CMYK, gray, HDR/iccMAX, arbitrary rendering-intent transforms,
black-point compensation and display calibration are outside this subset.
Informational luminance, measurement, black-point, technology and viewing-text
tags do not add those operations to the relative matrix/TRC decode.

## Why sRGB2014 is not replaced with named sRGB

A profile's name is not a conversion rule. The pinned sRGB2014 fixture contains
a 1024-entry curve and a small encoded neutral-axis offset. Both are retained.
`RgbDefinition::from_relative_xyz_matrix` separates the declared XYZ reference
white from the actual transform's matrix sum. The neutral-space constructors
remain available for definitions that must map neutral RGB onto their declared
white. No column normalization or wider conversion tolerance is used to admit
the profile.

## Tested profiles and boundaries

The pinned published corpus accepts Compact v4 sRGB, Display P3,
Adobe-compatible RGB, and the ICC-authored sRGB2014 mirror. Compact v2 fixtures
with overlapping packed tag ranges are intentionally rejected; this describes
our admission policy, not a claim that the upstream files are corrupt.
Grayscale and CMYK fixtures are also explicit rejections.

There are 4,108 independent 120-digit exact-tag vectors for the accepted
published profiles, plus synthetic curve and adaptation tests. They cover
near-black, mixed colors, endpoints, threshold neighbors and sampled knots.
The f64 source-energy precision budgets are separate from FP16 storage budgets.
A secondary Little CMS endpoint check confirms the retained relative PCS matrix;
its interpolation behavior does not define the core's sampled-curve contract.

Try [identical pixels with different metadata](../examples/extensions/?demo=pixel-metadata)
in Wasm, or inspect them in the [native lab](color-labs.md). Provenance, licenses,
hashes and detailed results live with the
[profile fixtures](https://github.com/gpui-luma-dev/xgpui/tree/main/extensions/crates/xgpui-color/tests/reference/real-icc).
Historical engine evaluations are retained in
[validation notes](https://github.com/gpui-luma-dev/xgpui/tree/main/validation/icc-import-probe).
