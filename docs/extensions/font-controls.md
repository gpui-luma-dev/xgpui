# Variable fonts and OpenType inspection

`xgpui 3.0` adds precise variable-font axes and resolved-face feature inspection.
Native CoreText on macOS, DirectWrite on Windows, and the Parley backend apply
the same settings through shaping, metrics, wrapping, fallback and rasterization.
`xgpui-parley 0.4` also provides an optional quantized preview controller.

## Try the live inspector

[Open the Wasm font inspector](../examples/extensions/?demo=font-controls), or
choose [WebGPU](../examples/extensions/?demo=font-controls&backend=webgpu) /
[WebGL2](../examples/extensions/?demo=font-controls&backend=webgl) explicitly.
It reuses the native lab's GPUI sliders and bundled fonts. Parley/HarfRust shapes
and Skrifa/tiny-skia rasterizes the actual glyphs in Wasm; CSS text is not used.
WebGPU/WebGL2 chooses the GPU renderer, not a different text engine.

For native CoreText/DirectWrite versus Parley, run from the repository root:

```sh
just text --backend both
```

Each window has independent Roboto Flex sliders for **weight**, **width**,
**slant**, and **optical size**, plus Default, Condensed, Slanted, and Wide presets.
The large specimen, wrapping specimen and measured advance update together.
Focus a slider and use arrows, Shift for larger steps, or Home/End for limits.
Feature tags and available axis ranges appear with the static samples.

Run the lab tests, with backtraces enabled:

```sh
just lab-test
```

To focus on the slider interaction tests:

```sh
just lab-test live_axis -- --nocapture
```

The labs and tests were confirmed working on macOS and Windows on November 5,
2026. Native and Parley output need not be pixel-identical. A subtle slant
appearance difference between the tested macOS and Windows displays was reported
in both panels, with matching endpoints; it did not block acceptance. This does
not establish equivalent rendering on every display or validate every IME path.

## Set precise axes

Load or install a font that supports the requested axes. Roboto Flex is bundled
with the lab; applications provide their own fonts.

```rust
use gpui::{FontTag, FontVariations, div, prelude::*};

let axes = FontVariations::new([
    (FontTag::WEIGHT, 650.0),
    (FontTag::WIDTH, 85.0),
    (FontTag::SLANT, -7.5),
    (FontTag::OPTICAL_SIZE, 14.0),
])?;

let specimen = div()
    .font_family("Roboto Flex")
    .font_variations(axes)
    .child("Hamburgefonts 012345");
```

For explicit text runs, assign the same settings to `gpui::font(family).variations`.
`TextStyle::font_variations` participates in inheritance and refinement.
Coordinates serialize as a tag-to-number map, for example
`{"slnt": -7.5, "wght": 650.0}`.

