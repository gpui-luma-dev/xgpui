use super::*;

fn grid() -> MeshLattice {
    // A warped 3 × 4 lattice in unit space, like the lab's sample grid.
    let mut points = Vec::new();
    for r in 0..3 {
        for c in 0..4 {
            let (u, v) = (c as f64 / 3., r as f64 / 2.);
            points.push(Point::new(
                u + 0.06 * (v * 5.).sin(),
                v + 0.08 * (u * 4.).cos(),
            ));
        }
    }
    MeshLattice::new(3, 4, points).unwrap()
}

/// `mesh_tangent_position` from xgpui-luma `paint/mesh.rs`, in f32, as the
/// oracle for the lab migration.
fn lab_tangent(m: &MeshLattice, row: usize, col: usize, horizontal: bool) -> (f32, f32) {
    let p = |r: usize, c: usize| {
        let q = m.point(r, c);
        (q.x as f32, q.y as f32)
    };
    let sub = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0, a.1 - b.1);
    let dist = |a: (f32, f32), b: (f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    let (rows, cols) = (m.rows(), m.cols());
    let (raw, limit) = if horizontal {
        if col == 0 {
            (sub(p(row, 1), p(row, 0)), dist(p(row, 0), p(row, 1)))
        } else if col + 1 == cols {
            (
                sub(p(row, col), p(row, col - 1)),
                dist(p(row, col), p(row, col - 1)),
            )
        } else {
            let d = sub(p(row, col + 1), p(row, col - 1));
            (
                (d.0 * 0.5, d.1 * 0.5),
                dist(p(row, col), p(row, col - 1)).min(dist(p(row, col), p(row, col + 1))),
            )
        }
    } else if row == 0 {
        (sub(p(1, col), p(0, col)), dist(p(0, col), p(1, col)))
    } else if row + 1 == rows {
        (
            sub(p(row, col), p(row - 1, col)),
            dist(p(row, col), p(row - 1, col)),
        )
    } else {
        let d = sub(p(row + 1, col), p(row - 1, col));
        (
            (d.0 * 0.5, d.1 * 0.5),
            dist(p(row, col), p(row - 1, col)).min(dist(p(row, col), p(row + 1, col))),
        )
    };
    let max = limit * 0.7;
    let length = (raw.0 * raw.0 + raw.1 * raw.1).sqrt();
    if length <= f32::EPSILON || length <= max {
        raw
    } else {
        (raw.0 * max / length, raw.1 * max / length)
    }
}

#[test]
fn automatic_tangents_match_the_lab() {
    let m = grid();
    for r in 0..3 {
        for c in 0..4 {
            for (axis, horizontal) in [(Axis::U, true), (Axis::V, false)] {
                let ours = m.resolved_tangent(r, c, axis);
                let lab = lab_tangent(&m, r, c, horizontal);
                assert!(
                    (ours.x as f32 - lab.0).abs() < 1e-6 && (ours.y as f32 - lab.1).abs() < 1e-6
                );
            }
        }
    }
    let controls = m.controls();
    assert_eq!(controls.len(), 12);
    assert_eq!(
        controls[5].position,
        [m.point(1, 1).x as f32, m.point(1, 1).y as f32]
    );
}

#[test]
fn edges_are_shared_and_continuous() {
    let m = grid();
    let b = m.cell_boundary(0, 1);
    assert_eq!(b.top, m.edge(0, 1, Axis::U));
    assert_eq!(b.right, m.cell_boundary(0, 2).left);
    // Row paths are C¹ through interior nodes because the tangent is shared.
    let a = m.edge(1, 0, Axis::U);
    let c = m.edge(1, 1, Axis::U);
    assert_eq!(a.p3, c.p0);
    assert!(((a.p3 - a.p2) - (c.p1 - c.p0)).hypot() < 1e-15);
    assert_eq!(m.row_path(1).segments().count(), 3);
    assert_eq!(m.col_path(2).segments().count(), 2);
    assert_eq!(m.wireframe().segments().count(), 3 * 3 + 4 * 2);
}

