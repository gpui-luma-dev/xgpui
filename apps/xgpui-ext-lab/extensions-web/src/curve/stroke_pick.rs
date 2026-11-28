//! Compare the painted stroke with its bounds and pointer footprint.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use xgpui_curve::geom::{Affine, BezPath};
use xgpui_curve::gpui_paint;
use xgpui_curve::stroke::{Cap, Join, StrokeFootprint, StrokeProfile, StrokeSpace, StrokeStyle};

pub struct StrokePanel {
    screen_width: bool,
    stretch: bool,
    dashed: bool,
    variable_width: bool,
    guides: bool,
    pointer: Option<Point<Pixels>>,
}

impl Default for StrokePanel {
    fn default() -> Self {
        Self {
            screen_width: false,
            stretch: true,
            dashed: true,
            variable_width: false,
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

fn footprint(
    path: &BezPath,
    transform: Affine,
    screen_width: bool,
    dashed: bool,
    variable_width: bool,
) -> Result<StrokeFootprint, xgpui_curve::CurveError> {
    let mut style = StrokeStyle::new(28.);
    style.start_cap = Cap::Square;
    style.end_cap = Cap::Round;
    style.join = Join::Miter(4.);
    if dashed {
        style.dash_pattern = vec![95., 40.];
    }
    let space = if screen_width {
        StrokeSpace::LogicalPixels
    } else {
        StrokeSpace::Model
    };
    if variable_width {
        let mut profile = StrokeProfile::new(vec![(0., 8.), (0.6, 36.), (1., 18.)])?;
        profile.start_cap = style.start_cap;
        profile.end_cap = style.end_cap;
        profile.join = style.join;
        StrokeFootprint::from_dashed_profile(
            path,
            &profile,
            &style.dash_pattern,
            0.,
            transform,
            space,
            0.1,
        )
    } else {
        StrokeFootprint::new(path, &style, transform, space, 0.1)
    }
}

pub fn render(lab: &CurveLab, _window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let StrokePanel {
        screen_width,
        stretch,
        dashed,
        variable_width,
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
                .child(button("Variable width", variable_width).on_click(cx.listener(|this, _, _, cx| {
                    this.stroke.variable_width = !this.stroke.variable_width;
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
                        let footprint = match footprint(&path, transform, screen_width, dashed, variable_width) {
                            Ok(footprint) => footprint,
                            Err(error) => {
                                log::error!("Could not build stroke footprint: {error}");
                                return;
                            }
                        };
                        let pointer = pointer.filter(|p| bounds.contains(p)).map(kp);
                        let hit = pointer.is_some_and(|p| footprint.contains(p, 0.));

                        match gpui_paint::stroke_footprint_path(&footprint) {
                            Ok(Some(path)) => window.paint_path(path, rgb(if hit { 0x4dd2a8 } else { 0x5aa9ff })),
                            Ok(None) => {}
                            Err(error) => {
                                log::error!("Could not tessellate stroke footprint: {error}");
                                return;
                            }
                        }

                        if guides {
                            if let Some(b) = footprint.bounds() {
                                let mut frame = BezPath::new();
                                frame.move_to((b.x0, b.y0));
                                frame.line_to((b.x1, b.y0));
                                frame.line_to((b.x1, b.y1));
                                frame.line_to((b.x0, b.y1));
                                frame.close_path();
                                stroke(window, &frame, Affine::IDENTITY, 1., rgb(0xffd166));
                            }
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
    use xgpui_curve::geom::Point as KPoint;

    #[::core::prelude::v1::test]
    fn dashed_footprint_preserves_gaps_in_both_width_modes() {
        let line = line(KPoint::new(0., 0.), KPoint::new(400., 0.));
        let transform = Affine::scale_non_uniform(1., 0.5);
        for screen_width in [false, true] {
            let footprint = footprint(&line, transform, screen_width, true, false).unwrap();
            let painted = gpui_paint::stroke_footprint_path(&footprint)
                .unwrap()
                .unwrap();
            assert!(!painted.vertices.is_empty());
            let bounds = footprint.bounds().unwrap();
            assert_eq!(f32::from(painted.bounds.origin.x), bounds.x0 as f32);
            assert_eq!(f32::from(painted.bounds.origin.y), bounds.y0 as f32);
            let contains = |x, y| footprint.contains(KPoint::new(x, y), 0.);
            assert!(contains(40., 0.));
            assert!(!contains(115., 0.));
            assert_eq!(contains(40., 10.), screen_width);
        }
    }

    #[::core::prelude::v1::test]
    fn dashed_variable_width_connector_tessellates_in_both_width_modes() {
        let path = connector();
        let transform = Affine::scale_non_uniform(1., 0.72);
        for screen_width in [false, true] {
            let footprint = footprint(&path, transform, screen_width, true, true).unwrap();
            let painted = gpui_paint::stroke_footprint_path(&footprint)
                .unwrap()
                .unwrap();
            assert!(!painted.vertices.is_empty());
            assert!(footprint.contains(transform * KPoint::new(95., 290.), 0.));
        }
    }
}
