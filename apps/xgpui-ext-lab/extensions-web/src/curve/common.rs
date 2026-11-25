//! Drawing and interaction helpers shared by the curve panels.
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc};
use xgpui_curve::geom::{Affine, BezPath, Point as KPoint};
use xgpui_curve::gpui_paint;

/// Bounds of an interactive canvas, recorded during prepaint.
pub type Area = Rc<Cell<Bounds<Pixels>>>;

pub fn area() -> Area {
    Rc::new(Cell::new(Bounds::default()))
}

/// A toggle-style button; `active` highlights it.
pub fn button(label: impl Into<SharedString>, active: bool) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(ElementId::Name(label.clone()))
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(if active { 0x2f5aa8 } else { 0x1d2940 }))
        .hover(|s| s.bg(rgb(0x3a6bc4)))
        .child(label)
}

/// Uniformly fit a `w × h` design space into `bounds`, centered.
pub fn fit(bounds: Bounds<Pixels>, w: f64, h: f64) -> Affine {
    let (bx, by) = (
        f64::from(f32::from(bounds.origin.x)),
        f64::from(f32::from(bounds.origin.y)),
    );
    let (bw, bh) = (
        f64::from(f32::from(bounds.size.width)),
        f64::from(f32::from(bounds.size.height)),
    );
    let scale = (bw / w).min(bh / h).max(1e-6);
    let (ox, oy) = (bx + (bw - w * scale) / 2., by + (bh - h * scale) / 2.);
    Affine::new([scale, 0., 0., scale, ox, oy])
}

/// Map `0..1` coordinates onto `bounds` (non-uniform).
pub fn unit(bounds: Bounds<Pixels>) -> Affine {
    gpui_paint::bounds_transform(bounds)
}

pub fn kp(p: Point<Pixels>) -> KPoint {
    KPoint::new(f64::from(f32::from(p.x)), f64::from(f32::from(p.y)))
}

pub fn gp(p: KPoint) -> Point<Pixels> {
    point(px(p.x as f32), px(p.y as f32))
}

/// Model-space position of a window point under `transform`.
pub fn to_model(transform: Affine, p: Point<Pixels>) -> KPoint {
    transform.inverse() * kp(p)
}

pub fn dot(window: &mut Window, center: KPoint, radius: f32, color: impl Into<Background>) {
    let r = px(radius);
    window.paint_quad(
        fill(Bounds::centered_at(gp(center), size(r * 2., r * 2.)), color).corner_radii(r),
    );
}

/// A node marker with a dark ring, readable on any background.
pub fn handle(window: &mut Window, center: KPoint, radius: f32, color: impl Into<Background>) {
    dot(window, center, radius + 2., hsla(0., 0., 0., 0.75));
    dot(window, center, radius, color);
}

pub fn stroke(
    window: &mut Window,
    path: &BezPath,
    transform: Affine,
    width: f32,
    color: impl Into<Background>,
) {
    if let Some(p) = gpui_paint::stroke_path(path, transform, width) {
        window.paint_path(p, color);
    }
}

pub fn filled(
    window: &mut Window,
    path: &BezPath,
    transform: Affine,
    color: impl Into<Background>,
) {
    if let Some(p) = gpui_paint::fill_path(path, transform) {
        window.paint_path(p, color);
    }
}

/// Dark halo plus light line.
pub fn contrast(window: &mut Window, path: &BezPath, transform: Affine, width: f32) {
    gpui_paint::paint_contrasting_path(
        window,
        path,
        transform,
        (width + 2.5, hsla(0., 0., 0., 0.6)),
        (width, hsla(0., 0., 1., 0.92)),
    );
}

pub fn line(a: KPoint, b: KPoint) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(a);
    p.line_to(b);
    p
}

/// A dark card with a caption, holding an interactive canvas.
pub fn card(caption: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .rounded_lg()
        .bg(rgb(0x151d2e))
        .child(div().text_color(rgb(0xb8c6de)).child(caption.into()))
}

