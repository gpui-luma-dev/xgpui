use super::*;
use crate::geom::{self, PathEl, PathSeg};

fn pts(xy: &[(f64, f64)]) -> Vec<Point> {
    xy.iter().map(|&(x, y)| Point::new(x, y)).collect()
}

fn sample() -> Vec<Point> {
    pts(&[(0., 1.), (1., 3.), (2., 2.), (4., 5.), (5., 4.5)])
}

fn ends(path: &BezPath) -> Vec<Point> {
    path.elements()
        .iter()
        .filter_map(|el| match el {
            PathEl::MoveTo(p)
            | PathEl::LineTo(p)
            | PathEl::QuadTo(_, p)
            | PathEl::CurveTo(_, _, p) => Some(*p),
            PathEl::ClosePath => None,
        })
        .collect()
}

#[test]
fn step_variants_match_d3_vertices() {
    let p = pts(&[(0., 0.), (2., 1.), (4., 0.)]);
    let step = ends(&series_path(&p, SeriesCurve::Step));
    assert_eq!(
        step,
        pts(&[(0., 0.), (1., 0.), (1., 1.), (3., 1.), (3., 0.), (4., 0.)])
    );
    let before = ends(&series_path(&p, SeriesCurve::StepBefore));
    assert_eq!(
        before,
        pts(&[(0., 0.), (0., 1.), (2., 1.), (2., 0.), (4., 0.)])
    );
    let after = ends(&series_path(&p, SeriesCurve::StepAfter));
    assert_eq!(
        after,
        pts(&[(0., 0.), (2., 0.), (2., 1.), (4., 1.), (4., 0.)])
    );
}

#[test]
fn interpolating_curves_pass_through_every_sample() {
    for curve in [
        SeriesCurve::Linear,
        SeriesCurve::Cardinal { tension: 0.3 },
        SeriesCurve::CatmullRom { alpha: 0.5 },
        SeriesCurve::CatmullRom { alpha: 1. },
        SeriesCurve::Natural,
        SeriesCurve::MonotoneX,
    ] {
        let path = series_path(&sample(), curve);
        assert_eq!(ends(&path), sample(), "{curve:?}");
    }
}

#[test]
fn natural_spline_is_c2_at_interior_knots() {
    let segs: Vec<PathSeg> = series_path(&sample(), SeriesCurve::Natural)
        .segments()
        .collect();
    for w in segs.windows(2) {
        assert!((geom::deriv1(w[0], 1.) - geom::deriv1(w[1], 0.)).hypot() < 1e-9);
        assert!((geom::deriv2(w[0], 1.) - geom::deriv2(w[1], 0.)).hypot() < 1e-9);
    }
    // Natural end condition: zero second derivative at both ends.
    assert!(geom::deriv2(segs[0], 0.).hypot() < 1e-9);
    assert!(geom::deriv2(*segs.last().unwrap(), 1.).hypot() < 1e-9);
}

#[test]
fn cardinal_and_catmull_rom_relations() {
    let straight = series_path(&sample(), SeriesCurve::Cardinal { tension: 1. });
    for seg in straight.segments() {
        let PathSeg::Cubic(c) = seg else { panic!() };
        assert_eq!((c.p1, c.p2), (c.p0, c.p3));
    }
    assert_eq!(
        series_path(&sample(), SeriesCurve::CatmullRom { alpha: 0. }),
        series_path(&sample(), SeriesCurve::Cardinal { tension: 0. })
    );
    // Uniform Catmull-Rom interior tangent is (p[i+1] − p[i−1]) / 2.
    let segs: Vec<PathSeg> = series_path(&sample(), SeriesCurve::Cardinal { tension: 0. })
        .segments()
        .collect();
    let s = sample();
    assert!((geom::deriv1(segs[1], 0.) - (s[2] - s[0]) / 2.).hypot() < 1e-12);
}

#[test]
fn basis_and_bundle_shapes() {
    let path = series_path(&sample(), SeriesCurve::Basis);
    let e = ends(&path);
    assert_eq!(e.first(), sample().first());
    assert_eq!(e.last(), sample().last());
    let flat = series_path(&sample(), SeriesCurve::Bundle { beta: 0. });
    let (a, b) = (sample()[0], sample()[4]);
    for p in geom::flatten_polylines(&flat, 1e-6).concat() {
        assert!((p - a).cross(b - a).abs() < 1e-9);
    }
    assert_eq!(
        series_path(&sample(), SeriesCurve::Bundle { beta: 1. }),
        path
    );
}

