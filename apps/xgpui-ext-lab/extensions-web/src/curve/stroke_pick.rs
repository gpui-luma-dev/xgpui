//! Compare the painted stroke with its bounds and pointer footprint.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use xgpui_curve::geom::{Affine, BezPath, FillRule, Point as KPoint, fill_contains};
use xgpui_curve::gpui_paint;
use xgpui_curve::stroke::{Cap, Join, StrokeStyle, stroke_outline};

pub struct StrokePanel {
    screen_width: bool,
    stretch: bool,
    dashed: bool,
    guides: bool,
    pointer: Option<Point<Pixels>>,
}

impl Default for StrokePanel {
    fn default() -> Self {
        Self {
            screen_width: false,
            stretch: true,
            dashed: true,
            guides: true,
            pointer: None,
        }
    }
}

fn connector() -> BezPath {
    let mut path = BezPath::new();
    path.move_to((95., 290.));
    path.curve_to((240., 295.), (210., 84.), (375., 115.));
    path.line_to((480., 270.));
    path.line_to((635., 145.));
    path
}

fn footprint(path: &BezPath, transform: Affine, screen_width: bool, dashed: bool) -> BezPath {
    let mut style = StrokeStyle::new(28.);
    style.start_cap = Cap::Square;
    style.end_cap = Cap::Round;
    style.join = Join::Miter(4.);
    if dashed {
        style.dash_pattern = vec![95., 40.];
    }
    if screen_width {
        stroke_outline(&(transform * path), &style, 0.1)
    } else {
        transform * stroke_outline(path, &style, 0.1)
    }
}

pub fn render(lab: &CurveLab, _window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let StrokePanel {
        screen_width,
        stretch,
        dashed,
        guides,
        pointer,
    } = lab.stroke;
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(button(if screen_width { "Width: logical pixels" } else { "Width: model units" }, screen_width)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.stroke.screen_width = !this.stroke.screen_width;
                        cx.notify();
                    })))
                .child(button("Nonuniform scale", stretch).on_click(cx.listener(|this, _, _, cx| {
                    this.stroke.stretch = !this.stroke.stretch;
                    cx.notify();
                })))
                .child(button("Dashes", dashed).on_click(cx.listener(|this, _, _, cx| {
                    this.stroke.dashed = !this.stroke.dashed;
                    cx.notify();
                })))
                .child(button("Bounds and centerline", guides).on_click(cx.listener(|this, _, _, cx| {
                    this.stroke.guides = !this.stroke.guides;
                    cx.notify();
                }))),
        )
        .child(div().text_color(rgb(0x9fb0cc)).child(
            "The stroke turns green on a hit. The red pointer dot marks a miss. Selection slop is zero.",
        ))
        .child(
            div()
                .id("stroke-picking-canvas")
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .rounded_lg()
                .bg(rgb(0x151d2e))
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if !hovered {
                        this.stroke.pointer = None;
                        cx.notify();
                    }
                }))
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    this.stroke.pointer = Some(event.position);
                    cx.notify();
                }))
                .child(canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let transform = fit(bounds, 740., 420.)
                            * Affine::scale_non_uniform(1., if stretch { 0.72 } else { 1. });
                        let path = connector();
                        let outline = footprint(&path, transform, screen_width, dashed);
                        let pointer = pointer.filter(|p| bounds.contains(p)).map(kp);
                        let hit = pointer.is_some_and(|p| fill_contains(&outline, p, FillRule::NonZero));

                        let mut builder = PathBuilder::fill().with_style(PathStyle::Fill(
                            FillOptions::default().with_fill_rule(gpui::FillRule::NonZero),
                        ));
                        gpui_paint::append(&mut builder, &outline, Affine::IDENTITY);
                        match builder.build() {
                            Ok(path) => window.paint_path(path, rgb(if hit { 0x4dd2a8 } else { 0x5aa9ff })),
                            Err(_) => return,
                        }

                        if guides {
                            let b = outline.bounding_box();
                            let mut frame = BezPath::new();
                            frame.move_to((b.x0, b.y0));
                            frame.line_to((b.x1, b.y0));
                            frame.line_to((b.x1, b.y1));
                            frame.line_to((b.x0, b.y1));
                            frame.close_path();
                            stroke(window, &frame, Affine::IDENTITY, 1., rgb(0xffd166));
                            stroke(window, &path, transform, 1., rgb(0x9fb0cc));
                        }
                        if let Some(p) = pointer {
                            handle(window, p, 4., rgb(if hit { 0x4dd2a8 } else { 0xff6b6b }));
                        }
                    },
                ).size_full()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn dashed_footprint_preserves_gaps_in_both_width_modes() {
        let line = line(KPoint::new(0., 0.), KPoint::new(400., 0.));
        let transform = Affine::scale_non_uniform(1., 0.5);
        for screen_width in [false, true] {
            let outline = footprint(&line, transform, screen_width, true);
            let contains = |x, y| fill_contains(&outline, KPoint::new(x, y), FillRule::NonZero);
            assert!(contains(40., 0.));
            assert!(!contains(115., 0.));
            assert_eq!(contains(40., 10.), screen_width);
        }
    }
}
