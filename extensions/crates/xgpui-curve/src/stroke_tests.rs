use super::*;
use crate::geom::FillRule;

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
    let circle = geom::circle_path((0., 0.), 50., 1e-6);
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
    let outline = stroke_outline(&wave(), &StrokeStyle::new(4.), 0.01);
    assert!(!outline.elements().is_empty());
}

/// With round joins and caps a stroke is exactly the points within half the
/// width of the centerline. Checks a grid of points well inside and well
/// outside that boundary (by `margin`).
fn assert_covers_exactly(path: &BezPath, width: f64, margin: f64) {
    let out = StrokeProfile::constant(width)
        .unwrap()
        .caps(Cap::Round)
        .with_join(Join::Round)
        .outline(path, 1e-3)
        .unwrap();
    let half = width / 2.;
    let bb = path.bounding_box().inflate(half + 1., half + 1.);
    for i in 0..60 {
        for j in 0..60 {
            let q = Point::new(
                bb.x0 + (i as f64 + 0.5) * bb.width() / 60.,
                bb.y0 + (j as f64 + 0.5) * bb.height() / 60.,
            );
            let d = geom::nearest_on_path(path, q).unwrap().distance.abs();
            let filled = geom::fill_contains(&out, q, geom::FillRule::NonZero);
            if d < half - margin {
                assert!(filled, "hole at {q:?}, {d:.3} from the centerline");
            }
            if d > half + margin {
                assert!(!filled, "fill at {q:?}, {d:.3} from the centerline");
            }
        }
    }
}

fn svg(d: &str) -> BezPath {
    crate::svg_path::parse(d).unwrap()
}

#[test]
fn cusps_and_tight_bends_leave_no_holes() {
    // A cusp at (5, 7.5), and a bend tighter than the half width (radius 4.49).
    assert_covers_exactly(&svg("M0,0 C10,10 0,10 10,0"), 4., 0.05);
    assert_covers_exactly(&svg("M0,0 C50,-40 100,40 100,0"), 30., 0.05);
    // The curvature peaks inside the clamped stretch; its evolute cusp must
    // not make the inner side overshoot.
    assert_covers_exactly(
        &svg("M30.55,75.72 C76.03,27.31 40.11,50.72 58.9,67.75 Z"),
        22.7,
        0.05,
    );
    // Starts inside a bend tighter than the half width.
    assert_covers_exactly(
        &svg("M4.62,42.81 C14.21,57.79 73.22,73.61 43.79,16.89"),
        30.3,
        0.05,
    );
}

#[test]
fn reversals_and_single_segment_seams_get_their_joins() {
    // An exact 180° turn gets a round end.
    assert_covers_exactly(&svg("M0,0 L10,0 L0,0"), 2., 0.05);
    // A closed subpath of one curve gets a join at its seam.
    let one = svg("M0,0 C100,100 100,-100 0,0 Z");
    let miter = StrokeProfile::constant(10.)
        .unwrap()
        .with_join(Join::Miter(4.))
        .outline(&one, 1e-3)
        .unwrap();
    assert!(geom::fill_contains(
        &miter,
        Point::new(-6., 0.),
        geom::FillRule::NonZero
    ));
    assert_covers_exactly(&one, 10., 0.05);
}

#[test]
fn random_round_strokes_cover_exactly() {
    let mut seed = 0x5eed_u64;
    let mut rnd = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..24 {
        let mut path = BezPath::new();
        let mut pt = || Point::new(rnd() * 100., rnd() * 100.);
        path.move_to(pt());
        for _ in 0..2 {
            path.curve_to(pt(), pt(), pt());
        }
        let width = 2. + rnd() * 28.;
        assert_covers_exactly(&path, width, 0.05);
    }
}

#[test]
fn an_empty_subpath_does_not_shift_later_tapers() {
    let profile = StrokeProfile::tapered(10., 0.5, 0.5).unwrap();
    let alone = profile.outline(&svg("M0,10 L100,10"), 1e-3).unwrap();
    let after_empty = profile
        .outline(&svg("M0,0 L0,0 M0,10 L100,10"), 1e-3)
        .unwrap();
    let q = Point::new(50., 14.);
    assert!(geom::fill_contains(&alone, q, geom::FillRule::NonZero));
    assert!(geom::fill_contains(
        &after_empty,
        q,
        geom::FillRule::NonZero
    ));
}
#[test]
fn shared_footprint_covers_caps_join_and_boundary_but_leaves_dash_gaps() {
    let mut path = BezPath::new();
    path.move_to((0., 0.));
    path.line_to((25., 0.));
    path.line_to((25., 100.));
    path.move_to((100., 0.));
    path.line_to((100., 100.));
    let mut style = StrokeStyle::new(10.);
    style.start_cap = Cap::Square;
    style.end_cap = Cap::Round;
    style.join = Join::Miter(4.);
    style.dash_pattern = vec![60., 30.];
    let transform = geom::Affine::scale_non_uniform(2., 1.);

    for (space, left, right, join_x, round_tip, gap) in [
        (StrokeSpace::Model, -10., 210., 58., 40., 50.),
        (StrokeSpace::LogicalPixels, -5., 205., 54., 15., 25.),
    ] {
        let footprint = StrokeFootprint::new(&path, &style, transform, space, 0.001).unwrap();
        let hit = |x, y| footprint.contains(Point::new(x, y), 0.);
        let bounds = footprint.bounds().unwrap();
        assert!((bounds.x0 - left).abs() < 1e-6);
        assert!(hit(left, 4.));
        assert!(!hit(left - 1., 4.));
        assert!(footprint.contains(Point::new(left - 1., 4.), 1.));
        assert!((bounds.x1 - right).abs() < 1e-6);
        assert!(hit(200., 55.));
        assert!(!hit(200., 75.));
        assert!(hit(join_x, -4.));
        assert!(hit(50., round_tip));
        assert!(!hit(join_x, round_tip - 1.));
        assert!(!hit(50., gap));
    }
}

#[test]
fn shared_footprint_validates_inputs_and_omits_zero_length_contours() {
    let mut path = BezPath::new();
    path.move_to((3., 4.));
    path.line_to((3., 4.));
    let mut style = StrokeStyle::new(2.);
    let build = |style: &StrokeStyle| {
        StrokeFootprint::new(
            &path,
            style,
            geom::Affine::IDENTITY,
            StrokeSpace::Model,
            0.01,
        )
    };
    let footprint = build(&style).unwrap();
    assert!(footprint.bounds().is_none());
    assert!(!footprint.contains(Point::new(3., 4.), 100.));
    style.dash_pattern = vec![0., 2.];
    assert!(matches!(
        build(&style),
        Err(CurveError::OutOfRange("dash length"))
    ));
}

#[test]
fn shared_profile_restarts_at_each_contour_in_both_spaces() {
    let mut path = BezPath::new();
    for y in [0., 50.] {
        path.move_to((0., y));
        path.line_to((100., y));
    }
    let profile = StrokeProfile::new(vec![(0., 2.), (1., 18.)]).unwrap();
    let transform = geom::Affine::scale_non_uniform(1., 2.);
    for space in [StrokeSpace::Model, StrokeSpace::LogicalPixels] {
        let footprint =
            StrokeFootprint::from_profile(&path, &profile, transform, space, 0.01).unwrap();
        for y in [0., 100.] {
            assert!(!footprint.contains(Point::new(10., y + 5.), 0.));
            assert!(footprint.contains(Point::new(90., y + 7.), 0.));
            assert_eq!(
                footprint.contains(Point::new(90., y + 12.), 0.),
                space == StrokeSpace::Model
            );
        }
    }
}
