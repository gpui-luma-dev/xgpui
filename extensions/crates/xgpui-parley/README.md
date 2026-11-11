# xgpui-parley

Experimental text extension for **xgpui** using Parley and HarfRust. It provides
text shaping, resolved-face metrics, grayscale rasterization and opt-in BiDi
paragraph layout. GPUI retains native input, frame caches and glyph atlas ownership.

## Dependencies

```toml
[dependencies]
gpui = { package = "xgpui", version = "=3.0.0" }
xgpui-parley = "=0.5.0"
```

## Select the backend

Register the application's fonts in `font_context`, then construct the backend
before GPUI builds its text caches:

```rust,ignore
use gpui::Application;
use std::sync::Arc;
use xgpui_parley::ParleyTextSystem;

let backend = Arc::new(ParleyTextSystem::new(font_context, "IBM Plex Sans")?);
let app = Application::with_platform_and_text_system(
    gpui_platform::current_platform(false), backend,
);
```

The example also uses `gpui_platform` from the `xgpui-platform` package at `2.0.2`.
The default family must be present; `.SystemUIFont` maps to that family.
`FontFallbacks` specifies ordered fallback. Optional `system-fonts` discovery is
off by default, and native platform initialization remains unchanged.

## Variable fonts

Use typed axis tags with core's precise variation settings:

```rust,ignore
use gpui::{FontTag, FontVariations};

let mut face = gpui::font("Roboto Flex");
face.variations = FontVariations::new([
    (FontTag::WEIGHT, 650.0),
    (FontTag::WIDTH, 85.0),
    (FontTag::SLANT, -7.5),
    (FontTag::OPTICAL_SIZE, 14.0),
])?;
```

