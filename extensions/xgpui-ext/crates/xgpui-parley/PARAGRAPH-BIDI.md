# Paragraph/BiDi contracts

Implemented: optional core hook, immutable shape/reflow results, visual glyph
painting, GPUI caches, and a paragraph editing/IME lab. Current scope is automatic
base direction, start alignment, one size, multiple font runs and U+2028 breaks.

## Interfaces

Actual GPUI surfaces; signatures abbreviated:

```rust,ignore
PlatformTextSystem::shape_paragraph(ParagraphRequest { text, font_size, runs })
    -> Result<Arc<dyn PlatformParagraphShape>>;
PlatformParagraphShape::layout(width)
    -> Result<Arc<dyn PlatformParagraphLayout>>;

// PlatformParagraphLayout:
source(); font_size(); size(); runs();
hit_test(point); caret_bounds(position);
move_visual(position, direction); selection_bounds(anchor, focus);

struct ParagraphPosition { byte_index: usize, affinity: TextAffinity }
```

Native text backends default to Unsupported. Finite sizes/widths and covering
UTF-8 font runs are validated. Published results are immutable. The adapter
retains Parley `Layout<()>`; reflow clones it, breaks lines and aligns. Interaction
queries use Parley Cursor/Selection on that exact layout. Core contains no Parley
types or independent BiDi/line-breaking algorithm.

## Identity, coordinates and paint

Glyph runs are in visual order; indices remain logical source bytes. Positions
include run/glyph offsets and line baseline, relative to paragraph top-left.
Resolved font IDs retain blob, face index and variation identity across reflow
and font loading. A glyph start index alone cannot reconstruct caret geometry.

`paint_paragraph` uses `Window::paint_glyph`/`paint_emoji` and the existing atlas.
Colors resolve by logical index. Selection/decorations use geometry fragments;
forward-only flat-line style traversal cannot handle RTL. Apply element origin
and scrolling once at the UI boundary.

## Cache contract

| GPUI frame-cache entry | Key |
| --- | --- |
| Shape | Source, ordered `FontRun`s, size |
| Reflow | Shape parameters plus width |

Both levels participate in current/previous-frame reuse/truncation and font
invalidation. Exact repeats reuse layout Arcs; width changes reuse shaping.
Backend calls execute outside cache locks. Generation changes prevent stale
publication; held layouts/face IDs remain valid. Colors, selection and device
scale do not enter shaping keys. Future direction/language/alignment/metric
options must enter the appropriate key before exposure.

## Input contract

GPUI retains native input ownership through `ElementInputHandler`. The lab
translates UTF-16/source UTF-8, including candidate-relative selection offsets.
Caret/hit tests and marked/selection fragments use the painted text revision.
Single-rectangle IME queries return the requested caret/relevant fragment,
not a union across BiDi fragments. Stale geometry is unavailable until repaint.

## Remaining work

Explicit direction/language/alignment; broader BiDi/UAX #14 conformance; paragraph
conversion IMEs; Windows/Linux runtime and accessibility validation. Inline
widgets, vertical text, ellipsis, justification, hyphenation and color emoji
remain outside scope. [Validation and timing definitions](../../../../apps/text-lab/README.md).
