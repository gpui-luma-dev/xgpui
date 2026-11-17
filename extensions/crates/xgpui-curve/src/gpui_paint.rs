//! GPUI adapters: turn kurbo paths into `gpui::Path<Pixels>` with native
//! `move_to` / `line_to` / `curve_to` / `cubic_bezier_to` commands (no CPU
//! rasterization), and paint high-contrast gizmos.
//!
//! Path coordinates are window pixels; pass an [`Affine`] to map from another
//! space (for example normalized `0..1` coordinates into element bounds with
//! [`bounds_transform`]).

use gpui::{Background, Bounds, Path, PathBuilder, Pixels, Window, point, px};

use crate::geom::{Affine, BezPath, PathEl, Point};

fn pt(p: Point) -> gpui::Point<Pixels> {
    point(px(p.x as f32), px(p.y as f32))
}

/// Feed `path` (mapped by `transform`) into a GPUI path builder.
pub fn append(builder: &mut PathBuilder, path: &BezPath, transform: Affine) {
    for el in path.elements() {
        match transform * *el {
            PathEl::MoveTo(p) => builder.move_to(pt(p)),
            PathEl::LineTo(p) => builder.line_to(pt(p)),
            PathEl::QuadTo(c, p) => builder.curve_to(pt(p), pt(c)),
            PathEl::CurveTo(c1, c2, p) => builder.cubic_bezier_to(pt(p), pt(c1), pt(c2)),
            PathEl::ClosePath => builder.close(),
        }
    }
}

/// A filled GPUI path, or `None` if tessellation fails or the path is empty.
pub fn fill_path(path: &BezPath, transform: Affine) -> Option<Path<Pixels>> {
    if path.elements().is_empty() {
        return None;
    }
    let mut builder = PathBuilder::fill();
    append(&mut builder, path, transform);
    builder.build().ok()
}

/// A stroked GPUI path of `width` pixels.
pub fn stroke_path(path: &BezPath, transform: Affine, width: f32) -> Option<Path<Pixels>> {
    if path.elements().is_empty() || !(width.is_finite() && width > 0.) {
        return None;
    }
    let mut builder = PathBuilder::stroke(px(width));
    append(&mut builder, path, transform);
    builder.build().ok()
}

/// Map normalized `0..1` coordinates onto `bounds`.
pub fn bounds_transform(bounds: Bounds<Pixels>) -> Affine {
    let (x, y) = (
        f64::from(f32::from(bounds.origin.x)),
        f64::from(f32::from(bounds.origin.y)),
    );
    let (w, h) = (
        f64::from(f32::from(bounds.size.width)),
        f64::from(f32::from(bounds.size.height)),
    );
    Affine::new([w, 0., 0., h, x, y])
}

/// Paint `path` twice — a wide `outer` halo, then a narrow `inner` stroke —
/// so wireframes and gizmos stay legible over any background.
pub fn paint_contrasting_path(
    window: &mut Window,
    path: &BezPath,
    transform: Affine,
    outer: (f32, impl Into<Background>),
    inner: (f32, impl Into<Background>),
) {
    if let Some(halo) = stroke_path(path, transform, outer.0) {
        window.paint_path(halo, outer.1);
    }
    if let Some(line) = stroke_path(path, transform, inner.0) {
        window.paint_path(line, inner.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_tessellate_through_gpui() {
        let mut path = BezPath::new();
        path.move_to((0., 0.));
        path.line_to((1., 0.));
        path.quad_to((1., 1.), (0.5, 1.));
        path.curve_to((0.2, 1.), (0., 0.8), (0., 0.));
        path.close_path();
        let bounds = Bounds::new(point(px(10.), px(20.)), gpui::size(px(100.), px(50.)));
        let t = bounds_transform(bounds);
        assert_eq!(t * Point::new(1., 1.), Point::new(110., 70.));
        assert!(fill_path(&path, t).is_some());
        assert!(stroke_path(&path, t, 2.).is_some());
        assert!(stroke_path(&path, t, 0.).is_none());
        assert!(fill_path(&BezPath::new(), t).is_none());
    }
}
