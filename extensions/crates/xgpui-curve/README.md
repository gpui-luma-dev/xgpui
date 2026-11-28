# xgpui-curve

Curve and contour math for **xgpui**, also usable on its own: segment
geometry, arc length and path effects, plot series curves, functional
(y = f(x)) curves and CSS easing, mesh-gradient lattices, curved field spines
with matching WGSL and Metal shader code, an editable node path, squircles,
variable-width strokes, freehand fitting, SVG path data, and GPUI path
adapters.

Geometry uses xgpui-owned `f64` types in `xgpui_curve::geom`.
Kurbo provides numerical algorithms internally.

## Migrating from the kurbo API

- Import `Point`, `Vec2`, `Rect`, `Affine`, `BezPath`, and segment types from
  `xgpui_curve::geom`. Curve evaluation and path bounds are inherent methods.
  Use `geom::circle_path` and `geom::rounded_rect_path` to construct shape paths.
- Replace `kurbo_stroke` and `kurbo::Stroke` with `stroke::stroke_outline`
  and `stroke::StrokeStyle`. Set `Join::Miter(limit)` for a miter join.
- Canvas callers should remove the `kurbo` feature and convert canvas points
  and bounds by copying their coordinates at the call site.
- This changes Rust source compatibility. Use `svg_path::parse` and
  `svg_path::to_svg` for SVG `d` strings. They use the same codec and
  preserve the existing absolute-command output and arc-to-cubic conversion.
  The owned types define no Rust serialization format.

## Dependency

```toml
[dependencies]
xgpui-curve = "0.3.0"
```

Every module except `geom` has a feature of the same name (`svg-path` for
`svg_path`), all on by default. Turn off `gpui` to use the math without GPUI.

| Module | Contents |
| --- | --- |
| `geom` | Evaluation, derivatives, curvature, Hermite ↔ Bézier, tight bounds, nearest point, split, intersection, rectangle clipping, flattening, fill and stroke hit testing |
| `measure` | Closed-form quadratic arc length, `MeasuredPath` (arc-length lookup, differential geometry, contour-aware frame sampling), trim and dash |
| `series` | d3-style plot curves: linear, step, basis, cardinal, Catmull-Rom, natural, monotone (PCHIP), bundle; area fills |
| `func` | `FunctionalCurve` (PCHIP, linear, x-clamped Bézier), CSS easing (`cubic-bezier`, `steps`, `linear()`), LUT baking |
| `mesh` | `MeshLattice`: Hermite lattices with automatic or explicit tangents, fold clamping, cursor-exact edge dragging |
| `spine` | `CurveSpine`: quadratic field spines, (s, d⊥) projection, arrowheads, the inner-bend seam rule, `CURVE_SPINE_WGSL` / `CURVE_SPINE_MSL` |
| `node` | `NodePath`: Corner, Smooth, Symmetric and automatic (Catmull-Rom, natural, Hobby) nodes with constraint-preserving edits |
| `shape` | `Squircle` (continuous-curvature corners), `ArcShape` (annular sectors) |
| `stroke` | Styled constant-width outlines, variable-width profiles, and `StrokeFootprint` for shared bounds and picking |
| `fit` | Ramer–Douglas–Peucker, Schneider cubic fitting, and parameter-preserving fits with sample-error diagnostics |
| `svg_path` | SVG `d` parsing and serialization |
| `gpui_paint` | `gpui::Path<Pixels>` builders, fallible footprint tessellation, and a two-pass contrast stroke |

## Plot a series

```rust
use xgpui_curve::geom::Point;
use xgpui_curve::series::{SeriesCurve, series_path};

let samples = [Point::new(0., 1.), Point::new(1., 3.), Point::new(2., 2.), Point::new(3., 5.)];
let path = series_path(&samples, SeriesCurve::MonotoneX);
assert_eq!(path.segments().count(), 3);
```

## Bend a field along a spine

```rust
use xgpui_curve::geom::Point;
use xgpui_curve::spine::CurveSpine;

let mut spine = CurveSpine::with_bend(Point::new(0.2, 0.5), Point::new(0.8, 0.5), 0.3);
// Straighten until the projection is continuous out to 0.6 (this bend's reach
// is 0.5).
spine.limit_bend(0.6);
assert!(spine.reach() >= 0.6 - 1e-9);
let local = spine.project(Point::new(0.5, 0.4));
assert!((0. ..=1.).contains(&local.s));
```

The GPU code computes the same projection: include `CURVE_SPINE_WGSL` (or
`CURVE_SPINE_MSL` after `#include <metal_stdlib>`) and call
`curve_spine_project(p, p0, c, p1, t_min, t_max)`, which returns
`(signed distance, t, s, length)`. Pack the arguments with
`CurveSpine::shader_params`.

## Tone curves and easing