#[test]
fn monotone_x_never_overshoots_and_falls_back_on_reversal() {
    let data = pts(&[
        (0., 0.),
        (1., 0.),
        (1.5, 1.),
        (3., 1.),
        (3.2, 0.2),
        (5., 0.),
    ]);
    for seg in series_path(&data, SeriesCurve::MonotoneX).segments() {
        let (lo, hi) = (
            seg.start().y.min(seg.end().y),
            seg.start().y.max(seg.end().y),
        );
        for i in 0..=50 {
            let y = seg.eval(i as f64 / 50.).y;
            assert!(y >= lo - 1e-12 && y <= hi + 1e-12);
        }
    }
    let reversing = pts(&[(0., 0.), (2., 1.), (1., 2.)]);
    let path = series_path(&reversing, SeriesCurve::MonotoneX);
    assert!(path.segments().all(|s| matches!(s, PathSeg::Line(_))));
    let vertical = pts(&[(0., 0.), (1., 1.), (0.5, 2.), (2., 3.)]);
    assert_eq!(
        ends(&series_path(&vertical, SeriesCurve::MonotoneY)),
        vertical
    );
}

#[test]
fn non_finite_samples_break_the_line() {
    let mut data = sample();
    data.insert(2, Point::new(f64::NAN, 1.));
    let path = series_path(&data, SeriesCurve::CatmullRom { alpha: 0.5 });
    assert_eq!(
        path.elements()
            .iter()
            .filter(|e| matches!(e, PathEl::MoveTo(_)))
            .count(),
        2
    );
    assert!(series_path(&[], SeriesCurve::Linear).elements().is_empty());
}

#[test]
fn areas_are_closed_per_run() {
    let mut data = sample();
    data.push(Point::new(f64::INFINITY, 0.));
    data.extend(pts(&[(7., 1.), (8., 2.)]));
    let area = series_area(&data, SeriesCurve::Natural, Baseline::Constant(0.));
    let closes = area
        .elements()
        .iter()
        .filter(|e| matches!(e, PathEl::ClosePath))
        .count();
    assert_eq!(closes, 2);
    assert!(geom::fill_contains(
        &area,
        Point::new(1., 0.5),
        geom::FillRule::NonZero
    ));
    assert!(!geom::fill_contains(
        &area,
        Point::new(1., -0.5),
        geom::FillRule::NonZero
    ));
    let lower: Vec<Point> = sample().iter().map(|p| Point::new(p.x, p.y - 1.)).collect();
    let band = series_area(&sample(), SeriesCurve::Linear, Baseline::Series(&lower));
    assert!(geom::fill_contains(
        &band,
        Point::new(0.5, 1.5),
        geom::FillRule::NonZero
    ));
}

#[test]
fn clipped_series_stays_in_the_viewport() {
    let viewport = Rect::new(0.5, 1.5, 4.5, 4.);
    let path = series_path_clipped(&sample(), SeriesCurve::CatmullRom { alpha: 0.5 }, viewport);
    assert!(!path.elements().is_empty());
    for p in geom::flatten_polylines(&path, 1e-4).concat() {
        assert!(p.x >= 0.5 - 1e-6 && p.x <= 4.5 + 1e-6 && p.y >= 1.5 - 1e-6 && p.y <= 4. + 1e-6);
    }
}

fn radar() -> Vec<Point> {
    pts(&[(0., -4.), (3., -1.), (2., 3.), (-1., 2.5), (-3., -1.)])
}

/// The tangent leaving the start of the path and the one arriving at its end
/// (the closing segment's end, which is the start point).
fn seam_tangents(path: &BezPath) -> (geom::Vec2, geom::Vec2) {
    let segs: Vec<PathSeg> = path.segments().collect();
    let (first, last) = (segs[0], segs[segs.len() - 1]);
    (geom::deriv1(first, 0.), geom::deriv1(last, 1.))
}

#[test]
fn closed_curves_close_and_interpolate() {
    for curve in [
        ClosedCurve::Linear,
        ClosedCurve::Cardinal { tension: 0.3 },
        ClosedCurve::CatmullRom { alpha: 0. },
        ClosedCurve::CatmullRom { alpha: 0.5 },
        ClosedCurve::CatmullRom { alpha: 1. },
    ] {
        let path = series_path_closed(&radar(), curve);
        assert_eq!(
            path.elements().last(),
            Some(&PathEl::ClosePath),
            "{curve:?}"
        );
        let mut expected = radar();
        // Linear closes with ClosePath; curves draw the closing span.
        if curve != ClosedCurve::Linear {
            expected.push(radar()[0]);
        }
        assert_eq!(ends(&path), expected, "{curve:?}");
    }
}

#[test]
fn closed_smooth_curves_have_no_corner_at_the_seam() {
    for curve in [
        ClosedCurve::Basis,
        ClosedCurve::Cardinal { tension: 0. },
        ClosedCurve::CatmullRom { alpha: 0.5 },
    ] {
        let path = series_path_closed(&radar(), curve);
        let (out, back) = seam_tangents(&path);
        let cross = out.cross(back) / (out.hypot() * back.hypot());
        assert!(cross.abs() < 1e-9 && out.dot(back) > 0., "{curve:?}");
        assert_eq!(ends(&path).first(), ends(&path).last(), "{curve:?}");
    }
}

