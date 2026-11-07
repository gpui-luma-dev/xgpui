# xgpui-parley

Opt-in Parley 0.12/HarfRust text backend: flat-LTR compatibility, BiDi paragraphs,
resolved-face metrics, precise variable-font axes and grayscale rasterization.
GPUI retains platform input,
frame caches and glyph atlas ownership.

## GPUI public API changes: additions and compatibility

This crate adds `Application::with_platform_and_text_system` to select the
backend before GPUI caches are built, optional `PlatformTextSystem::shape_paragraph`,
paragraph layout/cache geometry, and `paint_paragraph`. The optional trait method
defaults to Unsupported, so existing text backends need no required override.
Existing application construction and native initialization retain their behavior.
Additive method names can still collide with downstream extension methods.

Parley uses GPUI's existing glyph atlas; it does not require custom-draw scene
variants or wide-gamut presentation. Its flat-line supported subset and fallback
behavior are documented below; those are extension behavior, not required
changes to existing GPUI callers.

**[GPUI public API changes: additions and compatibility](gpui-public-api.md#added-text-backend-and-paragraph-seams).**

## Construction

```toml
[dependencies]
gpui = { package = "xgpui", version = "=3.0.0" }
xgpui-parley = { version = "=0.4.0", features = ["system-fonts"] }
```

```rust
use gpui::{Application, PlatformTextSystem};
use std::sync::Arc;
use xgpui_parley::ParleyTextSystem;

// Register application fonts in font_context before construction.
let backend = Arc::new(ParleyTextSystem::new(font_context, "IBM Plex Sans")?);
let app = Application::with_platform_and_text_system(
    gpui_platform::current_platform(false), backend,
);
```

The explicit default family must exist; `.SystemUIFont` maps to it. Ordered
fallback uses `FontFallbacks`. `system-fonts` is optional and off by default.
Native platform initialization is unchanged. Native and Parley font-controls
labs/tests are verified on macOS and Windows. Linux runtime acceptance and
paragraph conversion IMEs remain separate checks.

## Variable fonts and inspection

Version 0.4.0 accepts `Font::variations` in flat-line and paragraph shaping and
retains the actual resolved coordinates for metrics and rasterization. Use
`FontTag::{WEIGHT, WIDTH, SLANT, OPTICAL_SIZE, ITALIC}` for standard axes, and
validated tag parsing for custom axes; see the [typed styling example](font-controls.md#set-precise-axes). Explicit
signed/fractional axes, clamping, fallback and instance isolation share core's
[font-controls contract](font-controls.md). `font_axes`, `opentype_features`, and
`font_capabilities` are available through GPUI's text system. Parley supplies raw
resolved-face tables through the backend hook; the parsers remain private in core.

### Inspect axes and OpenType features

After registering the font with the selected backend, query it through the
window's text system:

```rust
use gpui::{FontQuery, font};

let text_system = window.text_system();
let capabilities = text_system.font_capabilities();
let font_id = text_system.resolve_font(&font("Roboto Flex"));

if capabilities.axis_introspection {
    match text_system.font_axes(font_id)? {
        FontQuery::Unsupported => { /* Hide the axis inspector. */ }
        FontQuery::Supported(axes) => {
            for axis in axes {
                println!("{}: {} .. {} (default {}, hidden={})",
                    axis.tag, axis.min_value, axis.max_value,
                    axis.default_value, axis.hidden);
            }
        }
    }
}

if capabilities.feature_introspection {
    match text_system.opentype_features(font_id)? {
        FontQuery::Unsupported => { /* Hide the feature inspector. */ }
        FontQuery::Supported(features) => {
            for feature in features {
                println!("{}: GSUB={}, GPOS={}",
                    feature.tag, feature.substitution, feature.positioning);
            }
        }
    }
}
```

A successful empty list differs from unsupported inspection; `?` propagates
read/parsing errors. Results describe the resolved face, including any font
substitution performed by `resolve_font`. Use IDs from this same text system.
Feature presence does not guarantee an effect on every script or string.

### Set axes with typed tags

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

Check `capabilities.static_variations` before offering axis editing. Standard
tags need no parsing; coordinates remain validated and clamp per resolved face.

### Interactive previews

`VariationPreview` is an optional extension utility for a fixed font descriptor:
configure axis-specific intervals and a finite capacity, then obtain quantized
GPUI fonts before resolution. It works with native text backends too. Retain the
controller throughout the inspector session; keep precise author values separate.
It never recycles font IDs or evicts atlas resources. See the
[preview policy](font-controls.md#interactive-preview-policy) and live lab.

## Paragraphs and input

```rust
// FontRuns contain descriptor IDs resolved by this application's TextSystem.
let paragraph = window.text_system().layout_paragraph(
    text, gpui::px(24.0), &font_runs, width,
)?;
let hit = paragraph.hit_test(mouse_position - bounds.origin)?;
let caret = paragraph.caret_bounds(hit)?;
let fragments = paragraph.selection_bounds(anchor, hit)?;

// Explicit base direction and language use the extension API directly.
// `parley` is the Arc<ParleyTextSystem> installed as the app's text backend.
let options = xgpui_parley::ParagraphOptions {
    direction: xgpui_parley::TextDirection::RightToLeft,
    language: Some(xgpui_parley::TextLanguage::parse("ur")?),
};
let urdu = parley
    .prepare_paragraph_with_options(&text, gpui::px(24.0), &font_runs, options)?
    .layout(width)?; // Cache shapes/reflows yourself; GPUI's cache has no options.

// During element paint:
window.handle_input(&focus, gpui::ElementInputHandler::new(bounds, entity), cx);
gpui::paint_paragraph(paragraph.as_ref(), bounds.origin, color_at, window)?;
```

The input entity implements `EntityInputHandler`. It translates UTF-16 edits to
source UTF-8 and uses the painted paragraph for IME geometry. Candidate anchors
use the requested caret/fragment; marked text uses selection fragments. Edits
invalidate geometry until repaint. Apply origin/scrolling once at the UI edge.

## The typeset() element

`typeset("…")` wraps the paragraph path as an ordinary GPUI element, using only
public GPUI APIs (`Window::request_measured_layout`, a GPUI global and
`paint_paragraph`). Install it with the backend, then use it like text:

```rust
xgpui_parley::typeset::install(backend.clone(), cx); // the installed Arc<ParleyTextSystem>

div().w(px(150.0)).truncate().child(
    xgpui_parley::typeset("تقرير الإيرادات").rtl().language("ar".parse()?),
)
```

It inherits GPUI's text style, including `white_space`, all three
`text_overflow` modes, `line_clamp` and `text_align` (Left/Right act as
start/end). Mixed styles use GPUI's model: `with_highlights` and `with_runs`, as
on `StyledText`, including per-run fonts, colors, backgrounds, underlines and
strikethroughs; styles follow truncated text. Wrapping text reports min-content and max-content widths, as in
taffy's Parley example. Truncation is logical and verified by reshaping. Shapes,
reflows and fits are cached in the engine. Without `install` it renders as GPUI
text. Under this backend, plain GPUI text uses the left-to-right-only flat-line
path, so right-to-left text needs `typeset()`. The text lab's Paragraph panel
draws every label with `typeset()`: truncated cells, a tooltip and a clamp, plus
a UI-language toggle (en/he/ar/fa/ur) that mirrors the panel for right-to-left
languages.

## GPUI versus extension APIs

| Added to GPUI | Purpose |
| --- | --- |
| `Application::with_platform_and_text_system(Rc<dyn Platform>, Arc<dyn PlatformTextSystem>)` | Select the backend before cache construction |
| `PlatformTextSystem::shape_paragraph(ParagraphRequest)` | Optional provider hook; default returns Unsupported |
| `PlatformParagraphShape::layout(width)` | Immutable width-dependent reflow |
| `PlatformParagraphLayout` | Source, size, visual glyph runs, hit testing, caret, movement and selection geometry |
| `ParagraphRequest`, `ParagraphPosition`, `TextAffinity`, `VisualDirection` | Backend-neutral request and interaction types |
| `WindowTextSystem::layout_paragraph(text, size, runs, width)` | GPUI-owned shaping/reflow cache entry point |
| `paint_paragraph(layout, origin, color_at, window)` | Visual glyph painting through the existing atlas |

`ParleyTextSystem`, `FaceRegistry`, `RegisteredFace`, `GlyphRasterizer`,
`ParagraphShape`, `ParagraphLayout`, `VariationPreview`, `ParagraphOptions`,
`TextDirection`, `TextLanguage`, `Typeset` and `typeset::install` belong to **xgpui-parley**, not GPUI.
Explicit paragraph direction and language add no GPUI API.
Parley/HarfRust/Skrifa types do not enter the core signatures.

The paragraph additions change no existing public struct fields or required trait
methods. Core 3.0 font controls separately add descriptor/style fields; see the
[migration table](gpui-public-api.md#font-axes-and-inspection-core-30). Existing backend implementations need no paragraph override;
`Application::with_platform` and `PlatformTextSystem::layout_line` retain their
contracts. `EntityInputHandler`, `ElementInputHandler` and glyph/emoji paint
methods are existing GPUI APIs. Internal changes add paragraph frame-cache
entries and invalidation; the macOS variable-weight correction changes native
font selection behavior without adding public API.

## Contracts

| Boundary | Contract |
| --- | --- |
| Font identity | Requested descriptors distinguish features/fallbacks/variations; shaped IDs retain actual blob, face index and normalized variation location |
| Lifetime | IDs are not recycled; old layouts remain rasterizable after font registration |
| Shape cache | GPUI key: source, ordered FontRuns, size |
| Reflow cache | Same key plus width; immutable layout Arcs; width changes reuse shaping |
| Invalidation | Both cache levels follow frame retention/reuse and font generation; backend calls run outside cache locks |
| Geometry | Logical UTF-8 plus affinity; visual glyph order; paragraph-local positions include baseline/glyph offsets |
| Paint | Existing bounds cache and sprite atlas; colors resolve by logical index; decorations/backgrounds remain caller-owned |
| Native providers | Optional paragraph hook defaults to Unsupported |

Parley owns shaping, BiDi, breaking and interaction geometry. Reflow clones and
rebreaks retained Parley layout. Skrifa metrics/rasterization use the same
registered instance; a bounded LRU retains hinting setup and one staged outline
avoids repeating extraction between matching bounds and raster calls. Shaping
and rasterization use separate locks. No extension layout or
bitmap cache is added. [Core API additions](gpui-public-api.md).

## Supported subset

Flat-line compatibility requires LTR, one size, scale 1 and start alignment.
`try_layout_line` returns errors; the infallible hook substitutes `.notdef` with
original indices and reports rejection outside the backend lock.

Paragraphs support mixed LTR/RTL, automatic direction (explicit direction and a
paragraph language through `prepare_paragraph_with_options`), start alignment, one size,
multiple font runs and U+2028 hard breaks. Callers split document paragraphs;
the lab normalizes pasted line endings to U+2028.

Color emoji (COLR v0/v1, sbix and CBDT) are supported in plain GPUI text and
`typeset()`; registered emoji families are automatic fallbacks. Synthetic styles,
LCD, dilation, inline boxes, vertical text and split-style cluster fragments are
unsupported. Raster size/bounds are capped
at 4096 device pixels. Explicit alignment, per-run languages, advanced styling,
accessibility and broader BiDi/UAX #14 conformance remain follow-ups.

## Lab and validation

[The Wasm font inspector](../examples/extensions/?demo=font-controls) runs the
same GPUI axis controls with Parley shaping and rasterization in Wasm, through
WebGPU or WebGL2. It displays axis ranges, GSUB/GPOS feature tags and precise
static samples beside the live preview. This is separate from the default GPUI
web text backend, whose explicit-axis requests remain unsupported.

```sh
just text --backend both
just text --workload paragraph
just text --workload paragraph --timings-only
just text --workload scroll --scroll-frames 640 --exit-after-scroll
just lab-test
```

The lab covers bundled fallback/variable fonts, mixed scripts, wrapping, caret,
selection, clipboard and composition. First/warm/reflow CPU diagnostics separate
cache hits from new-width layout; seed timings exclude GPU work. Native optical
size and host fallback can differ, so matched fonts do not imply pixel identity.

Automated checks cover face/cache identity, visual conversion, atlas reuse,
UTF-16 composition/candidate replacement and stale/wrapped IME geometry.
Manual macOS results (2026-10-24): single-line native/Parley Japanese–Romaji popup
selection/commit/cancel passed; paragraph U.S. accent-panel placement, selection,
commit/cancel passed by user confirmation without keyboard-setting changes.
The November 5, 2026 native/Parley font-controls labs and unit tests passed on
macOS and Windows. Paragraph conversion IMEs and Linux runtime/IME acceptance
remain unvalidated.
