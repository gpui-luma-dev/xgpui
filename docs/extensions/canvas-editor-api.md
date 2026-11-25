# Canvas editor API contract (#384)

This is the implementation contract for the mixed-shape editor epic, not a
claim of a finished general-purpose editing framework. Camera navigation, scene
editing and optional coordinate adapters are implemented on the #384 subtask
branches, with a native mixed-shape editor exercising them.
The new APIs belong to the extension. The editor also requires a core GPUI
pinch-dispatch correction, documented separately for individual behavior review.

## Compatibility and identity

Keep existing query, visibility, hit-test and bounds-update signatures working.
Add indexed variants rather than changing the item type yielded by existing
iterators. An index is a position in `scene.items()` and the scene's back-to-front
paint order. It remains valid during bounds/data updates and index rebuilding.
It is not a persistent object ID, and must not be transferred to a replacement
scene. Pair caches with scene identity. Dynamic insertion, deletion, reordering
and generational handles remain outside this implementation slice.

Front-to-back hit candidates must reverse paint order and include every AABB
candidate. Consumers own fine fill/stroke tests, visibility and locked-item
policy. Rejecting a candidate must allow inspection of the next candidate.
Picking never implies that an entire AABB is a painted shape.

## Scene mutation and invalidation

- Preserve `revision()` and `cache_key()` as geometry-only invalidation contracts.
- Add a distinct content revision/key incorporating scene identity, geometry
  revision and content revision for appearance-dependent captures.
- Geometry-safe mutable access exposes `T`, never mutable item bounds. Granting
  mutable access conservatively advances the content revision, even if a caller
  writes the same value. Invalid indices leave all revisions unchanged.
- Data that changes geometric extent must also update bounds; data access does
  not infer geometry. The initial editor uses this access for appearance changes.
- Batch bounds updates validate all indices and bounds, including duplicates,
  before changing anything. Invalid or duplicate indices reject the whole batch.
  A nonempty accepted batch advances geometry revision once; an empty batch is a
  successful no-op. Refit unique affected leaves and ancestors, then compare
  behavior and cost with individual updates for sparse and large selections.
- An explicit index rebuild preserves paint positions, identity, data and bounds.
  Because it changes only spatial partitioning, it does not invalidate geometry
  or content captures. An internal index generation can be added if needed.

These policies are tested against linear query references, revision keys and
atomic failure cases. They do not promise automatic spatial-bound tracking or a
dynamic BVH.

## Implemented camera navigation

`center()` reads the world point at the viewport center. Bounds projection
helpers apply the existing point projections to both corners without validation
or clamping; callers retain the finite/representable-input responsibility.

`fit_bounds(bounds, viewport, padding)` uses default limits 0.01..=64.
`fit_bounds_with_limits` accepts explicit finite positive limits. Padding is
uniform logical pixels on all four sides. The viewport must be finite and
positive, and padding must leave positive space on both axes. Bounds must be
finite and ordered. A point target centers at maximum zoom; a line target is
constrained by its nonzero dimension. A maximum zoom may leave extra space.
A minimum zoom that would crop the target causes failure, rather than a false
claim of fitting. Invalid or unrepresentable fits return false and leave the
entire camera unchanged. Extremely small spans can round away at finite precision.

The native and browser card Overview actions now fit actual scene bounds with
24 logical pixels of padding and explicit limits 1e-6..=64. The smaller minimum
allows the tall 100k-card scene to fit; subsequent input uses the same limits.
Mandelbrot limits are unchanged. Fit Selection will use the same primitive in
the editor rather than a separate camera implementation.

`world_to_screen_affine` and `screen_to_world_affine` return checked optional
`[a, b, c, d, e, f]` coefficients, where
`x' = a*x + c*y + e` and `y' = b*x + d*y + f`.
They return None for non-finite viewport/coefficients. No dependencies are added;
consumers can construct `xgpui_curve::geom::Affine::new(coefficients)` directly. Absolute
matrices can lose local precision at large translated centers even when every
coefficient is finite. Use direct camera point projection for those views;
affine conversion does not provide arbitrary-precision coordinates.

The mixed-shape editor copies canvas point and bounds coordinates into curve
geometry for path bounds and precise picking. The existing curve camera now
reuses Camera2D while preserving its panel-fit composition and resize behavior.

## Editor consumer

Run `just canvas editor` (or `just canvas --editor`). Rectangles, a holed fill,
a concave fill and an open cubic stroke overlap in one retained scene. The BVH
provides front-to-back AABB candidates; fill/stroke tests decide which painted
object receives input. Hidden and locked objects are skipped by application policy.

The lab supports Shift-click group selection, snapped/free dragging, Escape
restoration, fit scene/selection, color edits, hide/lock toggles and index rebuild.
The retained bounds overview uses `content_cache_key`, so geometry and appearance
changes both refresh it. It deliberately previews colored bounds, not full shape
thumbnails. Shape geometry stays in the lab; Canvas does not depend on xgpui-curve.
The default card and browser views remain available.

`just canvas editor-smoke` is registered in labs.toml. It dispatches real GPUI
window input and saves startup/selection/edited captures plus a report under
`target/canvas-editor-smoke`. Focus observers are asynchronous, so its focus-loss
assertions run after queued effects rather than immediately inside blur().

The editor also exposed pinch suppression after keyboard input. That isolated
core behavior fix and its registered window probe are in PR #402; individual
compatibility review is recorded in `docs/api-reviews/384-pinch-input.md`.

## Validation and scope

Camera tests cover asymmetric bounds, padding, points/lines, zoom limits, inverse
bounds projection, invalid input, extreme finite/subnormal bounds, affine
projection agreement and coefficient overflow. Lab navigation tests cover fitting
both 10k/100k scenes and scrolling outward without a minimum-zoom jump.

Scene/editor checks cover fine picking, appearance invalidation, batch updates,
rebuild equivalence and real-window move/snap/cancel/focus/zoom interactions. Windows and
Linux rely on integration CI until main merge; do not request branch testing from
the owner. No release/version change is part of this slice.
