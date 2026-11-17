//! Functional curves: a draggable tone curve (monotone, linear or clamped
//! Bézier) with its baked LUT, and animated CSS easing functions.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use web_time::Instant;
use xgpui_curve::func::{Easing, FunctionalCurve, LinearStop, StepPosition};
use xgpui_curve::kurbo::{Affine, BezPath, Point as KPoint};

const S: f64 = 300.;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Monotone,
    Linear,
    Bezier,
}

pub struct FuncPanel {
    /// Knots in 0..1, x increasing.
    knots: Vec<KPoint>,
    kind: Kind,
    drag: Option<usize>,
    area: Area,
    view: Camera,
    started: Instant,
    animate: bool,
}

impl FuncPanel {
    pub fn new() -> Self {
        Self {
            knots: vec![
                KPoint::new(0., 0.05),
                KPoint::new(0.25, 0.18),
                KPoint::new(0.5, 0.62),
                KPoint::new(0.78, 0.8),
                KPoint::new(1., 0.97),
            ],
            kind: Kind::Monotone,
            drag: None,
            area: area(),
            view: Camera::default(),
            started: Instant::now(),
            animate: true,
        }
    }

    fn curve(&self) -> Option<FunctionalCurve> {
        match self.kind {
            Kind::Monotone => FunctionalCurve::monotone(&self.knots).ok(),
            Kind::Linear => FunctionalCurve::linear(&self.knots).ok(),
            Kind::Bezier => {
                // Handles at thirds, aimed along the neighbors' chord (Catmull-Rom style).
                let k = &self.knots;
                let slope = |i: usize| {
                    let (a, b) = (k[i.saturating_sub(1)], k[(i + 1).min(k.len() - 1)]);
                    (b.y - a.y) / (b.x - a.x).max(1e-9)
                };
                let handles: Vec<_> = (0..k.len() - 1)
                    .map(|i| {
                        let h = (k[i + 1].x - k[i].x) / 3.;
                        (
                            KPoint::new(k[i].x + h, k[i].y + h * slope(i)),
                            KPoint::new(k[i + 1].x - h, k[i + 1].y - h * slope(i + 1)),
                        )
                    })
                    .collect();
                FunctionalCurve::bezier(k, &handles).ok()
            }
        }
    }

    /// Model is y-up in 0..1; the design square is y-down.
    fn transform(view: Camera, bounds: Bounds<Pixels>) -> Affine {
        view.around(bounds) * fit(bounds, S, S) * Affine::new([S, 0., 0., -S, 0., S])
    }
}

fn plot(f: impl Fn(f64) -> f64, lo: f64, hi: f64) -> BezPath {
    let mut p = BezPath::new();
    for i in 0..=200 {
        let x = lo + (hi - lo) * i as f64 / 200.;
        let pt = KPoint::new(x, f(x));
        if i == 0 { p.move_to(pt) } else { p.line_to(pt) }
    }
    p
}

fn easings() -> Vec<(&'static str, Easing)> {
    let stop = |output, input| LinearStop { output, input };
    vec![
        ("ease", Easing::EASE),
        ("ease-in", Easing::EASE_IN),
        ("ease-out", Easing::EASE_OUT),
        ("ease-in-out", Easing::EASE_IN_OUT),
        (
            "cubic-bezier(.5,-.5,.5,1.5)",
            Easing::cubic_bezier(0.5, -0.5, 0.5, 1.5).unwrap(),
        ),
        (
            "steps(4, jump-end)",
            Easing::steps(4, StepPosition::JumpEnd).unwrap(),
        ),
        (
            "steps(4, jump-both)",
            Easing::steps(4, StepPosition::JumpBoth).unwrap(),
        ),
        (
            "linear(0, .9 30%, .7, 1)",
            Easing::linear_stops(&[
                stop(0., None),
                stop(0.9, Some(0.3)),
                stop(0.7, None),
                stop(1., None),
            ])
            .unwrap(),
        ),
    ]
}

