use super::*;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn zigzag(kind: NodeMode) -> NodePath {
    let anchors = [p(0., 0.), p(1., 1.), p(2., 0.2), p(3., 1.4), p(4., 0.)];
    NodePath::new(
        anchors
            .iter()
            .map(|&a| Node {
                anchor: a,
                handle_in: None,
                handle_out: None,
                mode: kind,
            })
            .collect(),
        false,
    )
    .unwrap()
}

fn samples(path: &NodePath) -> Vec<Point> {
    let mut out = Vec::new();
    for i in 0..path.segment_count() {
        let seg = path.segment(i);
        for k in 0..=16 {
            out.push(seg.eval(k as f64 / 16.));
        }
    }
    out
}

fn on_path(path: &NodePath, q: Point) -> f64 {
    geom::nearest_on_path(&path.to_bezpath(), q)
        .unwrap()
        .distance
}

fn assert_g1(node: &Node) {
    let (a, b) = (
        node.handle_in.unwrap() - node.anchor,
        node.handle_out.unwrap() - node.anchor,
    );
    assert!(
        a.cross(b).abs() <= 1e-9 * (1. + a.hypot() * b.hypot()),
        "{node:?}"
    );
    assert!(a.dot(b) < 0., "{node:?}");
}

#[test]
fn corner_paths_are_polylines() {
    let path = zigzag(NodeMode::Corner);
    assert_eq!(path.segment_count(), 4);
    assert!((0..4).all(|i| matches!(path.segment(i), PathSeg::Line(_))));
    let mut closed = path.clone();
    closed.set_closed(true);
    assert_eq!(closed.segment_count(), 5);
    assert!(matches!(
        closed.to_bezpath().elements().last(),
        Some(geom::PathEl::ClosePath)
    ));
}

#[test]
fn smooth_and_symmetric_constraints_hold_through_edits() {
    for mode in [NodeMode::Smooth, NodeMode::Symmetric] {
        let mut path = zigzag(NodeMode::Auto(AutoSmooth::CatmullRom { alpha: 0.5 }));
        path.set_mode(2, mode).unwrap();
        path.move_handle(2, Side::Out, p(2.8, 0.9)).unwrap();
        assert_g1(&path.nodes()[2]);
        path.drag_segment(1, 0.4, Vec2::new(0.1, -0.2)).unwrap();
        assert_g1(&path.nodes()[2]);
        if mode == NodeMode::Symmetric {
            let n = path.nodes()[2];
            let (a, b) = (
                (n.handle_in.unwrap() - n.anchor).hypot(),
                (n.handle_out.unwrap() - n.anchor).hypot(),
            );
            assert!((a - b).abs() < 1e-12);
        }
    }
    // A corner's other handle is untouched.
    let mut path = zigzag(NodeMode::Auto(AutoSmooth::Natural));
    path.set_mode(2, NodeMode::Corner).unwrap();
    let before = path.nodes()[2].handle_in;
    path.move_handle(2, Side::Out, p(2.5, 2.)).unwrap();
    assert_eq!(path.nodes()[2].handle_in, before);
}

#[test]
fn segment_drag_follows_the_cursor() {
    for mode in [
        NodeMode::Corner,
        NodeMode::Smooth,
        NodeMode::Auto(AutoSmooth::Hobby),
    ] {
        let mut path = zigzag(mode);
        if mode == NodeMode::Smooth {
            path = zigzag(NodeMode::Auto(AutoSmooth::Natural));
            for i in 0..5 {
                path.set_mode(i, NodeMode::Smooth).unwrap();
            }
        }
        for t in [0.25, 0.5, 0.7] {
            let before = path.segment(1).eval(t);
            let delta = Vec2::new(-0.05, 0.12);
            path.drag_segment(1, t, delta).unwrap();
            assert!(
                (path.segment(1).eval(t) - (before + delta)).hypot() < 1e-12,
                "{mode:?} {t}"
            );
        }
    }
}

#[test]
fn insert_at_leaves_the_curve_unchanged() {
    for mode in [
        NodeMode::Corner,
        NodeMode::Auto(AutoSmooth::CatmullRom { alpha: 0.5 }),
        NodeMode::Auto(AutoSmooth::Natural),
        NodeMode::Auto(AutoSmooth::Hobby),
    ] {
        for closed in [false, true] {
            let mut path = zigzag(mode);
            path.set_closed(closed);
            let before = samples(&path);
            let index = path.insert_at(1, 0.3).unwrap();
            assert_eq!(index, 2);
            for q in &before {
                assert!(on_path(&path, *q) < 1e-9, "{mode:?} closed={closed}");
            }
            for q in samples(&path) {
                assert!(
                    on_path(
                        &{
                            let mut original = zigzag(mode);
                            original.set_closed(closed);
                            original
                        },
                        q
                    ) < 1e-9
                );
            }
            // A later automatic update does not move anything either.
            let frozen = samples(&path);
            path.update_auto();
            assert_eq!(samples(&path), frozen);
        }
    }
}

#[test]
fn natural_runs_are_c2() {
    for closed in [false, true] {
        let mut path = zigzag(NodeMode::Auto(AutoSmooth::Natural));
        path.set_closed(closed);
        let interior: Vec<usize> = if closed {
            (0..5).collect()
        } else {
            (1..4).collect()
        };
        for i in interior {
            let prev = if i == 0 { 4 } else { i - 1 };
            let (a, b) = (path.segment(prev), path.segment(i));
            assert!((geom::deriv1(a, 1.) - geom::deriv1(b, 0.)).hypot() < 1e-9);
            assert!(
                (geom::deriv2(a, 1.) - geom::deriv2(b, 0.)).hypot() < 1e-9,
                "closed={closed} node {i}"
            );
        }
    }
}