Use these constants for the five [registered axes](https://learn.microsoft.com/en-us/typography/opentype/spec/dvaraxisreg):

| Constant | OpenType tag | Coordinate meaning |
| --- | --- | --- |
| `FontTag::WEIGHT` | `wght` | Weight; conventionally 400 for regular |
| `FontTag::WIDTH` | `wdth` | Percentage of normal width |
| `FontTag::SLANT` | `slnt` | Angle in degrees; negative values lean right |
| `FontTag::OPTICAL_SIZE` | `opsz` | Optical design size in typographic points |
| `FontTag::ITALIC` | `ital` | Italic design; conventionally 0 for roman and 1 for italic |

Constants are already validated tags and need no `.parse()` or error handling.
`FontVariations::new` remains fallible because coordinates must be finite. The
font determines its supported ranges: constants do not guarantee a font has that
axis or impose a universal range. Italic design and slant angle are separate axes.
Explicit optical size does not track GPUI's pixel font size automatically.

Custom axes remain available through validated parsing:

```rust
let grade = "GRAD".parse::<FontTag>()?;
let custom_axes = FontVariations::new([(grade, 25.0)])?;
```

The spelling change does not alter serialization: constants still write standard
tags such as `wght` and `slnt` into the same tag-to-number map.

`FontTag` validates four printable ASCII bytes. `FontVariations` accepts finite
signed `f32` coordinates, sorts tags, keeps the last duplicate and normalizes
negative zero. Author settings retain their precision; core does not quantize
them. Invalid tags, NaN and infinity are rejected.

Existing family/weight/style matching chooses a face first. Explicit axis values
override corresponding implicit coordinates, then clamp to that face's range.
An unsupported axis is ignored on that face, while the original request remains
available to fallback faces. Each fallback resolves its own effective coordinates.
Empty settings preserve existing backend behavior. Explicit `opsz` uses font
design units and stays fixed across text sizes; automatic optical sizing is not
part of this API.

## Inspect a resolved face

```rust
use gpui::{FontQuery, font};

let text_system = window.text_system();
let capabilities = text_system.font_capabilities();
let id = text_system.resolve_font(&font("Roboto Flex"));

match text_system.font_axes(id)? {
    FontQuery::Unsupported => { /* Hide axis inspection. */ }
    FontQuery::Supported(axes) => {
        for axis in axes {
            println!("{}: {} .. {} (default {})",
                axis.tag, axis.min_value, axis.max_value, axis.default_value);
        }
    }
}
let features = text_system.opentype_features(id)?;
```

`FontCapabilities` reports `feature_introspection`, `axis_introspection`, and
`static_variations`. Check `static_variations` before offering editing controls:
the existing `resolve_font` convenience method can fall back after resolution
errors. A `FontId` belongs to the text system that resolved it.

Queries distinguish **unsupported**, **successful empty results**, and **errors**.
`VariationAxisInfo` contains tag, minimum, default, maximum and hidden status.
`OpenTypeFeatureInfo` identifies each tag's GSUB/GPOS provenance; tags are sorted
and duplicates merged. Feature presence does not guarantee an effect on a given
script, language or string, or describe its allowed parameter values. Existing
`FontFeatures` supports numeric values as well as on/off settings.

Core owns the private `fvar`, GSUB and GPOS parsers. The default
`PlatformTextSystem::font_axes` and `opentype_features` methods call the backend's
`font_table(FontId, FontTag)` hook. Backends obtain bytes from their actual
resolved face; core interprets the metadata. The hook returns `Unsupported`,
`Supported(None)` for an absent table, `Supported(Some(bytes))` for a present
table, or an error for a failed read/invalid ID. No public parser function,
additional package, or copied parser implementation is required.

## Backend support

| Backend | Feature/axis inspection | Explicit static axes |
| --- | --- | --- |
| Apple font-kit / CoreText | Supported | Supported; macOS labs/tests verified |
| Windows / DirectWrite | Supported | Supported when the `IDWriteTextLayout4` runtime probe succeeds; Windows labs/tests verified |
| Parley / HarfRust / Skrifa | Supported | Supported for flat lines and paragraphs; macOS/Windows labs/tests verified |
| Parley in Wasm | Supported | Supported; Wasm inspector interactions verified on WebGPU and WebGL2 |
| Cosmic / Linux | Unsupported | Nonempty explicit axis requests rejected |
| GPUI default web text backend (Cosmic/Canvas) | Unsupported | Nonempty explicit axis requests rejected; this is an integration limit, not a browser-wide restriction |

The Wasm inspector was verified on this host with both renderers, including
pointer controls, keyboard input, presets and restored measurements. This is not
exhaustive browser/device acceptance.

Shared Apple code also serves iOS, but macOS acceptance does not establish iOS
runtime validation. Cosmic and browser parity are tracked in
[#141](https://github.com/gpui-luma-dev/xgpui/issues/141).

## Interactive preview policy

`xgpui_parley::VariationPreview` is an extension utility that returns ordinary
GPUI `Font` descriptors; it works with native backends too. Retain one controller
for an inspector's lifetime. Supply face-specific axis ranges, per-axis intervals
and a positive combination capacity to `VariationPreview::new`, then call
`font(&author_settings)` for each update. Quantization happens before resolution,
so layout and paint use the same coordinates. Commit precise author settings
separately from the preview.

The lab admits at most **512 combinations per window**, using steps of
`wght=10`, `wdth=1`, `slnt=0.25`, and `opsz=1`. Once full, it retains the last
preview and displays a message. Revisiting admitted settings and Default still
works; restarting creates a fresh session. These are lab policy choices, not
universal font defaults.

Admission limits descriptors in that controller. They do not set a global
CPU/GPU memory budget or evict fonts, glyphs or atlas pages. Retained layouts keep
valid font IDs. Resource budgets and frame-safe eviction remain
[#139](https://github.com/gpui-luma-dev/xgpui/issues/139).

## Upgrade to core 3.0

The breaking changes are new fields on `Font`, `TextStyle`, and
`TextStyleRefinement`. Add `variations: Default::default()` to exhaustive font
literals, `font_variations: Default::default()` to exhaustive text-style literals,
and `font_variations: None` to refinement literals. Patterns can use `..`.
Constructor/default-based callers generally only need recompilation against the
matching crate family. No axis request is introduced implicitly.

The inspection and styling methods are additive. `PlatformTextSystem::font_table`
has an unsupported default, so existing custom backends compile without an
override. See [core API migration](gpui-public-api.md#font-axes-and-inspection-core-30)
and [release versions](../WHATS_NEW.md#font-controls-crate-versions).