`FontTag::ITALIC` selects the registered italic-design axis; custom tags can be
parsed explicitly. Both paragraph and flat-line paths retain effective axes for
metrics and rasterization. GPUI's `font_axes`, `opentype_features` and
`font_capabilities` expose resolved-face inspection. Optional `VariationPreview`
quantizes before resolution and limits admitted descriptors without evicting
resources. See the [font-controls guide](https://xgpui.dev/font-controls.html)
for styling, fallback, backend support and migration.

## Lines and paragraphs

| Path | Supported behavior |
| --- | --- |
| Flat line | One size, LTR, unwrapped, start-aligned; UTF-8 indices and baseline-relative positions. Plain GPUI text uses this path, so right-to-left text is rejected. |
| `typeset()` | Element over the paragraph path: inherited GPUI text style, direction/language, wrapping, truncation and line clamps. |
| Paragraph | Automatic direction through GPUI; explicit base direction and paragraph language through `prepare_paragraph_with_options`; multiple descriptor runs, reflow, hit testing, carets and selection geometry. |

Use `window.text_system().layout_paragraph(...)` and `gpui::paint_paragraph`
for paragraph integration. Colors, backgrounds, decorations and editing behavior
remain application-controlled. See the
[paragraph contract](https://github.com/gpui-luma-dev/xgpui/blob/main/extensions/crates/xgpui-parley/PARAGRAPH-BIDI.md)
for source positions, affinity, cache ownership and reflow.

The fallible line API reports unsupported input. GPUI's infallible line hook
uses finite `.notdef` fallback geometry and exposes an error-handler hook;
a fallback result should not be mistaken for complete shaping support.

## The `typeset()` element

`typeset("…")` is a text element shaped by this backend. Install the backend
once, then use it wherever GPUI text would go:

```rust,ignore
// Inside Application::run, with the same Arc passed to with_platform_and_text_system:
xgpui_parley::typeset::install(backend.clone(), cx);

div()
    .w(px(150.0))
    .truncate() // inherited: nowrap + end ellipsis, as for GPUI text
    .child(xgpui_parley::typeset("דוח הכנסות report").rtl().with_id("cell"))
```

It inherits the parent's text style: font, size, line height, color,
`white_space`, the three `text_overflow` modes, `line_clamp` and `text_align`
(`Left`/`Right` act as start/end, so they mirror for right-to-left text). It adds
`.direction(..)`/`.rtl()`/`.ltr()` and `.language(..)`. `.with_id(..)` exposes it
to screen readers as a label, as with `gpui::Text`.

- **Sizing:** wrapping text reports Parley's min-content and max-content widths
  to layout, so it can shrink to its longest word in flex layouts (native GPUI
  text reports its single-line width for both).
- **Truncation:** cuts are logical and checked by reshaping, so joined scripts and
  kerning measure correctly and the base direction puts the ellipsis on the
  correct side. Wrapped text with `line_clamp` truncates at the end.
- **Placement:** unbroken lines are positioned by the element (right edge for
  right-to-left start alignment); wrapped lines use Parley's alignment.
- **Caching:** shapes, reflows and fits live in the installed engine (a GPUI
  global), bounded by a least-recently-used policy.
- **Fallback:** without `install`, it renders as plain GPUI text.

- **Mixed styles:** `.with_highlights([(range, HighlightStyle)])` restyles byte
  ranges over the inherited style, and `.with_runs(Vec<TextRun>)` replaces it,
  as on `StyledText`. Runs may change font family, weight, style, features and
  variations (shaped per run), color, background, underline and strikethrough.
  Truncated text maps back to its source runs, so styles follow the kept text and
  the affix takes its neighbor's style. Decorations use GPUI's own placement.
  Runs share the inherited font size, and weights and styles need real faces.

Not yet supported: selection, editing and interactive ranges (links).
Plain GPUI text under this backend uses the flat-line path, which rejects
right-to-left text; use `typeset()` for it.

## Color emoji

Glyphs the resolved face has color data for are painted as GPUI emoji sprites,
in plain GPUI text and in `typeset()`. Supported formats: COLR v0 layers, COLR v1
paint graphs (solid, linear, radial and sweep fills, transforms, clips and
composite modes) and color bitmap strikes (sbix, as in Apple Color Emoji, and
CBDT, as in Noto Color Emoji). Register the platform emoji font like any other
font; families named "…Emoji…" with color tables become automatic last-resort
fallbacks, so text styles need not list them.

## Current limits

Synthetic styles, LCD rasterization, dilation, inline boxes, vertical layout and
split-style cluster fragments are unsupported. Color glyphs use the font's first
palette and black for the foreground color; monochrome bitmap strikes are not
drawn. COLR sweep and radial gradients are implemented but not yet verified
against a real font.
Raster requests above 4096 pixels per device dimension are rejected.
Languages apply per paragraph, not per run. Explicit alignment and advanced
styling remain follow-ups. An empty right-to-left paragraph places its caret
slightly inside the right edge, following Parley's nominal empty-line advance.

Retained face IDs are not recycled, and old layouts remain rasterizable after
font loading. Pin fonts, fallback, variation settings and shaping versions for
deterministic results; host discovery and raster pixels can still differ.
Windows/Linux use the same adapter. On 2026-11-04 the owner confirmed all native
labs, including text, and `just lab-test` passed on Windows. Linux runtime and
focused IME/assistive-technology acceptance remain separate checks; see the
[validation scope](https://xgpui.dev/local-development.html#windows-validation).

## Guides and examples

- [Text backend setup, supported subset and validation](https://xgpui.dev/parley-text.html)
- [Wasm font-controls demo](https://xgpui.dev/examples/extensions/?demo=font-controls) — the native lab’s sliders and font inspection, running through Parley in Wasm on WebGPU/WebGL2.
- [Text and editing/IME lab](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/text-lab/README.md)
- [Bundled test fonts and licenses](https://github.com/gpui-luma-dev/xgpui/blob/main/extensions/crates/xgpui-parley/tests/fonts/README.md)

Run `just lab-text` from the repository root for the native comparison lab.

## GPUI public API changes: additions and compatibility

Use this crate with xgpui's matching framework packages. Its integration depends
on [xgpui's GPUI API changes](https://xgpui.dev/gpui-public-api.html#added-text-backend-and-paragraph-seams).
See the [compatibility guide](https://xgpui.dev/parley-text.html#gpui-public-api-changes-additions-and-compatibility)
for additions, affected public types and migration requirements.

## Project

[Repository](https://github.com/gpui-luma-dev/xgpui) · [Release preparation](https://xgpui.dev/releasing.html) ·
[Website](https://xgpui.dev/). Licensed under Apache-2.0.

This extension uses its own `0.4.0` version, separate from xgpui core `3.0.0`
and the upstream GPUI baseline.