pub fn render(lab: &mut CurveLab, window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let panel = &lab.func;
    let curve = panel.curve();
    let knots = panel.knots.clone();
    let kind = panel.kind;
    let view = panel.view;
    let reduce = cx.reduce_motion();
    let phase = if panel.animate && !reduce {
        window.request_animation_frame();
        (panel.started.elapsed().as_secs_f64() / 1.6).fract()
    } else {
        0.6
    };
    let kind_button = |k: Kind, label: &'static str, cx: &mut Context<CurveLab>| {
        button(label, kind == k).on_click(cx.listener(move |this, _, _, cx| {
            this.func.kind = k;
            cx.notify();
        }))
    };
    let controls = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(kind_button(Kind::Monotone, "Monotone (PCHIP)", cx))
        .child(kind_button(Kind::Linear, "Linear", cx))
        .child(kind_button(Kind::Bezier, "Clamped Bézier", cx))
        .child(
            button("Animate easing", panel.animate && !reduce).on_click(cx.listener(
                |this, _, _, cx| {
                    this.func.animate = !this.func.animate;
                    cx.notify();
                },
            )),
        )
        .child(view_button(cx, view, |lab: &mut CurveLab| {
            &mut lab.func.view
        }));
    let lut = curve.as_ref().map(|c| c.to_lut_f32::<256>());
    let editor_curve = curve.clone();
    let editor = interactive(
        "tone-curve",
        cx,
        &panel.area,
        |lab: &mut CurveLab| &mut lab.func.view,
        |this: &mut CurveLab, at, _, _| {
            let t = FuncPanel::transform(this.func.view, this.func.area.get());
            this.func.drag = this
                .func
                .knots
                .iter()
                .position(|k| (t * *k).distance(kp(at)) <= 12.);
            this.func.drag.is_some()
        },
        |this: &mut CurveLab, at, _| {
            if let Some(i) = this.func.drag {
                let m =
                    FuncPanel::transform(this.func.view, this.func.area.get()).inverse() * kp(at);
                let k = &mut this.func.knots;
                let n = k.len();
                let (lo, hi) = match i {
                    0 => (0., 0.),
                    _ if i == n - 1 => (1., 1.),
                    _ => (k[i - 1].x + 0.01, k[i + 1].x - 0.01),
                };
                k[i] = KPoint::new(m.x.clamp(lo, hi), m.y.clamp(0., 1.));
            }
        },
        |this: &mut CurveLab, _| this.func.drag = None,
    )
    .overflow_hidden()
    .child(
        canvas(record(&panel.area), move |bounds, _, window, _| {
            let t = FuncPanel::transform(view, bounds);
            let frame = Affine::IDENTITY;
            let (a, b) = (t * KPoint::new(0., 0.), t * KPoint::new(1., 1.));
            window.paint_quad(fill(
                Bounds::from_corners(gp(KPoint::new(a.x, b.y)), gp(KPoint::new(b.x, a.y))),
                rgb(0x0c1220),
            ));
            for i in 1..4 {
                let g = i as f64 / 4.;
                stroke(
                    window,
                    &line(t * KPoint::new(g, 0.), t * KPoint::new(g, 1.)),
                    frame,
                    1.,
                    hsla(0., 0., 1., 0.08),
                );
                stroke(
                    window,
                    &line(t * KPoint::new(0., g), t * KPoint::new(1., g)),
                    frame,
                    1.,
                    hsla(0., 0., 1., 0.08),
                );
            }
            stroke(
                window,
                &line(t * KPoint::new(0., 0.), t * KPoint::new(1., 1.)),
                frame,
                1.,
                hsla(0., 0., 1., 0.2),
            );
            if let Some(c) = &editor_curve {
                contrast(window, &plot(|x| c.eval(x), 0., 1.), t, 2.);
            }
            for k in &knots {
                handle(window, t * *k, 5., rgb(0xffffff));
            }
        })
        .size_full(),
    );
    let strip = canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if let Some(lut) = lut {
                let w = f32::from(bounds.size.width) / lut.len() as f32;
                for (i, v) in lut.iter().enumerate() {
                    let x = bounds.origin.x + px(i as f32 * w);
                    window.paint_quad(fill(
                        Bounds::new(
                            point(x, bounds.origin.y),
                            size(px(w + 0.5), bounds.size.height),
                        ),
                        hsla(0., 0., *v, 1.),
                    ));
                }
            }
        },
    )
    .w(px(300.))
    .h(px(22.));
    let gallery = easings().into_iter().map(move |(label, easing)| {
        card(label).child(
            div().w(px(150.)).h(px(110.)).child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let t =
                            fit(bounds, 150., 110.) * Affine::new([110., 0., 0., -70., 20., 90.]);
                        window.paint_quad(fill(bounds, rgb(0x0c1220)));
                        stroke(
                            window,
                            &line(KPoint::new(0., 0.), KPoint::new(1., 0.)),
                            t,
                            1.,
                            hsla(0., 0., 1., 0.15),
                        );
                        stroke(
                            window,
                            &line(KPoint::new(0., 1.), KPoint::new(1., 1.)),
                            t,
                            1.,
                            hsla(0., 0., 1., 0.15),
                        );
                        stroke(
                            window,
                            &plot(|x| easing.eval(x), -0.15, 1.15),
                            t,
                            2.,
                            rgb(0x8fd3ff),
                        );
                        dot(
                            window,
                            t * KPoint::new(phase, easing.eval(phase)),
                            4.,
                            rgb(0xffd166),
                        );
                    },
                )
                .size_full(),
            ),
        )
    });
    div()
        .id("functions-scroll")
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
                .gap_4()
                .child(
                    card("Tone curve and its 256-entry LUT")
                        .child(div().w(px(300.)).h(px(300.)).child(editor))
                        .child(strip),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .max_w(px(680.))
                        .children(gallery),
                ),
        )
        .into_any_element()
}
