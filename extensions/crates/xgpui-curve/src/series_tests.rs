use super::*;
use crate::geom::{self, ParamCurve, PathEl, PathSeg};

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
            PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => Some(*p),
            PathEl::ClosePath => None,
        })
        .collect()
}

#[test]
fn step_variants_match_d3_vertices() {
    let p = pts(&[(0., 0.), (2., 1.), (4., 0.)]);
    let step = ends(&series_path(&p, SeriesCurve::Step));
    assert_eq!(step, pts(&[(0., 0.), (1., 0.), (1., 1.), (3., 1.), (3., 0.), (4., 0.)]));
    let before = ends(&series_path(&p, SeriesCurve::StepBefore));
    assert_eq!(before, pts(&[(0., 0.), (0., 1.), (2., 1.), (2., 0.), (4., 0.)]));
    let after = ends(&series_path(&p, SeriesCurve::StepAfter));
    assert_eq!(after, pts(&[(0., 0.), (2., 0.), (2., 1.), (4., 1.), (4., 0.)]));
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
    let segs: Vec<PathSeg> = series_path(&sample(), SeriesCurve::Natural).segments().collect();
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
    let segs: Vec<PathSeg> = series_path(&sample(), SeriesCurve::Cardinal { tension: 0. }).segments().collect();
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
    assert_eq!(series_path(&sample(), SeriesCurve::Bundle { beta: 1. }), path);
}

#[test]
fn monotone_x_never_overshoots_and_falls_back_on_reversal() {
    let data = pts(&[(0., 0.), (1., 0.), (1.5, 1.), (3., 1.), (3.2, 0.2), (5., 0.)]);
    for seg in series_path(&data, SeriesCurve::MonotoneX).segments() {
        let (lo, hi) = (seg.start().y.min(seg.end().y), seg.start().y.max(seg.end().y));
        for i in 0..=50 {
            let y = seg.eval(i as f64 / 50.).y;
            assert!(y >= lo - 1e-12 && y <= hi + 1e-12);
        }
    }
    let reversing = pts(&[(0., 0.), (2., 1.), (1., 2.)]);
    let path = series_path(&reversing, SeriesCurve::MonotoneX);
    assert!(path.segments().all(|s| matches!(s, PathSeg::Line(_))));
    let vertical = pts(&[(0., 0.), (1., 1.), (0.5, 2.), (2., 3.)]);
    assert_eq!(ends(&series_path(&vertical, SeriesCurve::MonotoneY)), vertical);
}

#[test]
fn non_finite_samples_break_the_line() {
    let mut data = sample();
    data.insert(2, Point::new(f64::NAN, 1.));
    let path = series_path(&data, SeriesCurve::CatmullRom { alpha: 0.5 });
    assert_eq!(path.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count(), 2);
    assert!(series_path(&[], SeriesCurve::Linear).elements().is_empty());
}

#[test]
fn areas_are_closed_per_run() {
    let mut data = sample();
    data.push(Point::new(f64::INFINITY, 0.));
    data.extend(pts(&[(7., 1.), (8., 2.)]));
    let area = series_area(&data, SeriesCurve::Natural, Baseline::Constant(0.));
    let closes = area.elements().iter().filter(|e| matches!(e, PathEl::ClosePath)).count();
    assert_eq!(closes, 2);
    assert!(geom::fill_contains(&area, Point::new(1., 0.5), geom::FillRule::NonZero));
    assert!(!geom::fill_contains(&area, Point::new(1., -0.5), geom::FillRule::NonZero));
    let lower: Vec<Point> = sample().iter().map(|p| Point::new(p.x, p.y - 1.)).collect();
    let band = series_area(&sample(), SeriesCurve::Linear, Baseline::Series(&lower));
    assert!(geom::fill_contains(&band, Point::new(0.5, 1.5), geom::FillRule::NonZero));
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
