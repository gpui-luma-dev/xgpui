use super::*;
use crate::geom::{FillRule, Shape};

fn wave() -> BezPath {
    let mut p = BezPath::new();
    p.move_to((0., 0.));
    p.curve_to((30., 40.), (60., -40.), (100., 0.));
    p.quad_to((130., 30.), (160., 0.));
    p
}

fn inside(path: &BezPath, q: Point) -> bool {
    geom::fill_contains(path, q, FillRule::NonZero)
}

#[test]
fn constant_width_outline_matches_the_centerline_distance() {
    let center = wave();
    let outline = StrokeProfile::constant(10.)
        .unwrap()
        .outline(&center, 0.01)
        .unwrap();
    // Points well inside the stroke are filled, points well outside are not.
    for seg in center.segments() {
        for i in 1..20 {
            let t = i as f64 / 20.;
            let (p, n) = (seg.eval(t), geom::normal(seg, t).unwrap());
            assert!(
                inside(&outline, p + n * 4.5) && inside(&outline, p - n * 4.5),
                "{t}"
            );
            assert!(
                !inside(&outline, p + n * 5.6)
                    || !inside(&outline, p - n * 5.6)
                    || geom::curvature(seg, t).abs() > 0.05
            );
        }
    }
    // Outline boundary points sit at half the width (away from caps and joins).
    let (seg, t) = (center.segments().next().unwrap(), 0.4);
    let edge = seg.eval(t) + geom::normal(seg, t).unwrap() * 5.;
    let hit = geom::nearest_on_path(&outline, edge).unwrap();
    assert!(hit.distance < 0.01 + 1e-9, "{}", hit.distance);
}

#[test]
fn tapered_strokes_vanish_at_the_ends() {
    let mut line = BezPath::new();
    line.move_to((0., 0.));
    line.line_to((100., 0.));
    let profile = StrokeProfile::tapered(20., 0.25, 0.25).unwrap();
    assert_eq!(profile.width_at(0.).0, 0.);
    assert_eq!(profile.width_at(0.5).0, 20.);
    assert_eq!(profile.width_at(0.125), (10., 80.));
    let outline = profile.outline(&line, 0.01).unwrap();
    assert!(inside(&outline, Point::new(50., 9.)));
    assert!(!inside(&outline, Point::new(5., 3.)));
    assert!(inside(&outline, Point::new(12.5, 4.)));
    assert!(!inside(&outline, Point::new(12.5, 6.)));
}

#[test]
fn closed_strokes_are_rings_and_caps_extend() {
    let circle = kurbo::Circle::new((0., 0.), 50.).to_path(1e-6);
    let ring = StrokeProfile::constant(8.)
        .unwrap()
        .outline(&circle, 0.01)
        .unwrap();
    assert!(inside(&ring, Point::new(50., 0.)));
    assert!(!inside(&ring, Point::new(0., 0.)));
    assert!(!inside(&ring, Point::new(55., 0.)));

    let mut line = BezPath::new();
    line.move_to((0., 0.));
    line.line_to((10., 0.));
    for (cap, beyond) in [(Cap::Butt, false), (Cap::Square, true), (Cap::Round, true)] {
        let outline = StrokeProfile::constant(4.)
            .unwrap()
            .caps(cap)
            .outline(&line, 0.001)
            .unwrap();
        assert_eq!(inside(&outline, Point::new(11.5, 0.)), beyond, "{cap:?}");
        assert_eq!(inside(&outline, Point::new(-1.5, 0.)), beyond, "{cap:?}");
    }
}

#[test]
fn joins_cover_corners() {
    let mut corner = BezPath::new();
    corner.move_to((0., 0.));
    corner.line_to((50., 0.));
    corner.line_to((50., 50.));
    for (join, tip_filled) in [
        (Join::Miter(4.), true),
        (Join::Bevel, false),
        (Join::Round, false),
    ] {
        let outline = StrokeProfile::constant(10.)
            .unwrap()
            .with_join(join)
            .outline(&corner, 0.01)
            .unwrap();
        // The outer corner region: (54, −4) is inside the miter only.
        assert_eq!(
            inside(&outline, Point::new(54., -4.)),
            tip_filled,
            "{join:?}"
        );
        // Just around the corner point every join fills.
        assert!(inside(&outline, Point::new(52., -2.)), "{join:?}");
    }
}

#[test]
fn profiles_are_validated() {
    assert!(StrokeProfile::new(vec![]).is_err());
    assert!(StrokeProfile::new(vec![(0., -1.)]).is_err());
    assert!(StrokeProfile::new(vec![(0.5, 1.), (0.2, 1.)]).is_err());
    let kurbo_outline = kurbo_stroke(&wave(), &kurbo::Stroke::new(4.), 0.01);
    assert!(!kurbo_outline.elements().is_empty());
}