#[test]
fn hobby_is_smooth_and_rounds_a_square_into_a_circle() {
    let open = zigzag(NodeMode::Auto(AutoSmooth::Hobby));
    for i in 1..4 {
        assert_g1(&open.nodes()[i]);
    }
    let square = [p(1., 0.), p(0., 1.), p(-1., 0.), p(0., -1.)];
    let circle = NodePath::new(
        square
            .iter()
            .map(|&a| Node::auto(a, AutoSmooth::Hobby))
            .collect(),
        true,
    )
    .unwrap();
    for q in samples(&circle) {
        assert!((q.to_vec2().hypot() - 1.).abs() < 3e-3, "{q:?}");
    }
    // Handle length matches the classic 4-arc circle approximation (≈ 0.552).
    let h = (circle.nodes()[0].handle_out.unwrap() - square[0]).hypot();
    assert!((h - 0.5523).abs() < 0.02, "{h}");
}

#[test]
fn catmull_rom_passes_through_anchors_with_g1() {
    let path = zigzag(NodeMode::Auto(AutoSmooth::CatmullRom { alpha: 0.5 }));
    for i in 1..4 {
        assert_g1(&path.nodes()[i]);
        assert_eq!(path.segment(i).start(), path.nodes()[i].anchor);
    }
    // Open ends have no handle on the outside.
    assert!(path.nodes()[0].handle_in.is_none());
}

#[test]
fn moving_and_deleting_update_automatic_handles() {
    let mut path = zigzag(NodeMode::Auto(AutoSmooth::Natural));
    let before = path.nodes()[1];
    path.move_anchor(2, p(2., 1.5)).unwrap();
    assert_ne!(path.nodes()[1].handle_out, before.handle_out);
    path.delete_node(2).unwrap();
    assert_eq!(path.nodes().len(), 4);
    assert!(path.move_anchor(0, p(f64::NAN, 0.)).is_err());
    let comb = path.curvature_comb(4, 0.1);
    assert_eq!(comb.len(), 3 * 5);
}

#[test]
fn out_of_range_indices_are_errors_not_wraps() {
    let mut path = NodePath::new(
        vec![
            Node::corner(p(0., 0.)),
            Node::corner(p(50., 50.)),
            Node::corner(p(100., 0.)),
        ],
        false,
    )
    .unwrap();
    let before = path.clone();
    // An open path of three nodes has segments 0 and 1 only.
    assert!(path.drag_segment(2, 0.5, Vec2::new(0., 10.)).is_err());
    assert!(path.insert_at(2, 0.5).is_err());
    assert!(path.move_anchor(3, p(0., 0.)).is_err());
    assert!(path.move_handle(3, Side::In, p(0., 0.)).is_err());
    assert!(path.set_mode(3, NodeMode::Smooth).is_err());
    assert!(path.delete_node(3).is_err());
    assert_eq!(path, before);
}

#[test]
fn insert_keeps_constraints_and_missing_handles() {
    let mut sym = Node::corner(p(0., 0.));
    sym.handle_in = Some(p(-30., -30.));
    sym.handle_out = Some(p(30., 30.));
    sym.mode = NodeMode::Symmetric;
    let mut end = Node::corner(p(100., 0.));
    end.handle_in = Some(p(70., 30.));
    let mut path = NodePath::new(vec![sym, end], false).unwrap();
    let curve = samples(&path);
    path.insert_at(0, 0.5).unwrap();
    // The split shortens the outgoing handle, so the node is Smooth now.
    assert_eq!(path.nodes()[0].mode, NodeMode::Smooth);
    assert_g1(&path.nodes()[0]);
    for q in curve {
        assert!(on_path(&path, q) < 1e-6);
    }

    // A segment with only its end handle keeps no start handle after a split.
    let mut end = Node::corner(p(100., 0.));
    end.handle_in = Some(p(70., 30.));
    let mut path = NodePath::new(vec![Node::corner(p(0., 0.)), end], false).unwrap();
    let curve = samples(&path);
    path.insert_at(0, 0.5).unwrap();
    assert_eq!(path.nodes()[0].handle_out, None);
    for q in curve {
        assert!(on_path(&path, q) < 1e-6);
    }
}

#[test]
fn smooth_corners_get_handles() {
    let corners = || {
        NodePath::new(
            vec![
                Node::corner(p(0., 0.)),
                Node::corner(p(50., 50.)),
                Node::corner(p(100., 0.)),
            ],
            false,
        )
        .unwrap()
    };
    for mode in [NodeMode::Smooth, NodeMode::Symmetric] {
        let mut path = corners();
        path.set_mode(1, mode).unwrap();
        let node = path.nodes()[1];
        assert!(
            node.handle_in.is_some() && node.handle_out.is_some(),
            "{mode:?}"
        );
        assert_g1(&node);
        // An open end gets the handle on its segment's side. (Symmetric
        // mirrors it, as it always has; the mirror draws nothing.)
        path.set_mode(0, mode).unwrap();
        assert!(path.nodes()[0].handle_out.is_some());
        if mode == NodeMode::Smooth {
            assert_eq!(path.nodes()[0].handle_in, None);
        }
    }
    // Dragging one handle of a handleless Smooth node creates the other.
    let mut path = corners();
    let mut node = path.nodes()[1];
    node.mode = NodeMode::Smooth;
    path = NodePath::new(vec![path.nodes()[0], node, path.nodes()[2]], false).unwrap();
    path.move_handle(1, Side::Out, p(70., 50.)).unwrap();
    assert_g1(&path.nodes()[1]);
    assert!(path.nodes()[1].handle_in.is_some());
}