/// Smallest and largest zoom a [`Camera`] allows.
pub const ZOOM_RANGE: (f64, f64) = (0.25, 32.);

/// A canvas camera: pan and zoom applied on top of a panel's base transform,
/// in canvas-local pixels (so it survives the canvas moving or resizing).
#[derive(Clone, Copy, Debug, Default)]
pub struct Camera {
    camera: xgpui_canvas::Camera2D,
    pan_from: Option<KPoint>,
}

impl Camera {
    /// Current zoom factor.
    pub fn zoom(&self) -> f64 {
        self.camera.zoom()
    }

    /// Whether the view differs from the default.
    pub fn is_moved(&self) -> bool {
        self.camera.zoom() != 1. || self.camera.center() != xgpui_canvas::Point::default()
    }

    /// The camera as a window-space transform for a canvas at `bounds`.
    pub fn around(&self, bounds: Bounds<Pixels>) -> Affine {
        let o = kp(bounds.origin).to_vec2();
        let transform = Affine::new(
            self.camera
                .world_to_screen_affine(xgpui_canvas::Point::default())
                .expect("curve camera retains representable affine coefficients"),
        );
        Affine::translate(o) * transform * Affine::translate(-o)
    }

    /// Zoom by `factor` keeping the window point `at` fixed.
    pub fn zoom_at(&mut self, bounds: Bounds<Pixels>, at: Point<Pixels>, factor: f64) {
        if !factor.is_finite() || factor <= 0. {
            return;
        }
        let local = kp(at) - kp(bounds.origin);
        let mut next = self.camera;
        next.zoom_at_with_limits(
            xgpui_canvas::Point::new(local.x, local.y),
            xgpui_canvas::Point::default(),
            factor,
            ZOOM_RANGE.0,
            ZOOM_RANGE.1,
        );
        if next
            .world_to_screen_affine(xgpui_canvas::Point::default())
            .is_some()
        {
            self.camera = next;
        }
    }

    /// Move the content by a window-space delta.
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        let mut next = self.camera;
        next.pan_by(xgpui_canvas::Point::new(dx, dy));
        if next
            .world_to_screen_affine(xgpui_canvas::Point::default())
            .is_some()
        {
            self.camera = next;
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A canvas area with editing and navigation:
///
/// - left press calls `down`; if it returns `false` (nothing was hit) the
///   drag pans instead of calling `drag`;
/// - middle or right drag always pans;
/// - mouse wheel zooms at the pointer; trackpad two-finger scroll pans;
///   pinch, or Ctrl/⌘ with any scroll (how browsers report trackpad
///   pinches), zooms at the pointer.
pub fn interactive<V: 'static>(
    id: impl Into<ElementId>,
    cx: &mut Context<V>,
    area: &Area,
    view: fn(&mut V) -> &mut Camera,
    down: impl Fn(&mut V, Point<Pixels>, usize, &mut Context<V>) -> bool + 'static,
    drag: impl Fn(&mut V, Point<Pixels>, &mut Context<V>) + 'static,
    up: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Stateful<Div> {
    let start_pan = move |this: &mut V, at: Point<Pixels>| view(this).pan_from = Some(kp(at));
    let end = move |this: &mut V| view(this).pan_from = None;
    let (scroll_area, pinch_area) = (area.clone(), area.clone());
    div()
        .id(id.into())
        .size_full()
        .relative()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                if !down(this, e.position, e.click_count, cx) {
                    start_pan(this, e.position);
                }
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, e: &MouseDownEvent, _, _| start_pan(this, e.position)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, _, _| start_pan(this, e.position)),
        )
        .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
            let v = view(this);
            if let Some(from) = v.pan_from {
                if e.pressed_button.is_none() {
                    v.pan_from = None;
                    return;
                }
                let to = kp(e.position);
                v.pan_by(to.x - from.x, to.y - from.y);
                v.pan_from = Some(to);
                cx.notify();
            } else if e.pressed_button == Some(MouseButton::Left) {
                drag(this, e.position, cx);
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                end(this);
                up(this, cx);
                cx.notify();
            }),
        )
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener(move |this, _: &MouseUpEvent, _, _| end(this)),
        )
        .on_mouse_up(
            MouseButton::Right,
            cx.listener(move |this, _: &MouseUpEvent, _, _| end(this)),
        )
        .on_scroll(cx.listener(move |this, e: &ScrollEvent, _, cx| {
            let delta = e.delta.pixel_delta(px(24.));
            let (dx, dy) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
            let v = view(this);
            if e.modifiers.control || e.modifiers.platform || !e.delta.precise() {
                v.zoom_at(
                    scroll_area.get(),
                    e.position,
                    (dy * 0.01).clamp(-1., 1.).exp(),
                );
            } else {
                v.pan_by(dx, dy);
            }
            cx.stop_propagation();
            cx.notify();
        }))
        .on_pinch(cx.listener(move |this, e: &PinchEvent, _, cx| {
            view(this).zoom_at(
                pinch_area.get(),
                e.position,
                (1. + f64::from(e.delta)).max(0.01),
            );
            cx.stop_propagation();
            cx.notify();
        }))
}

