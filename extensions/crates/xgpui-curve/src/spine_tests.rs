use super::*;

fn spines() -> Vec<CurveSpine> {
    vec![
        CurveSpine::new(Point::new(0., 0.), Point::new(0.5, 0.8), Point::new(1., 0.)).unwrap(),
        CurveSpine::new(
            Point::new(0.2, 0.3),
            Point::new(0.9, 0.1),
            Point::new(0.4, 0.9),
        )
        .unwrap(),
        CurveSpine::with_bend(Point::new(0., 0.), Point::new(1., 0.5), 0.15),
        // Collinear, non-uniform, back-tracking.
        CurveSpine::new(Point::new(0., 0.), Point::new(2., 0.), Point::new(1., 0.)).unwrap(),
    ]
}

fn brute(spine: &CurveSpine, p: Point, lo: f64, hi: f64) -> f64 {
    let q = spine.quad();
    let n = 20_000;
    let mut best = (lo, f64::MAX);
    for i in 0..=n {
        let t = lo + (hi - lo) * i as f64 / n as f64;
        let d = q.eval(t).distance(p);
        if d < best.1 {
            best = (t, d);
        }
    }
    let step = (hi - lo) / n as f64;
    let (mut a, mut b) = ((best.0 - step).max(lo), (best.0 + step).min(hi));
    for _ in 0..200 {
        let m1 = a + (b - a) * 0.381966;
        let m2 = a + (b - a) * 0.618034;
        if q.eval(m1).distance(p) < q.eval(m2).distance(p) {
            b = m2
        } else {
            a = m1
        }
    }
    q.eval(0.5 * (a + b)).distance(p).min(best.1)
}

#[test]
fn projection_matches_brute_force() {
    for spine in spines() {
        for i in 0..7 {
            for j in 0..7 {
                let p = Point::new(-0.3 + i as f64 * 0.27, -0.4 + j as f64 * 0.25);
                let hit = spine.project(p);
                assert!(
                    (hit.distance.abs() - brute(&spine, p, 0., 1.)).abs() <= 1e-6,
                    "{spine:?} {p:?}"
                );
                let trimmed = spine.project_range(p, 0.25, 0.7);
                assert!((0.25..=0.7).contains(&trimmed.t));
                assert!((trimmed.distance.abs() - brute(&spine, p, 0.25, 0.7)).abs() <= 1e-6);
            }
        }
    }
}

#[test]
fn degenerate_spines_stay_finite() {
    let straight = CurveSpine::straight(Point::new(0., 0.), Point::new(2., 0.));
    let hit = straight.project(Point::new(0.5, 1.));
    assert!((hit.t - 0.25).abs() < 1e-12 && (hit.distance - 1.).abs() < 1e-12);
    assert!((hit.s - 0.25).abs() < 1e-12);
    let dot = CurveSpine::straight(Point::new(1., 1.), Point::new(1., 1.));
    let hit = dot.project(Point::new(2., 1.));
    assert!(hit.t.is_finite() && (hit.distance.abs() - 1.).abs() < 1e-12 && hit.s == 0.);
    assert!(dot.terminal_tangent().is_none() && dot.arrowhead(1., 0.5).is_none());
    // Coincident control and endpoint.
    let hook = CurveSpine::new(Point::new(0., 0.), Point::new(1., 1.), Point::new(1., 1.)).unwrap();
    for i in 0..50 {
        let hit = hook.project(Point::new(i as f64 * 0.05, 1. - i as f64 * 0.03));
        assert!(hit.t.is_finite() && hit.distance.is_finite() && hit.s.is_finite());
    }
    assert_eq!(straight.max_curvature(), 0.);
    assert_eq!(straight.reach(), f64::INFINITY);
}

#[test]
fn signed_distance_and_arc_position() {
    // y-up math frame: the arch's normal (tangent turned +90°) points up at the apex.
    let arch =
        CurveSpine::new(Point::new(0., 0.), Point::new(0.5, 1.), Point::new(1., 0.)).unwrap();
    assert!(arch.project(Point::new(0.5, 0.8)).distance > 0.);
    assert!(arch.project(Point::new(0.5, 0.2)).distance < 0.);
    let apex = arch.project(Point::new(0.5, 2.));
    assert!((apex.t - 0.5).abs() < 1e-12 && (apex.s - 0.5).abs() < 1e-12);
    assert_eq!(arch.project(Point::new(-1., -1.)).s, 0.);
    assert_eq!(arch.project(Point::new(2., -1.)).s, 1.);
}

#[test]
fn arrowhead_follows_the_terminal_tangent() {
    let spine =
        CurveSpine::new(Point::new(0., 0.), Point::new(1., 1.), Point::new(2., 1.)).unwrap();
    let tangent = spine.terminal_tangent().unwrap();
    assert!((tangent - Vec2::new(1., 0.)).hypot() < 1e-12);
    let [left, tip, right] = spine.arrowhead(0.2, 0.5).unwrap();
    assert_eq!(tip, spine.p1);
    // Wings are symmetric about the reversed tangent.
    assert!((left.x - right.x).abs() < 1e-12 && (left.y - tip.y + right.y - tip.y).abs() < 1e-12);
    assert!(left.x < tip.x);
}