/// The patch position as xgpui-shader's `mesh.wgsl` evaluates it: a
/// zero-twist bicubic Hermite patch from corner positions and tangents.
fn shader_patch(m: &MeshLattice, row: usize, col: usize, u: f64, v: f64) -> Point {
    let h = |t: f64| {
        [
            2. * t * t * t - 3. * t * t + 1.,
            -2. * t * t * t + 3. * t * t,
            t * t * t - 2. * t * t + t,
            t * t * t - t * t,
        ]
    };
    let (hu, hv) = (h(u), h(v));
    let corners = [
        (row, col, 0, 0),
        (row, col + 1, 1, 0),
        (row + 1, col, 0, 1),
        (row + 1, col + 1, 1, 1),
    ];
    let mut sum = Vec2::ZERO;
    for (r, c, i, j) in corners {
        sum += m.point(r, c).to_vec2() * (hu[i] * hv[j])
            + m.resolved_tangent(r, c, Axis::U) * (hu[2 + i] * hv[j])
            + m.resolved_tangent(r, c, Axis::V) * (hu[i] * hv[2 + j]);
    }
    sum.to_point()
}

#[test]
fn patch_and_iso_curves_match_the_mesh_shader() {
    // Jittered so the tangents differ around the cell, where a Coons patch
    // and the shader's Hermite patch disagree.
    let points = (0..3)
        .flat_map(|r| (0..4).map(move |c| (r, c)))
        .map(|(r, c)| {
            let j = ((r * 7 + c * 13) % 5) as f64 * 0.04 - 0.08;
            Point::new(c as f64 * 0.5 + j, r as f64 * 0.5 - j * 0.7)
        })
        .collect();
    let mut m = MeshLattice::new(3, 4, points).unwrap();
    m.set_tangent(1, 2, Axis::V, Tangent::Explicit(Vec2::new(0.1, 0.3)))
        .unwrap();
    for (axis, s) in [
        (Axis::U, 0.3),
        (Axis::V, 0.65),
        (Axis::U, 0.),
        (Axis::V, 1.),
    ] {
        let iso = m.iso_curve(1, 2, axis, s);
        for i in 0..=10 {
            let k = i as f64 / 10.;
            let (u, v) = match axis {
                Axis::U => (k, s),
                Axis::V => (s, k),
            };
            let expected = shader_patch(&m, 1, 2, u, v);
            assert!(iso.eval(k).distance(expected) < 1e-12, "{axis:?} {s} {k}");
            assert!(m.patch_point(1, 2, u, v).distance(expected) < 1e-12);
        }
    }
    // The v = 0 iso-curve is the top edge.
    let top = m.iso_curve(0, 0, Axis::U, 0.);
    let edge = m.edge(0, 0, Axis::U);
    for i in 0..=8 {
        assert!(top.eval(i as f64 / 8.).distance(edge.eval(i as f64 / 8.)) < 1e-12);
    }
}

#[test]
fn edge_drag_keeps_the_grabbed_point_under_the_cursor() {
    let mut m = grid();
    m.set_tangent_limit(10.).unwrap();
    for t in [0.2, 0.5, 0.8] {
        let before = m.edge(1, 1, Axis::U).eval(t);
        let delta = Vec2::new(0.01, -0.03);
        m.drag_edge(1, 1, Axis::U, t, delta).unwrap();
        let after = m.edge(1, 1, Axis::U).eval(t);
        assert!((after - (before + delta)).hypot() < 1e-6, "{t}");
    }
    assert!(matches!(m.tangent(1, 1, Axis::U), Tangent::Explicit(_)));
    assert!(matches!(m.tangent(1, 2, Axis::U), Tangent::Explicit(_)));
    m.reset_tangents();
    assert_eq!(m, {
        let mut g = grid();
        g.set_tangent_limit(10.).unwrap();
        g
    });
}

