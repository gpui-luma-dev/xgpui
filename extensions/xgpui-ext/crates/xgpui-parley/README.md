# xgpui-parley

Experimental Parley 0.12/HarfRust backend for GPUI: flat-LTR compatibility,
opt-in BiDi paragraphs, resolved-face metrics, and grayscale rasterization.
Native platform initialization remains unchanged.

[Website guide](../../../../docs/extensions/parley-text.md).

## Install the backend

```rust,ignore
let backend = Arc::new(ParleyTextSystem::new(font_context, "IBM Plex Sans")?);
let app = Application::with_platform_and_text_system(
    gpui_platform::current_platform(false), backend,
);
```

Preload the explicit default family. `.SystemUIFont` maps to it. Family names
are individual names; ordered fallback uses `FontFallbacks`. Optional
`system-fonts` discovery is off by default. Windows/Linux share this adapter;
[host validation](../../../../apps/text-lab/README.md#windows-and-linux-host-testing) remains outstanding.

## Ownership and identity

- One mutex protects reusable Parley contexts, face registry and hinting setup.
- Requested descriptor IDs distinguish features/fallbacks. Shaped runs use
  actual resolved-face IDs keyed by blob identity, collection index and
  canonical normalized variation coordinates.
- IDs are never recycled. Old layouts remain rasterizable after font loading;
  separately registered identical blobs may receive different IDs.
- Font batches validate before mutation. GPUI invalidates its caches after
  registration. The extension adds no layout or bitmap cache.
- Skrifa metrics/advances/bounds and rasterization use the same retained instance.
  Global bounds follow the font `head` table. A 64-entry LRU retains hinting setup.

## Flat-line bridge

```rust,ignore
let line = backend.try_layout_line(text, font_size, &font_runs)?;
// Lower-level conversion with a caller-owned registry:
let line = line_layout(&parley_layout, font_size, |run| faces.register(run))?;
```

Scale 1, start-aligned, one size, LTR, unwrapped; UTF-8 cluster indices and
baseline-relative positions are preserved. Unsupported input returns an error.
GPUI's infallible `layout_line` substitutes `.notdef` per Unicode scalar with
original indices and a permanent registered default face. Invalid sizes use
16px. `set_layout_error_handler` replaces the stderr diagnostic; callbacks run
outside the lock and only on uncached rejections. The missing-glyph sink likewise
runs outside the lock and currently covers only this flat-line path.

## Paragraph bridge

```rust,ignore
// GPUI-owned shape/reflow caches:
let paragraph = window.text_system().layout_paragraph(
    text, px(24.0), &font_runs, width,
)?;
let hit = paragraph.hit_test(local_point)?;
let caret = paragraph.caret_bounds(hit)?;
let rectangles = paragraph.selection_bounds(anchor, hit)?;
// During paint; colors are indexed by logical source byte:
gpui::paint_paragraph(paragraph.as_ref(), origin, color_at, window)?;
```

Automatic direction, start alignment, one size, multiple descriptor runs and
U+2028 hard breaks. CR/LF/U+2029 delimit separate caller-owned paragraphs.
Positions are logical UTF-8 plus affinity; geometry is paragraph-local. Reflow
clones/rebreaks retained Parley layout without rebuilding shaping. Glyph records
retain visual positions, logical indices and resolved IDs. Core sees no Parley
types. Native providers default to Unsupported.

GPUI owns both cache levels, frame retention and font-generation invalidation.
Painting uses existing glyph bounds/atlas paths; backgrounds and decorations
remain caller-owned. See [contracts](PARAGRAPH-BIDI.md) and the
[editing/IME lab](../../../../apps/text-lab/README.md).

## Limits

Synthetic styles, color/bitmap-only emoji, LCD, dilation, inline boxes, vertical
layout and split-style cluster fragments are unsupported. Raster requests above
4096px/device dimension return errors. Empty outlines return empty output;
provided raster bounds must match measured bounds. Bounds and raster calls each
walk outlines on misses; tiny-skia coverage is copied to the returned buffer.

Explicit paragraph direction/language, alignment options, advanced styling and
full Unicode conformance remain follow-ups. Deterministic shaping requires pinned
fonts, variation/optical-size policy, fallback, features, language, Unicode data
and shaper versions. Host discovery and raster pixels can still differ.

## Checks

Tests use [licensed bundled fixtures](tests/fonts/README.md), without host font
discovery. Native construction smoke runs on the main thread, without windows:

```sh
cargo test --locked --offline --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-parley/Cargo.toml --target-dir target
# Add --features native-test --test construction for host construction/raster smoke.
```