/// Largest jump in t between neighbours of an n × n grid over the spine's
/// surroundings, counting only points closer than `limit` to the spine.
fn max_jump(spine: &CurveSpine, limit: f64, n: usize) -> f64 {
    let at = |i: usize, j: usize| {
        Point::new(
            -0.5 + 2. * i as f64 / n as f64,
            -1. + 2. * j as f64 / n as f64,
        )
    };
    let mut worst: f64 = 0.;
    for i in 0..n {
        for j in 0..n {
            let (a, b, c) = (
                spine.project(at(i, j)),
                spine.project(at(i + 1, j)),
                spine.project(at(i, j + 1)),
            );
            if a.distance.abs() < limit && b.distance.abs() < limit && c.distance.abs() < limit {
                worst = worst.max((a.t - b.t).abs()).max((a.t - c.t).abs());
            }
        }
    }
    worst
}

#[test]
fn the_seam_rule_keeps_projection_continuous() {
    // A seam is a jump that does not shrink when the grid is refined; near the
    // centre of curvature t changes quickly but continuously, so its jumps do.
    let mut spine = CurveSpine::with_bend(Point::new(0., 0.), Point::new(1., 0.), 0.9);
    let radius = 0.35;
    assert!(spine.reach() < radius);
    let (coarse, fine) = (max_jump(&spine, radius, 50), max_jump(&spine, radius, 200));
    assert!(coarse > 0.2 && fine > 0.2, "seam expected: {coarse} {fine}");
    assert!(spine.limit_bend(radius));
    assert!(spine.reach() >= radius && spine.reach() < radius * 1.001);
    let (coarse, fine) = (
        max_jump(&spine, 0.7 * radius, 50),
        max_jump(&spine, 0.7 * radius, 200),
    );
    assert!(fine < coarse / 3., "continuous expected: {coarse} {fine}");
    assert!(!spine.limit_bend(radius));
}

#[test]
fn shader_params_pack_in_order() {
    let spine =
        CurveSpine::new(Point::new(1., 2.), Point::new(3., 4.), Point::new(5., 6.)).unwrap();
    assert_eq!(
        spine.shader_params(0.1, 0.9),
        [1., 2., 3., 4., 5., 6., 0.1, 0.9]
    );
    assert!(CurveSpine::new(Point::new(f64::NAN, 0.), Point::ZERO, Point::ZERO).is_err());
}

#[test]
fn a_spine_folded_into_a_cusp_has_zero_reach_and_unbends() {
    // Control collinear with the ends but beyond p1: the spine runs out to
    // x = 4/3 and turns back (a cusp, infinite curvature).
    let mut spine =
        CurveSpine::new(Point::new(0., 0.), Point::new(2., 0.), Point::new(1., 0.)).unwrap();
    assert_eq!(spine.max_curvature(), f64::INFINITY);
    assert_eq!(spine.reach(), 0.);
    assert!(spine.limit_bend(5.));
    assert!(spine.reach() >= 5.);
    // A control inside the chord is a straight spine.
    let straight =
        CurveSpine::new(Point::new(0., 0.), Point::new(0.6, 0.), Point::new(1., 0.)).unwrap();
    assert_eq!(straight.reach(), f64::INFINITY);
}

#[test]
fn a_straight_spine_with_its_control_on_an_end_is_not_a_cusp() {
    // B(t) = p0 + t²(p1 − p0): straight, with zero speed only at t = 0 (or 1).
    for (p0, c, p1) in [
        ((0., 0.), (0., 0.), (1., 1.)),
        ((0., 0.), (1., 1.), (1., 1.)),
        // Control inside the chord: straight, never stops.
        ((0., 0.), (0.25, 0.25), (1., 1.)),
    ] {
        let pt = |(x, y): (f64, f64)| Point::new(x, y);
        let mut spine = CurveSpine::new(pt(p0), pt(c), pt(p1)).unwrap();
        assert_eq!(spine.max_curvature(), 0., "{c:?}");
        assert_eq!(spine.reach(), f64::INFINITY, "{c:?}");
        assert!(!spine.limit_bend(0.5), "{c:?}");
        assert_eq!(spine.quad().p1, pt(c));
    }
}

#[test]
fn nan_range_bounds_mean_the_whole_spine() {
    let spine = CurveSpine::with_bend(Point::new(0., 0.), Point::new(1., 0.), 0.3);
    let p = Point::new(0.5, 0.5);
    let whole = spine.project(p);
    assert_eq!(spine.project_range(p, f64::NAN, 1.), whole);
    assert_eq!(spine.project_range(p, 0., f64::NAN), whole);
    assert_eq!(spine.project_range(p, f64::NAN, f64::NAN), whole);
}
