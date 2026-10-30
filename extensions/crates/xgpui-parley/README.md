# xgpui-parley

Experimental text extension for **xgpui** using Parley and HarfRust. It provides
text shaping, resolved-face metrics, grayscale rasterization and opt-in BiDi
paragraph layout. GPUI retains native input, frame caches and glyph atlas ownership.

## Dependencies

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.0.0" }
xgpui-parley = "0.2.1"
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

The example also uses `gpui_platform` from the `xgpui-platform` package at `1.0.0`.
The default family must be present; `.SystemUIFont` maps to that family.
`FontFallbacks` specifies ordered fallback. Optional `system-fonts` discovery is
off by default, and native platform initialization remains unchanged.

## Lines and paragraphs

| Path | Supported behavior |
| --- | --- |
| Flat line | One size, LTR, unwrapped, start-aligned; UTF-8 indices and baseline-relative positions. |
| Paragraph | Automatic direction, multiple descriptor runs, reflow, hit testing, carets and selection geometry. |

Use `window.text_system().layout_paragraph(...)` and `gpui::paint_paragraph`
for paragraph integration. Colors, backgrounds, decorations and editing behavior
remain application-controlled. See the
[paragraph contract](https://github.com/gpui-luma-dev/xgpui/blob/main/extensions/crates/xgpui-parley/PARAGRAPH-BIDI.md)
for source positions, affinity, cache ownership and reflow.

The fallible line API reports unsupported input. GPUI's infallible line hook
uses finite `.notdef` fallback geometry and exposes an error-handler hook;
a fallback result should not be mistaken for complete shaping support.

## Current limits

Synthetic styles, color/bitmap-only emoji, LCD rasterization, dilation, inline
boxes, vertical layout and split-style cluster fragments are unsupported.
Raster requests above 4096 pixels per device dimension are rejected.
Explicit paragraph direction/language and advanced styling remain follow-ups.

Retained face IDs are not recycled, and old layouts remain rasterizable after
font loading. Pin fonts, fallback, variation settings and shaping versions for
deterministic results; host discovery and raster pixels can still differ.
Windows/Linux use the same adapter, but runtime validation remains outstanding.

## Guides and examples

- [Text backend setup, supported subset and validation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/parley-text.md)
- [Text and editing/IME lab](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/text-lab/README.md)
- [Bundled test fonts and licenses](https://github.com/gpui-luma-dev/xgpui/blob/main/extensions/crates/xgpui-parley/tests/fonts/README.md)

Run `just text` from the repository root for the native comparison lab.

## GPUI public API changes: additions and compatibility

Use this crate with xgpui's matching framework packages. Its integration depends
on [xgpui's GPUI API changes](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md#added-text-backend-and-paragraph-seams).
See the [compatibility guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/parley-text.md#gpui-public-api-changes-additions-and-compatibility)
for additions, affected public types and migration requirements.

## Project

[Repository](https://github.com/gpui-luma-dev/xgpui) · [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md) ·
[Website](https://xgpui.dev/). Licensed under Apache-2.0.

This extension uses its own `0.2.0` version, separate from xgpui framework
`1.0.0` and the upstream GPUI baseline.
