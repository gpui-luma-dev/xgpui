//! Shapes and strokes: squircle corner smoothing against circular corners,
//! padded arc sectors with analytic hit testing, and freehand drawing fitted
//! with Schneider's algorithm and outlined as a tapered stroke.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use std::f64::consts::TAU;
use xgpui_curve::fit::{FitOptions, fit_cubic_path};
use xgpui_curve::geom::{BezPath, Point as KPoint, Rect, rounded_rect_path};
use xgpui_curve::shape::{ArcShape, Squircle};
use xgpui_curve::stroke::{Cap, StrokeProfile};
use xgpui_curve::svg_path;

const SMOOTHING: [f64; 4] = [0., 0.3, 0.6, 1.];
const SECTORS: [(f64, u32); 4] = [
    (0.32, 0xff6b6b),
    (0.18, 0xffd166),
    (0.28, 0x4dd2a8),
    (0.22, 0x5aa9ff),
];
const DRAW_W: f64 = 640.;
const DRAW_H: f64 = 360.;

pub struct ShapesPanel {
    smoothing: usize,
    radius: f64,
    raw: Vec<KPoint>,
    fitted: Option<BezPath>,
    drawing: bool,
    taper: bool,
    hover_sector: Option<usize>,
    draw_area: Area,
    view: Camera,
    arc_area: Area,
}

impl ShapesPanel {
    pub fn new() -> Self {
        let mut panel = Self {
            smoothing: 2,
            radius: 48.,
            raw: Vec::new(),
            fitted: None,
            drawing: false,
            taper: true,
            hover_sector: None,
            draw_area: area(),
            view: Camera::default(),
            arc_area: area(),
        };
        // A starting scribble so the stroke is visible before drawing.
        panel.raw = (0..120)
            .map(|i| {
                let t = i as f64 / 119.;
                KPoint::new(
                    60. + 520. * t,
                    180. + 110. * (t * TAU * 1.3).sin() * (1. - 0.5 * t),
                )
            })
            .collect();
        panel.refit();
        panel
    }

    fn refit(&mut self) {
        self.fitted = fit_cubic_path(
            &self.raw,
            FitOptions {
                tolerance: 2.,
                ..Default::default()
            },
        )
        .ok();
    }
}

fn sectors(center: KPoint) -> Vec<ArcShape> {
    let mut start = 0.;
    SECTORS
        .iter()
        .map(|&(share, _)| {
            let end = start + share * TAU;
            let s = ArcShape {
                pad_angle: 0.04,
                ..ArcShape::new(center, 50., 95., start, end)
            };
            start = end;
            s
        })
        .collect()
}

