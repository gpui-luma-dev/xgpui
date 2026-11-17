//! Drawing and interaction helpers shared by the curve panels.
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc};
use xgpui_curve::gpui_paint;
use xgpui_curve::kurbo::{Affine, BezPath, Point as KPoint};

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

/// Pointer handlers on a canvas area: `down`, `drag` (while the left button
/// is held) and `up`, each receiving the window position.
pub fn interactive<V: 'static>(
    id: impl Into<ElementId>,
    cx: &mut Context<V>,
    down: impl Fn(&mut V, Point<Pixels>, usize, &mut Context<V>) + 'static,
    drag: impl Fn(&mut V, Point<Pixels>, &mut Context<V>) + 'static,
    up: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .size_full()
        .relative()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                down(this, e.position, e.click_count, cx);
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
            if e.pressed_button == Some(MouseButton::Left) {
                drag(this, e.position, cx);
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                up(this, cx);
                cx.notify();
            }),
        )
}

/// Record `bounds` into `area` during prepaint.
pub fn record(area: &Area) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    let area = area.clone();
    move |bounds, _, _| area.set(bounds)
}