#[test]
fn edge_drag_is_exact_with_the_default_fold_clamp() {
    // Automatic tangents start at the clamp, so a drag along the wire
    // lengthens a clamped tangent; the other control must take that share.
    for (row, col) in [(0, 0), (1, 1), (2, 2)] {
        for t in [0.3, 0.5, 0.7] {
            for sign in [1., -1.] {
                let mut m = grid();
                let edge = m.edge(row, col, Axis::U);
                let delta = (edge.p3 - edge.p0).normalize() * (0.01 * sign);
                let before = edge.eval(t);
                m.drag_edge(row, col, Axis::U, t, delta).unwrap();
                let after = m.edge(row, col, Axis::U).eval(t);
                assert!(
                    (after - (before + delta)).hypot() < 1e-9,
                    "({row}, {col}) t {t} sign {sign}: moved {:?}, wanted {delta:?}",
                    after - before
                );
            }
        }
    }
}

#[test]
fn edge_drag_rejects_huge_indices_without_overflow() {
    let mut m = grid();
    assert!(
        m.drag_edge(usize::MAX, 0, Axis::V, 0.5, Vec2::new(0., 0.1))
            .is_err()
    );
    assert!(
        m.drag_edge(0, usize::MAX, Axis::U, 0.5, Vec2::new(0., 0.1))
            .is_err()
    );
}

#[test]
fn fold_clamp_applies_to_explicit_tangents() {
    let mut m = grid();
    m.set_tangent(1, 1, Axis::U, Tangent::Explicit(Vec2::new(50., 0.)))
        .unwrap();
    let resolved = m.resolved_tangent(1, 1, Axis::U);
    let nearer = m
        .point(1, 1)
        .distance(m.point(1, 0))
        .min(m.point(1, 1).distance(m.point(1, 2)));
    assert!((resolved.hypot() - 0.7 * nearer).abs() < 1e-12);
    assert!(
        m.set_tangent(0, 0, Axis::V, Tangent::Explicit(Vec2::new(f64::NAN, 0.)))
            .is_err()
    );
}

#[test]
fn hit_testing_finds_the_nearest_wire() {
    let m = grid();
    let edge = m.edge(2, 1, Axis::U);
    let on = edge.eval(0.4);
    let hit = m.hit_edge(on + Vec2::new(0., 0.002), 0.01).unwrap();
    assert_eq!((hit.row, hit.col, hit.axis), (2, 1, Axis::U));
    assert!((hit.t - 0.4).abs() < 0.05);
    assert!(m.hit_edge(Point::new(5., 5.), 0.01).is_none());
}

#[test]
fn construction_is_validated() {
    assert_eq!(
        MeshLattice::new(1, 4, vec![Point::ZERO; 4]).unwrap_err(),
        CurveError::Dimensions
    );
    assert_eq!(
        MeshLattice::new(2, 2, vec![Point::ZERO; 3]).unwrap_err(),
        CurveError::Dimensions
    );
    assert_eq!(monotone_channel_tangent(0.2, 0.6), 0.2);
    assert_eq!(monotone_channel_tangent(0.2, -0.6), 0.);
    assert_eq!(monotone_channel_tangent(-0.5, -0.3), -0.3);
}

#[test]
fn out_of_range_edits_are_errors() {
    let mut mesh = grid();
    let (rows, cols) = (mesh.rows(), mesh.cols());
    assert!(mesh.set_point(rows, 0, Point::ZERO).is_err());
    assert!(mesh.set_tangent(0, cols, Axis::U, Tangent::Auto).is_err());
    // The last column has no edge to its right; the last row none below.
    assert!(
        mesh.drag_edge(0, cols - 1, Axis::U, 0.5, Vec2::new(1., 1.))
            .is_err()
    );
    assert!(
        mesh.drag_edge(rows - 1, 0, Axis::V, 0.5, Vec2::new(1., 1.))
            .is_err()
    );
}
