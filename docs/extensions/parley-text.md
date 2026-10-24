# xgpui-parley

Opt-in Parley 0.12/HarfRust text backend: flat-LTR compatibility, BiDi paragraphs,
resolved-face metrics and grayscale rasterization. GPUI retains platform input,
frame caches and glyph atlas ownership.

**[GPUI public API changes: additions and compatibility](gpui-public-api.md#added-text-backend-and-paragraph-seams).**

## Construction

```toml
# Application under apps/:
[dependencies]
xgpui-parley = { path = "../../extensions/xgpui-ext/crates/xgpui-parley", features = ["system-fonts"] }
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
Native platform initialization is unchanged. Windows/Linux use the same adapter;
runtime validation remains outstanding.

## Paragraphs and input

```rust
// FontRuns contain descriptor IDs resolved by this application's TextSystem.
let paragraph = window.text_system().layout_paragraph(
    text, gpui::px(24.0), &font_runs, width,
)?;
let hit = paragraph.hit_test(mouse_position - bounds.origin)?;
let caret = paragraph.caret_bounds(hit)?;
let fragments = paragraph.selection_bounds(anchor, hit)?;

// During element paint:
window.handle_input(&focus, gpui::ElementInputHandler::new(bounds, entity), cx);
gpui::paint_paragraph(paragraph.as_ref(), bounds.origin, color_at, window)?;
```

The input entity implements `EntityInputHandler`. It translates UTF-16 edits to
source UTF-8 and uses the painted paragraph for IME geometry. Candidate anchors
use the requested caret/fragment; marked text uses selection fragments. Edits
invalidate geometry until repaint. Apply origin/scrolling once at the UI edge.

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
`ParagraphShape` and `ParagraphLayout` belong to **xgpui-parley**, not GPUI.
Parley/HarfRust/Skrifa types do not enter the core signatures.

The text additions change no existing public struct fields or required trait
methods. Existing backend implementations need no paragraph override;
`Application::with_platform` and `PlatformTextSystem::layout_line` retain their
contracts. `EntityInputHandler`, `ElementInputHandler` and glyph/emoji paint
methods are existing GPUI APIs. Internal changes add paragraph frame-cache
entries and invalidation; the macOS variable-weight correction changes native
font selection behavior without adding public API.

## Contracts

| Boundary | Contract |
| --- | --- |
| Font identity | Requested descriptors distinguish features/fallbacks; shaped IDs retain actual blob, face index and normalized variation location |
| Lifetime | IDs are not recycled; old layouts remain rasterizable after font registration |
| Shape cache | GPUI key: source, ordered FontRuns, size |
| Reflow cache | Same key plus width; immutable layout Arcs; width changes reuse shaping |
| Invalidation | Both cache levels follow frame retention/reuse and font generation; backend calls run outside cache locks |
| Geometry | Logical UTF-8 plus affinity; visual glyph order; paragraph-local positions include baseline/glyph offsets |
| Paint | Existing bounds cache and sprite atlas; colors resolve by logical index; decorations/backgrounds remain caller-owned |
| Native providers | Optional paragraph hook defaults to Unsupported |

Parley owns shaping, BiDi, breaking and interaction geometry. Reflow clones and
rebreaks retained Parley layout. Skrifa metrics/rasterization use the same
registered instance; a bounded LRU retains hinting setup. No extension layout or
bitmap cache is added. [Core API additions](gpui-public-api.md).

## Supported subset

Flat-line compatibility requires LTR, one size, scale 1 and start alignment.
`try_layout_line` returns errors; the infallible hook substitutes `.notdef` with
original indices and reports rejection outside the backend lock.

Paragraphs support mixed LTR/RTL, automatic direction, start alignment, one size,
multiple font runs and U+2028 hard breaks. Callers split document paragraphs;
the lab normalizes pasted line endings to U+2028.

Synthetic styles, color/bitmap emoji, LCD, dilation, inline boxes, vertical text
and split-style cluster fragments are unsupported. Raster size/bounds are capped
at 4096 device pixels. Explicit direction/language/alignment, advanced styling,
accessibility and broader BiDi/UAX #14 conformance remain follow-ups.

## Lab and validation

```sh
# Repository root; paragraph mode uses Parley, compare uses both backends.
just --justfile apps/text-lab/justfile ui
just --justfile apps/text-lab/justfile paragraph
just --justfile apps/text-lab/justfile paragraph --timings-only
just --justfile apps/text-lab/justfile scroll --scroll-frames 640 --exit-after-scroll
just --justfile apps/text-lab/justfile test
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
Paragraph conversion IMEs and Windows/Linux runtime/IME remain unvalidated.
