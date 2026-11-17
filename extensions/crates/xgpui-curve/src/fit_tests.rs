use super::*;
use crate::geom;

/// Deterministic pseudo-noise in [−1, 1].
fn noise(i: usize) -> f64 {
    let x = ((i as u64)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
        >> 33) as f64;
    (x / (1u64 << 31) as f64) * 2. - 1.
}

fn max_distance(path: &BezPath, points: &[Point]) -> f64 {
    points
        .iter()
        .map(|p| geom::nearest_on_path(path, *p).unwrap().distance)
        .fold(0., f64::max)
}

#[test]
fn noisy_strokes_fit_within_tolerance_with_few_segments() {
    let points: Vec<Point> = (0..400)
        .map(|i| {
            let t = i as f64 / 399. * 3.;
            Point::new(
                100. * t.cos() + 0.3 * noise(i),
                60. * (2. * t).sin() + 0.3 * noise(i + 999),
            )
        })
        .collect();
    let options = FitOptions {
        tolerance: 1.,
        corner_angle: None,
        simplify: None,
    };
    let path = fit_cubic_path(&points, options).unwrap();
    assert!(max_distance(&path, &points) <= 1. + 1e-9);
    let segments = path.segments().count();
    assert!(segments <= 20, "{segments} segments");
}

#[test]
fn corners_stay_sharp() {
    let mut points: Vec<Point> = (0..=20).map(|i| Point::new(i as f64 * 5., 0.)).collect();
    points.extend((1..=20).map(|i| Point::new(100., i as f64 * 5.)));
    let path = fit_cubic_path(
        &points,
        FitOptions {
            tolerance: 0.1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(max_distance(&path, &points) <= 0.1 + 1e-9);
    let segs: Vec<_> = path.segments().collect();
    let joint = segs
        .iter()
        .position(|s| (s.end() - Point::new(100., 0.)).hypot() < 1e-9)
        .expect("corner kept as a node");
    let (a, b) = (
        geom::tangent(segs[joint], 1.).unwrap(),
        geom::tangent(segs[joint + 1], 0.).unwrap(),
    );
    assert!(a.dot(b).abs() < 1e-9, "tangents should turn 90°");
}

#[test]
fn rdp_drops_collinear_points() {
    let points: Vec<Point> = (0..10)
        .map(|i| Point::new(i as f64, 2. * i as f64))
        .collect();
    assert_eq!(rdp(&points, 1e-9), vec![points[0], points[9]]);
    let bump = vec![
        Point::new(0., 0.),
        Point::new(1., 0.01),
        Point::new(2., 1.),
        Point::new(3., 0.),
    ];
    // (1, 0.01) is 0.44 from the (0,0)–(2,1) chord: kept at 0.1, dropped at 0.5.
    assert_eq!(rdp(&bump, 0.1), bump);
    assert_eq!(rdp(&bump, 0.5), vec![bump[0], bump[2], bump[3]]);
}

#[test]
fn degenerate_input() {
    assert!(
        fit_cubic_path(&[], FitOptions::default())
            .unwrap()
            .elements()
            .is_empty()
    );
    let one = fit_cubic_path(&[Point::new(1., 1.); 5], FitOptions::default()).unwrap();
    assert_eq!(one.elements().len(), 1);
    assert!(fit_cubic_path(&[Point::new(f64::NAN, 0.)], FitOptions::default()).is_err());
    let two = fit_cubic_path(
        &[Point::new(0., 0.), Point::new(3., 0.)],
        FitOptions::default(),
    )
    .unwrap();
    assert_eq!(two.segments().count(), 1);
}