/// A "Reset view" button showing the current zoom.
pub fn view_button<V: 'static>(
    cx: &mut Context<V>,
    current: Camera,
    view: fn(&mut V) -> &mut Camera,
) -> Stateful<Div> {
    button(
        format!("Reset view ({:.0}%)", current.zoom() * 100.),
        current.is_moved(),
    )
    .on_click(cx.listener(move |this, _, _, cx| {
        view(this).reset();
        cx.notify();
    }))
}

/// Navigation help shown under each zoomable canvas.
pub const NAV_HINT: &str =
    "Wheel or pinch to zoom · two-finger scroll, right/middle drag or drag on empty space to pan";

/// Record `bounds` into `area` during prepaint.
pub fn record(area: &Area) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    let area = area.clone();
    move |bounds, _, _| area.set(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(40.), px(30.)), size(px(800.), px(500.)))
    }

    #[::core::prelude::v1::test]
    fn zoom_keeps_the_anchor_fixed_and_clamps() {
        let b = bounds();
        let mut camera = Camera::default();
        let anchor = point(px(300.), px(200.));
        let before = camera.around(b).inverse() * kp(anchor);
        camera.zoom_at(b, anchor, 2.5);
        assert!((camera.zoom() - 2.5).abs() < 1e-12);
        // The content point under the pointer stays under the pointer.
        assert!((camera.around(b) * before - kp(anchor)).hypot() < 1e-9);
        camera.zoom_at(b, anchor, 1000.);
        assert!((camera.zoom() - ZOOM_RANGE.1).abs() < 1e-9);
        camera.zoom_at(b, anchor, 1e-9);
        assert!((camera.zoom() - ZOOM_RANGE.0).abs() < 1e-9);
        camera.zoom_at(b, anchor, f64::NAN);
        assert!((camera.zoom() - ZOOM_RANGE.0).abs() < 1e-9);
    }

    #[::core::prelude::v1::test]
    fn pan_moves_content_and_survives_the_canvas_moving() {
        let b = bounds();
        let mut camera = Camera::default();
        assert!(!camera.is_moved());
        camera.pan_by(25., -10.);
        camera.zoom_at(b, point(px(100.), px(100.)), 2.);
        let p = KPoint::new(10., 20.);
        let at = camera.around(b) * (p + kp(b.origin).to_vec2());
        // Move the canvas: the content moves with it.
        let moved = Bounds::new(point(px(140.), px(80.)), b.size);
        let at_moved = camera.around(moved) * (p + kp(moved.origin).to_vec2());
        assert!(((at_moved - at) - xgpui_curve::geom::Vec2::new(100., 50.)).hypot() < 1e-9);
        camera.reset();
        assert!(!camera.is_moved());
    }
}