```rust
use xgpui_curve::func::{Easing, FunctionalCurve};
use xgpui_curve::geom::Point;

let curve = FunctionalCurve::monotone(&[Point::new(0., 0.), Point::new(0.5, 0.65), Point::new(1., 1.)])?;
let lut = curve.to_lut_f32::<256>();
assert_eq!(lut[0], 0.);
assert!((Easing::EASE.eval(0.5) - 0.8024).abs() < 1e-4);
# Ok::<(), xgpui_curve::CurveError>(())
```

## Geometry at a distance

`MeasuredPath::geometry_at(s)` returns a `PathGeometry` with a `location`
(point, unit tangent, contour/segment index and local parameter), unit normal,
and signed curvature. It performs one arc-length lookup and uses the selected
segment directly, so callers do not need a parallel segment list.

```rust
use xgpui_curve::{geom::{BezPath, Point}, measure::MeasuredPath};
let mut path = BezPath::new();
path.move_to((0., 0.));
path.line_to((10., 0.));
let measured = MeasuredPath::new(&path, 1e-8)?;
let geometry = measured.geometry_at(5.).unwrap();
assert!(geometry.location.point.distance(Point::new(5., 0.)) < 1e-8);
assert_eq!(geometry.curvature, Some(0.));
# Ok::<(), xgpui_curve::CurveError>(())
```

Geometry is in path coordinates: curvature has inverse coordinate units, and
positive curvature bends toward the +90° normal. Screen transforms are not
implicitly applied. A stationary point can have a one-sided tangent and normal
while curvature is `None`; derivatives of magnitude <= 1e-12 and non-finite
curvature are treated as undefined. A regular straight line has `Some(0.)`.

Finite distances clamp to the path length. Non-finite distances, empty paths,
and non-finite evaluated positions return `None`. Joins use `locate`'s existing
segment/contour selection, without averaging derivatives. Closed paths clamp
to their final segment rather than wrapping around the seam.

This adds a method and a non-exhaustive result struct under `measure`.
Existing `Location` fields and `locate`/`point_at` behavior are unchanged; no
migration, serialization changes, or core GPUI changes are required.

## Parameter-preserving fits (prototype)

Use `fit::fit_parameterized` when samples carry a source parameter such as
simulation time. Unlike `fit_cubic_path`, it does not replace that parameter
with chord length or Newton reparameterization. Each output cubic carries a
source interval; evaluation within it uses the linearly normalized source
parameter. Loops and repeated positions retain their source ordering.

```rust
use xgpui_curve::{fit::{fit_parameterized, ParameterizedFitOptions}, geom::Point};

let source = [0., 0.1, 0.4, 1.];
let points = [Point::new(0., 0.), Point::new(0.1, 0.2),
              Point::new(0.4, 0.3), Point::new(1., 1.)];
let mut options = ParameterizedFitOptions::default();
options.tolerance = 0.001;
options.max_segments = 128;
let fit = fit_parameterized(&points, &source, options)?;
let quality = fit.diagnostics();
assert!(quality.tolerance_met);
let (segment, u) = fit.segment_at(0.4).unwrap();
assert!((fit.parameter_at(segment, u).unwrap() - 0.4).abs() < 1e-12);
let point = fit.point_at(0.4).unwrap();
assert!(point.distance(points[2]) <= options.tolerance);
# Ok::<(), xgpui_curve::CurveError>(())
```

Parameters must be finite and strictly increasing; positions may repeat. At
least two samples are required. Finite queries clamp to the source domain;
non-finite queries return `None`. The fitter shares estimated tangents at
splits but does not promise continuous curvature. It does not simplify samples.

Diagnostics report the maximum and RMS distance **at the supplied source
parameters**, input and segment counts, whether tolerance was met, and whether
the segment budget prevented a needed split. They include every input sample,
not a nearest point that might lie on another loop. A budget-limited result is
returned for inspection; callers must check `tolerance_met`. These errors do
not certify the unknown function between samples or bound derivative error.

The prototype is additive under the existing `fit` feature and works without
GPUI or `measure`. Existing `fit_cubic_path` behavior, `FitOptions`, serialized
data, and dependencies are unchanged. No migration is required. These helpers
belong in the curve extension; no core GPUI API additions are needed. The zeta
lab uses source intervals with `MeasuredPath::locate` and segment arc lengths
to replace its approximate source-to-length lookup table.

## Compatibility

Enums that may gain variants (curve kinds, caps, joins, easings, node modes,
errors) and the structs that report results (`Location`, `NearestPoint`,
`Intersection`, `PathHit`, `EdgeHit`, `SpinePoint`) are `#[non_exhaustive]`,
so new variants and fields can be added in minor releases. Match them with a
wildcard arm.

## GPUI public API changes: additions and compatibility

None. The optional `gpui` feature uses the existing `PathBuilder` and
`Window::paint_path`; everything else is plain Rust on owned geometry types. See
[curve integration compatibility](https://xgpui.dev/gpui-public-api.html#curve-reuses-existing-gpui-apis).

## License

Apache-2.0.
