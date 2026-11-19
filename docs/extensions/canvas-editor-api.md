# Canvas editor API contract (#384)

This is the implementation contract for the mixed-shape editor epic, not a
claim that the whole editor API already exists. Camera navigation is the first
implementation slice. Scene editing and optional point/rectangle adapters remain planned work.
All additions belong to the extension; no core GPUI API changes are planned.

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

## Planned scene mutation and invalidation

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

These policies must be tested when their implementation lands. They do not
promise automatic spatial-bound tracking or a dynamic BVH.

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
consumers can construct `kurbo::Affine::new(coefficients)` directly. Absolute
matrices can lose local precision at large translated centers even when every
coefficient is finite. Use direct camera point projection for those views;
affine conversion does not provide arbitrary-precision coordinates.

Optional point/rectangle conversions remain follow-on work. The shared curve-view
camera now delegates pan/zoom to Camera2D and constructs kurbo Affine from its
coefficients. Its wrapper preserves panel-origin composition and resize behavior,
and rejects updates whose absolute affine coefficients cannot be represented.
Existing anchored zoom, limit, moved-panel and reset checks exercise this consumer.

## Validation and scope

Camera tests cover asymmetric bounds, padding, points/lines, zoom limits, inverse
bounds projection, invalid input, extreme finite/subnormal bounds, affine
projection agreement and coefficient overflow. Lab navigation tests cover fitting
both 10k/100k scenes and scrolling outward without a minimum-zoom jump.

Scene/editor acceptance will add fine picking, appearance invalidation, batch
updates, rebuild equivalence and real-window move/snap/cancel checks. Windows and
Linux rely on integration CI until main merge; do not request branch testing from
the owner. No release/version change is part of this slice.