#[test]
fn closed_basis_matches_the_open_spline_on_wrapped_samples() {
    // Away from the ends, the open basis over the samples repeated three times
    // traces the closed curve; its middle copy must match span for span.
    let p = radar();
    let n = p.len();
    let tripled: Vec<Point> = p.iter().cycle().take(3 * n).copied().collect();
    let open: Vec<PathSeg> = series_path(&tripled, SeriesCurve::Basis)
        .segments()
        .collect();
    let closed: Vec<PathSeg> = series_path_closed(&p, ClosedCurve::Basis)
        .segments()
        .collect();
    // Open: line, then span i ends at knot(p[i-1], p[i], p[i+1]) for i in 1..; the
    // closed path's span j ends at knot(p[j], p[j+1], p[j+2]).
    for (j, seg) in closed.iter().enumerate().take(n) {
        let PathSeg::Cubic(c) = open[1 + n + j] else {
            panic!("cubic expected");
        };
        let PathSeg::Cubic(d) = *seg else {
            panic!("cubic expected");
        };
        for t in [0., 0.5, 1.] {
            assert!((c.eval(t) - d.eval(t)).hypot() < 1e-12, "span {j}");
        }
    }
}

#[test]
fn closed_tension_one_is_the_polygon() {
    let path = series_path_closed(&radar(), ClosedCurve::Cardinal { tension: 1. });
    for seg in path.segments() {
        let PathSeg::Cubic(c) = seg else { continue };
        let chord = c.p3 - c.p0;
        assert!(chord.cross(c.p1 - c.p0).abs() < 1e-12);
        assert!(chord.cross(c.p2 - c.p0).abs() < 1e-12);
    }
}

#[test]
fn closed_series_edge_cases() {
    assert!(series_path_closed(&[], ClosedCurve::Basis).is_empty());
    let single = series_path_closed(&pts(&[(1., 2.)]), ClosedCurve::Basis);
    assert_eq!(ends(&single), pts(&[(1., 2.)]));
    let pair = series_path_closed(&pts(&[(0., 0.), (1., 0.)]), ClosedCurve::Basis);
    assert_eq!(pair.elements().len(), 3);
    // A repeated first sample and a missing one are both tolerated.
    let mut p = radar();
    let clean = series_path_closed(&p, ClosedCurve::CatmullRom { alpha: 0.5 });
    p.push(p[0]);
    p.insert(2, Point::new(f64::NAN, 0.));
    let messy = series_path_closed(&p, ClosedCurve::CatmullRom { alpha: 0.5 });
    assert_eq!(clean, messy);
    // Closed area is the shape's, regardless of where the loop starts.
    let mut rotated = radar();
    rotated.rotate_left(2);
    for curve in [ClosedCurve::Basis, ClosedCurve::CatmullRom { alpha: 0.5 }] {
        let a = series_path_closed(&radar(), curve).area();
        let b = series_path_closed(&rotated, curve).area();
        assert!((a - b).abs() < 1e-9, "{curve:?}: {a} vs {b}");
    }
}

#[test]
fn step_area_baselines_step_on_the_same_side_as_the_top() {
    let top = pts(&[(0., 10.), (1., 20.), (2., 30.)]);
    let bottom = pts(&[(0., 0.), (1., 1.), (2., 2.)]);
    let inside = |curve, p: (f64, f64)| {
        let area = series_area(&top, curve, Baseline::Series(&bottom));
        geom::fill_contains(&area, Point::new(p.0, p.1), geom::FillRule::NonZero)
    };
    // StepAfter holds each value until the next x: the baseline is 1 on (1, 2).
    assert!(inside(SeriesCurve::StepAfter, (1.5, 1.5)));
    assert!(!inside(SeriesCurve::StepAfter, (1.5, 0.5)));
    // StepBefore changes first: the baseline is 1 on (0, 1).
    assert!(!inside(SeriesCurve::StepBefore, (0.5, 0.5)));
    assert!(inside(SeriesCurve::StepBefore, (0.5, 1.5)));
}

#[test]
fn monotone_area_baselines_are_curves_not_lines() {
    let top = pts(&[(0., 100.), (1., 100.), (2., 100.)]);
    let bottom = pts(&[(0., 0.), (1., 10.), (2., 0.)]);
    for curve in [SeriesCurve::MonotoneX, SeriesCurve::Natural] {
        let area = series_area(&top, curve, Baseline::Series(&bottom));
        // The baseline matches the same curve drawn forward on its own.
        let alone = series_path(&bottom, curve);
        for seg in alone.segments() {
            let mid = seg.eval(0.5);
            let below = Point::new(mid.x, mid.y - 0.05);
            let above = Point::new(mid.x, mid.y + 0.05);
            assert!(
                !geom::fill_contains(&area, below, geom::FillRule::NonZero),
                "{curve:?}"
            );
            assert!(
                geom::fill_contains(&area, above, geom::FillRule::NonZero),
                "{curve:?}"
            );
        }
    }
    let area = series_area(&top, SeriesCurve::MonotoneX, Baseline::Series(&bottom));
    // The PCHIP baseline is 7.5 at x = 0.5, so (0.5, 6) is below it.
    assert!(!geom::fill_contains(
        &area,
        Point::new(0.5, 6.),
        geom::FillRule::NonZero
    ));
}
