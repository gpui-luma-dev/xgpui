use super::*;

fn segs() -> Vec<PathSeg> {
    vec![
        PathSeg::Line(Line::new((0., 0.), (3., 4.))),
        PathSeg::Quad(QuadBez::new((0., 0.), (2., 3.), (4., -1.))),
        PathSeg::Cubic(CubicBez::new((0., 0.), (1., 3.), (3., -2.), (4., 1.))),
        PathSeg::Cubic(CubicBez::new((0., 0.), (4., 4.), (0., 4.), (4., 0.))),
    ]
}

#[test]
fn hermite_round_trips_exactly() {
    let c = CubicBez::new((0.25, -1.), (1., 3.), (3., -2.), (4., 1.5));
    let h = Hermite::from_cubic(c);
    assert_eq!(h.to_cubic(), c);
    assert_eq!(h.m0, Vec2::new(2.25, 12.));
}

#[test]
fn derivatives_match_finite_differences() {
    for seg in segs() {
        for i in 1..20 {
            let t = i as f64 / 20.;
            let h = 1e-6;
            let fd1 = (seg.eval(t + h) - seg.eval(t - h)) / (2. * h);
            let fd2 = (deriv1(seg, t + h) - deriv1(seg, t - h)) / (2. * h);
            assert!((deriv1(seg, t) - fd1).hypot() < 1e-6, "{seg:?} {t}");
            assert!((deriv2(seg, t) - fd2).hypot() < 1e-5, "{seg:?} {t}");
        }
    }
}

#[test]
fn curvature_matches_parabola() {
    // x = 2t − 1, y = x².
    let q = PathSeg::Quad(QuadBez::new((-1., 1.), (0., -1.), (1., 1.)));
    for i in 0..=10 {
        let t = i as f64 / 10.;
        let x = 2. * t - 1.;
        let expected = 2. / (1. + 4. * x * x).powf(1.5);
        assert!((curvature(q, t) - expected).abs() < 1e-12);
    }
}

#[test]
fn tangent_handles_coincident_controls() {
    let c = PathSeg::Cubic(CubicBez::new((0., 0.), (0., 0.), (1., 1.), (2., 0.)));
    let t0 = tangent(c, 0.).unwrap();
    assert!((t0 - Vec2::new(1., 1.).normalize()).hypot() < 1e-12);
    let end = PathSeg::Cubic(CubicBez::new((0., 0.), (1., 1.), (2., 0.), (2., 0.)));
    let t1 = tangent(end, 1.).unwrap();
    assert!((t1 - Vec2::new(1., -1.).normalize()).hypot() < 1e-12);
    assert!(tangent(PathSeg::Line(Line::new((1., 1.), (1., 1.))), 0.5).is_none());
}

#[test]
fn tight_bounds_match_dense_sampling() {
    for seg in segs() {
        let bounds = tight_bounds(seg);
        let mut sampled = Rect::from_points(seg.start(), seg.start());
        for i in 0..=100_000 {
            sampled = sampled.union_pt(seg.eval(i as f64 / 100_000.));
        }
        // Bounds contain every sample and are within sampling error of them.
        for (a, b) in [(bounds.x0, sampled.x0), (bounds.y0, sampled.y0)] {
            assert!(a <= b + 1e-12 && b - a < 1e-8, "{seg:?}");
        }
        for (a, b) in [(bounds.x1, sampled.x1), (bounds.y1, sampled.y1)] {
            assert!(a >= b - 1e-12 && a - b < 1e-8, "{seg:?}");
        }
    }
}

fn brute_nearest(seg: PathSeg, p: Point) -> f64 {
    let n = 20_000;
    let mut best = (0., f64::MAX);
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let d = seg.eval(t).distance(p);
        if d < best.1 {
            best = (t, d);
        }
    }
    // Golden-section refinement around the sampled minimum.
    let (mut lo, mut hi) = ((best.0 - 1. / n as f64).max(0.), (best.0 + 1. / n as f64).min(1.));
    for _ in 0..200 {
        let m1 = lo + (hi - lo) * 0.381966;
        let m2 = lo + (hi - lo) * 0.618034;
        if seg.eval(m1).distance(p) < seg.eval(m2).distance(p) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    seg.eval(0.5 * (lo + hi)).distance(p).min(best.1)
}

