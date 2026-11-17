# xgpui-curve

Curve and contour math for **xgpui**, also usable on its own: segment
geometry, arc length and path effects, plot series curves, functional
(y = f(x)) curves and CSS easing, mesh-gradient lattices, curved field spines
with matching WGSL and Metal shader code, an editable node path, squircles,
variable-width strokes, freehand fitting, SVG path data, and GPUI path
adapters.

All math is `f64` and built on [kurbo](https://docs.rs/kurbo), which is
re-exported as `xgpui_curve::kurbo`. kurbo types (`Point`, `Vec2`, `BezPath`,
`CubicBez`, …) appear in this crate's API, so a kurbo major version change is
a breaking change here.

## Dependency

```toml
[dependencies]
xgpui-curve = "0.1.0"
```

Every module except `geom` has a feature of the same name (`svg-path` for
`svg_path`), all on by default. Turn off `gpui` to use the math without GPUI.

| Module | Contents |
| --- | --- |
| `geom` | Evaluation, derivatives, curvature, Hermite ↔ Bézier, tight bounds, nearest point, split, intersection, rectangle clipping, flattening, fill and stroke hit testing |
| `measure` | Closed-form quadratic arc length, `MeasuredPath` (arc-length lookup), trim and dash |
| `series` | d3-style plot curves: linear, step, basis, cardinal, Catmull-Rom, natural, monotone (PCHIP), bundle; area fills |
| `func` | `FunctionalCurve` (PCHIP, linear, x-clamped Bézier), CSS easing (`cubic-bezier`, `steps`, `linear()`), LUT baking |
| `mesh` | `MeshLattice`: Hermite/Coons lattices with automatic or explicit tangents, fold clamping, cursor-exact edge dragging |
| `spine` | `CurveSpine`: quadratic field spines, (s, d⊥) projection, arrowheads, the inner-bend seam rule, `CURVE_SPINE_WGSL` / `CURVE_SPINE_MSL` |
| `node` | `NodePath`: Corner, Smooth, Symmetric and automatic (Catmull-Rom, natural, Hobby) nodes with constraint-preserving edits |
| `shape` | `Squircle` (continuous-curvature corners), `ArcShape` (annular sectors) |
| `stroke` | `StrokeProfile`: variable-width and tapered outlines with caps and joins |
| `fit` | Ramer–Douglas–Peucker and Schneider cubic fitting for pointer and pen input |
| `svg_path` | SVG `d` parsing and serialization |
| `gpui_paint` | `gpui::Path<Pixels>` builders and a two-pass contrast stroke |

## Plot a series

```rust
use xgpui_curve::kurbo::Point;
use xgpui_curve::series::{SeriesCurve, series_path};

let samples = [Point::new(0., 1.), Point::new(1., 3.), Point::new(2., 2.), Point::new(3., 5.)];
let path = series_path(&samples, SeriesCurve::MonotoneX);
assert_eq!(path.segments().count(), 3);
```

## Bend a field along a spine

```rust
use xgpui_curve::kurbo::Point;
use xgpui_curve::spine::CurveSpine;

let mut spine = CurveSpine::with_bend(Point::new(0.2, 0.5), Point::new(0.8, 0.5), 0.3);
// Keep the projection continuous within the kernel's reach.
spine.limit_bend(0.15);
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
use xgpui_curve::kurbo::Point;

let curve = FunctionalCurve::monotone(&[Point::new(0., 0.), Point::new(0.5, 0.65), Point::new(1., 1.)])?;
let lut = curve.to_lut_f32::<256>();
assert_eq!(lut[0], 0.);
assert!((Easing::EASE.eval(0.5) - 0.8024).abs() < 1e-4);
# Ok::<(), xgpui_curve::CurveError>(())
```

## Compatibility

Enums that may gain variants (curve kinds, caps, joins, easings, node modes,
errors) and the structs that report results (`Location`, `NearestPoint`,
`Intersection`, `PathHit`, `EdgeHit`, `SpinePoint`) are `#[non_exhaustive]`,
so new variants and fields can be added in minor releases. Match them with a
wildcard arm.

## GPUI public API changes: additions and compatibility

None. The optional `gpui` feature uses the existing `PathBuilder` and
`Window::paint_path`; everything else is plain Rust on kurbo types. See
[curve integration compatibility](https://xgpui.dev/gpui-public-api.html#curve-reuses-existing-gpui-apis).

## License

Apache-2.0.
