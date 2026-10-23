# Extension paint contract — first Metal slice

Checkpoint: 9504324. Effects belong in xgpui-shader; GPUI owns coverage,
ordering, inherited clip/opacity, resource submission and presentation.

See [paint contract hardening](paint-contract-hardening.md) for the corrected
border semantics and the unimplemented preparation/color/coordinate proposal.

## API

Add CPU-side Paint::{Native(Background), Custom(Arc<dyn CustomContent>)}.
Keep Background, Hsla, native Quad/Path/Shadow GPU layouts and current methods.
Add Styled::bg_paint / border_paint and Window::paint_path_with_paint /
paint_quad_with_paints. Existing `.bg(...)` also accepts Paint through Fill.
A custom paint is runtime-only: serde rejects it instead of inventing a registry;
native Paint values remain serializable. Style gains an optional border override;
this can break exhaustive downstream Style/StyleRefinement literals. New Paint
and Fill variants and the CustomDraw coverage field are public API additions.

## Coordinates and coverage

Producer texture spans the unclipped outer shape bounds, never the clipped
bounds. Borders sample one continuous field over the full outer rectangle, not
one field per edge. Path fills use the path's original geometric bounds.
Rounded rectangles use existing custom analytic coverage. Borders and paths
render white native geometry to a transparent offscreen mask using GPUI's current
pipelines, including dashed border coverage and 4x path MSAA. The compositor
uses the white mask's red channel as coverage and multiplies source alpha by
coverage and inherited opacity once. Reading white RGB preserves the existing
native border shader's visible antialiasing, including its inner-edge blend. This is independent content, not a filter of existing children.

## Lifetime, preparation and caching

Immutable producers remain Arc-owned by retained scene records; replay retains
coverage geometry too. Only GPUI submits. Source caches remain producer-owned,
queue/extent scoped. Metal caches coverage independently by exact normalized
geometry and physical extent (not clipping, producer, color, opacity or screen
origin). A 64 MiB coverage cache clears on overflow; each mask is also capped at
64 MiB. The cache budget includes geometry keys, but excludes in-flight GPU
resources and producer-owned output textures. Native masks contain no custom producers, so mask rendering cannot
recurse. Mask work is committed by the host before sampling and may harmlessly
complete even when a later paint fails. Native rendering retains its fast path.

## Scope and validation

First milestone: rounded background, dashed/asymmetric border, arbitrary path
fill, all using the same immutable producer. paint-lab compares native geometry
and custom paint, toggles inherited opacity and clipping, and animates one shared
mesh snapshot. GPU tests cover coverage parity, clipping without remapping,
producer alpha/opacity, path fill/stroke, replay, resize and source sharing.

Shadows and glyph/decorative paints are follow-up integration sites, not shipped
by this slice. Shadow mapping will use element bounds with clamp-to-edge sampling
through its blur region; glyphs need an explicit run/element coordinate policy.
Explicit cross-shape paint rectangles, coverage pooling/LRU, direct fragment
producer optimization and portable backend adapters remain later work. Windows
and wgpu continue to report unsupported custom content. No HDR/gamut changes.

## Native border adapter

A non-solid Native paint used as a border is rendered through a private native
background snapshot and the same coverage composition. The reserved host result
is Background, distinct from backend texture outputs. This adapter unpremultiplies
its native intermediate before the straight-alpha compositor; it currently
regenerates that native source when repainted. Existing solid border_color calls
retain the native direct path. This is an adapter for existing paints, not an
effect implementation in core GPUI.