pub fn render(lab: &mut CurveLab, _window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let panel = &lab.shapes;
    let smoothing = SMOOTHING[panel.smoothing];
    let radius = panel.radius;
    let fitted = panel.fitted.clone();
    let raw = panel.raw.clone();
    let taper = panel.taper;
    let view = panel.view;
    let hover = panel.hover_sector;
    let profile = if taper {
        StrokeProfile::tapered(18., 0.25, 0.3)
    } else {
        StrokeProfile::constant(10.)
    }
    .map(|p| p.caps(Cap::Round));
    let outline = fitted
        .as_ref()
        .zip(profile.ok())
        .and_then(|(f, p)| p.outline(f, 0.25).ok());
    let segments = fitted.as_ref().map_or(0, |f| f.segments().count());
    let svg = fitted.as_ref().map(svg_path::to_svg).unwrap_or_default();
    let svg_preview: String = svg.chars().take(160).collect();

    let controls = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(
            button(format!("Corner smoothing {smoothing:.1}"), smoothing > 0.).on_click(
                cx.listener(|this, _, _, cx| {
                    this.shapes.smoothing = (this.shapes.smoothing + 1) % SMOOTHING.len();
                    cx.notify();
                }),
            ),
        )
        .child(
            button(format!("Radius {radius:.0}"), false).on_click(cx.listener(|this, _, _, cx| {
                this.shapes.radius = match this.shapes.radius as u32 {
                    24 => 48.,
                    48 => 80.,
                    80 => 140.,
                    _ => 24.,
                };
                cx.notify();
            })),
        )
        .child(
            button(
                if taper {
                    "Stroke: tapered"
                } else {
                    "Stroke: constant"
                },
                taper,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.shapes.taper = !this.shapes.taper;
                cx.notify();
            })),
        )
        .child(
            button("Clear drawing", false).on_click(cx.listener(|this, _, _, cx| {
                this.shapes.raw.clear();
                this.shapes.fitted = None;
                cx.notify();
            })),
        )
        .child(view_button(cx, view, |lab: &mut CurveLab| {
            &mut lab.shapes.view
        }));

    let squircle = card("Squircle (fill) vs circular corners (outline)").child(
        div().w(px(300.)).h(px(220.)).child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let t = fit(bounds, 300., 220.);
                    let rect = Rect::new(20., 20., 280., 200.);
                    if let Ok(s) = Squircle::new(rect, radius, smoothing) {
                        filled(window, &s.to_path(0.05), t, rgb(0x3a6bc4));
                    }
                    let round = rounded_rect_path(rect, radius.min(90.), 0.05);
                    stroke(window, &round, t, 1.5, hsla(0.12, 1., 0.6, 0.9));
                },
            )
            .size_full(),
        ),
    );

    let donut = card(match hover {
        Some(i) => format!(
            "Arc sectors · hovering sector {} ({:.0}%)",
            i + 1,
            SECTORS[i].0 * 100.
        ),
        None => "Arc sectors · hover to hit-test analytically".into(),
    })
    .child(
        div()
            .id("arc-sectors")
            .w(px(220.))
            .h(px(220.))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                let t = fit(this.shapes.arc_area.get(), 220., 220.);
                let m = to_model(t, e.position);
                this.shapes.hover_sector = sectors(KPoint::new(110., 110.))
                    .iter()
                    .position(|s| s.contains(m));
                cx.notify();
            }))
            .child(
                canvas(record(&panel.arc_area), move |bounds, _, window, _| {
                    let t = fit(bounds, 220., 220.);
                    for (i, s) in sectors(KPoint::new(110., 110.)).iter().enumerate() {
                        let lift = if hover == Some(i) {
                            ArcShape {
                                outer_radius: 104.,
                                ..*s
                            }
                        } else {
                            *s
                        };
                        filled(window, &lift.to_path(0.05), t, rgb(SECTORS[i].1));
                    }
                })
                .size_full(),
            ),
    );

    let draw = card(format!(
        "Draw here · {} input points → {segments} cubic segments",
        raw.len()
    ))
    .child(
        div().w(px(640.)).h(px(360.)).child(
            interactive(
                "freehand",
                cx,
                &panel.draw_area,
                |lab: &mut CurveLab| &mut lab.shapes.view,
                |this: &mut CurveLab, at, _, _| {
                    let b = this.shapes.draw_area.get();
                    let m = to_model(this.shapes.view.around(b) * fit(b, DRAW_W, DRAW_H), at);
                    this.shapes.raw = vec![m];
                    this.shapes.fitted = None;
                    this.shapes.drawing = true;
                    true
                },
                |this: &mut CurveLab, at, _| {
                    if this.shapes.drawing {
                        let b = this.shapes.draw_area.get();
                        let m = to_model(this.shapes.view.around(b) * fit(b, DRAW_W, DRAW_H), at);
                        this.shapes.raw.push(m);
                    }
                },
                |this: &mut CurveLab, _| {
                    if this.shapes.drawing {
                        this.shapes.drawing = false;
                        this.shapes.refit();
                    }
                },
            )
            .rounded_md()
            .overflow_hidden()
            .bg(rgb(0x0c1220))
            .child(
                canvas(record(&panel.draw_area), move |bounds, _, window, _| {
                    let t = view.around(bounds) * fit(bounds, DRAW_W, DRAW_H);
                    if let Some(o) = &outline {
                        filled(window, o, t, rgb(0xe8eefc));
                    }
                    if let Some(f) = &fitted {
                        stroke(window, f, t, 1., hsla(0.58, 0.9, 0.55, 0.9));
                    }
                    for p in &raw {
                        dot(window, t * *p, 1.5, hsla(0.08, 0.95, 0.6, 0.8));
                    }
                })
                .size_full(),
            ),
        ),
    );

    div()
        .id("shapes-scroll")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .child(controls)
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_3()
                .child(squircle)
                .child(donut),
        )
        .child(draw)
        .child(div().text_color(rgb(0x9fb0cc)).child(format!(
            "SVG d: {svg_preview}{}",
            if svg.len() > 160 { "…" } else { "" }
        )))
        .into_any_element()
}