#[test]
fn nearest_is_within_a_micro_unit() {
    for seg in segs() {
        for p in [Point::new(1., 1.), Point::new(-2., 3.), Point::new(5., -4.), Point::new(2., 2.5)] {
            let hit = nearest(seg, p);
            assert!((hit.distance - brute_nearest(seg, p)).abs() <= 1e-6, "{seg:?} {p:?}");
            assert_eq!(hit.signed_distance.abs(), hit.distance);
        }
    }
    let line = PathSeg::Line(Line::new((0., 0.), (1., 0.)));
    assert!(nearest(line, Point::new(0.5, 1.)).signed_distance > 0.);
    assert!(nearest(line, Point::new(0.5, -1.)).signed_distance < 0.);
}

#[test]
fn split_preserves_geometry_and_derivatives() {
    for seg in segs() {
        let t = 0.37;
        let (left, right) = split(seg, t);
        assert_eq!(left.end(), right.start());
        for i in 0..=10 {
            let s = i as f64 / 10.;
            assert!(left.eval(s).distance(seg.eval(s * t)) < 1e-12);
            assert!(right.eval(s).distance(seg.eval(t + s * (1. - t))) < 1e-12);
        }
        assert!((deriv1(left, 1.) / t - deriv1(seg, t)).hypot() < 1e-9);
        assert!((deriv1(right, 0.) / (1. - t) - deriv1(seg, t)).hypot() < 1e-9);
    }
}

#[test]
fn curve_curve_intersections_lie_on_both_curves() {
    let a = PathSeg::Cubic(CubicBez::new((0., 0.), (1., 3.), (3., -2.), (4., 1.)));
    let b = PathSeg::Cubic(CubicBez::new((0., 1.), (1., -2.), (3., 3.), (4., 0.)));
    let hits = intersect_segments(a, b, 1e-9);
    assert_eq!(hits.len(), 3, "{hits:?}");
    for hit in &hits {
        assert!(a.eval(hit.t_a).distance(b.eval(hit.t_b)) < 1e-9);
    }
    let line = PathSeg::Line(Line::new((0., 0.5), (4., 0.5)));
    for hit in intersect_segments(a, line, 1e-9) {
        assert!((a.eval(hit.t_a).y - 0.5).abs() < 1e-9);
    }
    assert!(intersect_segments(a, PathSeg::Line(Line::new((10., 10.), (11., 11.))), 1e-9).is_empty());
}

#[test]
fn clipping_keeps_inside_parts_with_edges_on_the_rect() {
    let mut path = BezPath::new();
    path.move_to((-1., 0.5));
    path.line_to((2., 0.5));
    path.curve_to((3., 0.), (3., 1.), (0.5, 0.75));
    let clipped = clip_to_rect(&path, Rect::new(0., 0., 1., 1.));
    let segs: Vec<_> = clipped.segments().collect();
    assert!((segs[0].start() - Point::new(0., 0.5)).hypot() < 1e-12);
    assert!((segs[0].end() - Point::new(1., 0.5)).hypot() < 1e-12);
    let last = segs.last().unwrap().end();
    assert!((last - Point::new(0.5, 0.75)).hypot() < 1e-12);
    for seg in segs {
        for i in 0..=8 {
            let p = seg.eval(i as f64 / 8.);
            assert!((-1e-9..=1. + 1e-9).contains(&p.x) && (-1e-9..=1. + 1e-9).contains(&p.y));
        }
    }
}

#[test]
fn fill_rules_and_stroke_hits() {
    let mut path = BezPath::new();
    path.move_to((0., 0.));
    path.line_to((4., 0.));
    path.line_to((4., 4.));
    path.line_to((0., 4.));
    path.close_path();
    path.move_to((1., 1.));
    path.line_to((3., 1.));
    path.line_to((3., 3.));
    path.line_to((1., 3.));
    path.close_path();
    let hole = Point::new(2., 2.);
    assert!(fill_contains(&path, hole, FillRule::NonZero));
    assert!(!fill_contains(&path, hole, FillRule::EvenOdd));
    assert!(fill_contains(&path, Point::new(0.5, 0.5), FillRule::EvenOdd));
    assert!(stroke_contains(&path, Point::new(2., 0.2), 0.5));
    assert!(!stroke_contains(&path, Point::new(2., 0.4), 0.5));
    let hit = nearest_on_path(&path, Point::new(4.5, 2.)).unwrap();
    assert_eq!(hit.segment, 1);
}

#[test]
fn flatten_closes_and_stays_within_tolerance() {
    let mut path = BezPath::new();
    path.move_to((0., 0.));
    path.curve_to((0., 10.), (10., 10.), (10., 0.));
    path.close_path();
    let lines = flatten_polylines(&path, 0.01);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].first(), lines[0].last());
    let seg = path.segments().next().unwrap();
    for p in &lines[0] {
        if p.y > 1e-9 {
            assert!(nearest(seg, *p).distance <= 0.01 + 1e-9);
        }
    }
}
