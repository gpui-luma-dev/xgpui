//! Lab-local Euler–Maclaurin evaluator and cached, deforming grid.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use std::rc::Rc;
use web_time::Instant;
use xgpui_curve::{
    fit::{ParameterizedFit, ParameterizedFitOptions, fit_parameterized, rdp},
    geom::clip_to_rect,
    kurbo::{Affine, BezPath, PathSeg, Point as P, Rect, Shape},
    measure::{MeasuredPath, PathEffect},
};

// Complex arithmetic stays local to this experiment: no extension API changes.
#[derive(Clone, Copy, Debug)]
struct C(f64, f64);
impl C {
    fn add(self, b: Self) -> Self {
        Self(self.0 + b.0, self.1 + b.1)
    }
    fn mul(self, b: Self) -> Self {
        Self(self.0 * b.0 - self.1 * b.1, self.0 * b.1 + self.1 * b.0)
    }
    fn scale(self, b: f64) -> Self {
        Self(self.0 * b, self.1 * b)
    }
    fn div(self, b: Self) -> Self {
        self.mul(Self(b.0, -b.1))
            .scale(1. / (b.0 * b.0 + b.1 * b.1))
    }
    fn power(n: f64, s: Self) -> Self {
        let r = (s.0 * n.ln()).exp();
        let a = s.1 * n.ln();
        Self(r * a.cos(), r * a.sin())
    }
}
fn zeta(s: C) -> C {
    // Euler–Maclaurin with six Bernoulli corrections. Bounded demo domain:
    // -3 <= Re(s) <= 5, |Im(s)| <= 3, excluding |s-1| < .12.
    let terms = if s.1.abs() <= 4. {
        24
    } else {
        64.max(s.1.abs().ceil() as usize + 32)
    };
    let n = terms as f64;
    let mut sum = C(0., 0.);
    for k in 1..terms {
        sum = sum.add(C::power(k as f64, s.scale(-1.)));
    }
    sum = sum
        .add(C::power(n, C(1. - s.0, -s.1)).div(C(s.0 - 1., s.1)))
        .add(C::power(n, s.scale(-1.)).scale(0.5));
    let coefficients = [
        1. / 12.,
        -1. / 720.,
        1. / 30240.,
        -1. / 1209600.,
        1. / 47900160.,
        -691. / 1307674368000.,
    ];
    let mut rising = s;
    for (j, b) in coefficients.into_iter().enumerate() {
        if j > 0 {
            rising = rising
                .mul(s.add(C((2 * j - 1) as f64, 0.)))
                .mul(s.add(C((2 * j) as f64, 0.)));
        }
        sum = sum.add(
            rising
                .mul(C::power(n, C(-s.0 - (2 * j + 1) as f64, -s.1)))
                .scale(b),
        );
    }
    sum
}
struct Wire {
    samples: Vec<Option<(P, P)>>,
    hue: f32,
}
fn grid() -> Vec<Wire> {
    let mut wires = Vec::new();
    for vertical in [false, true] {
        let count = if vertical { 32 } else { 24 };
        for j in 0..=count {
            let fixed = -3. + j as f64 * 0.25;
            let samples = (0..=512)
                .map(|i| {
                    let u = i as f64 / 512.;
                    let p = if vertical {
                        P::new(fixed, -3. + 6. * u)
                    } else {
                        P::new(-3. + 8. * u, fixed)
                    };
                    if (p.x - 1.).hypot(p.y) < 0.12 {
                        return None;
                    }
                    let q = zeta(C(p.x, p.y));
                    Some((p, P::new(q.0, q.1)))
                })
                .collect();
            wires.push(Wire {
                samples,
                hue: if vertical { 0.97 } else { 0.15 },
            });
        }
    }
    wires
}
const TRACE_MAX: f64 = 120.;
const TRACE_STEPS: usize = 12288;
fn sample_trace(sigma: f64) -> Vec<P> {
    (0..=TRACE_STEPS)
        .map(|i| {
            let q = zeta(C(sigma, TRACE_MAX * i as f64 / TRACE_STEPS as f64));
            P::new(q.0, q.1)
        })
        .collect()
}

const FIT_TOLERANCE: f64 = 0.002;
struct FittedSpan {
    path: BezPath,
    measured: MeasuredPath,
    start: f64,
    end: f64,
}
struct FittedTrace {
    fit: ParameterizedFit,
    spans: Vec<FittedSpan>,
    measured: MeasuredPath,
}
impl FittedTrace {
    fn new(points: &[P]) -> Self {
        let parameters: Vec<_> = (0..points.len())
            .map(|i| TRACE_MAX * i as f64 / TRACE_STEPS as f64)
            .collect();
        let mut options = ParameterizedFitOptions::default();
        options.tolerance = FIT_TOLERANCE;
        options.max_segments = 4096;
        let fit =
            fit_parameterized(points, &parameters, options).expect("finite bounded lab samples");
        Self::from_fit(fit)
    }
    fn from_fit(fit: ParameterizedFit) -> Self {
        let mut spans = Vec::new();
        let mut total = 0.;
        for segment in fit.segments() {
            let b = segment.curve;
            let mut path = BezPath::new();
            path.move_to(b.p0);
            path.curve_to(b.p1, b.p2, b.p3);
            let measured = MeasuredPath::new(&path, 1e-9).expect("bounded fitted segment");
            let length = measured.length();
            spans.push(FittedSpan {
                path,
                measured,
                start: total,
                end: total + length,
            });
            total += length;
        }
        let measured = MeasuredPath::new(fit.path(), 1e-9).expect("bounded fitted trace");
        Self {
            fit,
            spans,
            measured,
        }
    }
    fn s_at_t(&self, t: f64) -> f64 {
        let (segment, u) = self.fit.segment_at(t).expect("finite source parameter");
        self.spans[segment].start
            + xgpui_curve::measure::arclen(
                PathSeg::Cubic(self.fit.segments()[segment].curve),
                0.,
                u,
                1e-9,
            )
    }
    fn t_at_s(&self, s: f64) -> f64 {
        let location = self.measured.locate(s).expect("nonempty trace");
        self.fit
            .parameter_at(location.segment, location.t)
            .expect("measured fit location")
    }
}
struct Trace {
    points: Vec<P>,
    zeros: Vec<f64>,
    fitted: FittedTrace,
}
impl Trace {
    fn new(sigma: f64) -> Self {
        let points = sample_trace(sigma);
        let mut zeros = Vec::new();
        // Refine local minima of |ζ| along the source line. A near miss is
        // never labeled as a zero: both complex components must vanish.
        for i in 1..points.len() - 1 {
            let norm = points[i].to_vec2().hypot();
            if norm >= points[i - 1].to_vec2().hypot() || norm >= points[i + 1].to_vec2().hypot() {
                continue;
            }
            let seed = TRACE_MAX * i as f64 / TRACE_STEPS as f64;
            let mut t = seed;
            for _ in 0..6 {
                let f = zeta(C(sigma, t));
                let a = zeta(C(sigma, t - 1e-4));
                let b = zeta(C(sigma, t + 1e-4));
                let d = C((b.0 - a.0) / 2e-4, (b.1 - a.1) / 2e-4);
                let denom = d.0 * d.0 + d.1 * d.1;
                if denom < 1e-16 {
                    break;
                }
                t -= ((f.0 * d.0 + f.1 * d.1) / denom).clamp(-0.05, 0.05);
            }
            let f = zeta(C(sigma, t));
            if (t - seed).abs() < 0.1 && (0. ..=TRACE_MAX).contains(&t) && f.0.hypot(f.1) < 1e-8 {
                if zeros
                    .last()
                    .is_none_or(|last: &f64| (t - *last).abs() > 0.01)
                {
                    zeros.push(t);
                }
            }
        }
        let fitted = FittedTrace::new(&points);
        Self {
            points,
            zeros,
            fitted,
        }
    }
}
fn trace_t(phase: f64) -> f64 {
    ((phase - 0.5) * 2.).clamp(0., 1.) * TRACE_MAX
}

pub struct ZetaPanel {
    trace: Option<Rc<Trace>>,
    sigma: f64,
    source_area: Area,
    source_drag: bool,
    wires: Option<Rc<Vec<Wire>>>,
    area: Area,
    timeline: Area,
    view: Camera,
    phase: f64,
    playing: bool,
    last: Option<Instant>,
    tolerance: f64,
    dots: bool,
    source_width: f32,
    trace_width: f32,
    tip_guides: bool,
    bezier: bool,
    constant_speed: bool,
    inspector: bool,
    compare: bool,
    pending_t: Option<f64>,
}
impl ZetaPanel {
    pub fn new() -> Self {
        Self {
            trace: None,
            sigma: 0.5,
            source_area: area(),
            source_drag: false,
            wires: None,
            area: area(),
            timeline: area(),
            view: Camera::default(),
            phase: 0.,
            playing: true,
            last: None,
            tolerance: 0.6,
            dots: false,
            source_width: 1.,
            trace_width: 1.25,
            tip_guides: true,
            bezier: true,
            constant_speed: true,
            inspector: true,
            compare: false,
            pending_t: None,
        }
    }
    #[cfg(not(target_family = "wasm"))]
    pub(super) fn snapshot(&mut self, phase: f64, sigma: f64) {
        self.set_sigma(sigma);
        self.pending_t = None;
        self.phase = phase;
        self.playing = false;
        self.last = None;
    }
    fn move_source(&mut self, at: Point<Pixels>) {
        let b = self.source_area.get();
        let x = f32::from(at.x - b.origin.x) as f64;
        let width = f32::from(b.size.width) as f64;
        let sigma = (((x - 26.) / (width - 44.).max(1.)).clamp(0.1, 0.9) * 40.).round() / 40.;
        self.set_sigma(sigma);
    }
    fn set_sigma(&mut self, sigma: f64) {
        if (sigma - self.sigma).abs() > 1e-9 {
            if self.phase >= 0.5 {
                if let Some(trace) = &self.trace {
                    self.pending_t = Some(if self.constant_speed {
                        trace.fitted.t_at_s(
                            ((self.phase - 0.5) * 2.).clamp(0., 1.)
                                * trace.fitted.measured.length(),
                        )
                    } else {
                        trace_t(self.phase)
                    });
                }
            }
            self.sigma = sigma;
            self.trace = None;
            // Keep the current t while comparing source lines.
            self.phase = self.phase.max(0.5);
            self.last = None;
        }
    }
    fn scrub(&mut self, at: Point<Pixels>) {
        let b = self.timeline.get();
        self.phase =
            ((f32::from(at.x - b.origin.x) / f32::from(b.size.width).max(1.)) as f64).clamp(0., 1.);
        self.playing = false;
        self.last = None;
    }
}
fn paths(wire: &Wire, phase: f64, transform: Affine, tolerance: f64) -> Vec<Vec<P>> {
    let mut result = Vec::new();
    let mut run = Vec::new();
    for sample in wire.samples.iter().chain(std::iter::once(&None)) {
        if let Some((a, b)) = sample {
            run.push(transform * (*a + (*b - *a) * phase));
        } else if !run.is_empty() {
            result.push(rdp(&run, tolerance));
            run.clear();
        }
    }
    result
}
// Keep tick labels readable at every camera scale.
fn tick_step(pixels_per_unit: f64) -> f64 {
    let desired = 70. / pixels_per_unit;
    let power = 10f64.powf(desired.log10().floor());
    [1., 2., 5., 10.]
        .into_iter()
        .map(|n| n * power)
        .find(|step| *step >= desired)
        .unwrap_or(power * 10.)
}
fn tick_text(value: f64, step: f64) -> String {
    let digits = (-step.log10().floor()).max(0.) as usize;
    let value = if value.abs() < step * 0.01 { 0. } else { value };
    format!("{value:.digits$}")
}
fn axis_label(text: String, at: P, centered: bool, window: &mut Window, cx: &mut App) {
    let mut style = window.text_style();
    style.color = rgb(0xc2d0e3).into();
    let line = window.text_system().shape_line(
        text.clone().into(),
        px(13.),
        &[style.to_run(text.len())],
        None,
    );
    let origin = point(
        px(at.x as f32) - if centered { line.width / 2. } else { px(0.) },
        px(at.y as f32),
    );
    window.paint_quad(fill(
        Bounds::new(
            origin - point(px(2.), px(0.)),
            size(line.width + px(4.), px(18.)),
        ),
        rgb(0x060b13),
    ));
    let _ = line.paint(origin, px(18.), TextAlign::Left, None, window, cx);
}
pub fn render(lab: &mut CurveLab, window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let p = &mut lab.zeta;
    let wires = p.wires.get_or_insert_with(|| Rc::new(grid())).clone();
    let sigma = p.sigma;
    let reduce_motion = cx.reduce_motion();
    let trace = p
        .trace
        .get_or_insert_with(|| Rc::new(Trace::new(sigma)))
        .clone();
    if let Some(current) = p.pending_t.take() {
        let progress = if p.constant_speed {
            trace.fitted.s_at_t(current) / trace.fitted.measured.length()
        } else {
            current / TRACE_MAX
        };
        p.phase = 0.5 + 0.5 * progress;
    }
    let zero_count = trace.zeros.len();
    let segment_count = trace.fitted.fit.diagnostics().segments;
    let fit_error = trace.fitted.fit.diagnostics().max_sample_error;
    let fit_rms = trace.fitted.fit.diagnostics().rms_sample_error;
    let fit_status = if trace.fitted.fit.diagnostics().tolerance_met {
        "tolerance met"
    } else if trace.fitted.fit.diagnostics().segment_limit_reached {
        "segment limit reached"
    } else {
        "tolerance not met"
    };
    let total_length = trace.fitted.measured.length();
    let now = Instant::now();
    if p.playing && !reduce_motion {
        if let Some(last) = p.last {
            let duration = if p.phase < 0.5 { 20. } else { 60. };
            p.phase =
                (p.phase + now.duration_since(last).as_secs_f64().min(0.1) / duration).min(1.);
        }
        if p.phase >= 1. {
            p.playing = false;
        } else {
            window.request_animation_frame();
        }
        p.last = Some(now);
    } else {
        p.last = None;
    }
    let (phase, view, tolerance, dots) = (p.phase, p.view, p.tolerance, p.dots);
    let (source_width, trace_width, tip_guides) = (p.source_width, p.trace_width, p.tip_guides);
    let (bezier, constant_speed, inspector, compare) =
        (p.bezier, p.constant_speed, p.inspector, p.compare);
    let progress = ((phase - 0.5) * 2.).clamp(0., 1.);
    let current_t = if constant_speed {
        trace.fitted.t_at_s(progress * total_length)
    } else {
        trace_t(phase)
    };
    let current_s = if constant_speed {
        progress * total_length
    } else {
        trace.fitted.s_at_t(current_t)
    };
    let controls = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(
            button(if p.playing { "Pause" } else { "Play" }, p.playing).on_click(cx.listener(
                |lab, _, _, cx| {
                    let p = &mut lab.zeta;
                    if p.phase >= 1. {
                        p.phase = 0.;
                    }
                    p.playing = !p.playing;
                    p.last = None;
                    cx.notify();
                },
            )),
        )
        .child(
            button("Grid", phase == 0.).on_click(cx.listener(|lab, _, _, cx| {
                lab.zeta.phase = 0.;
                lab.zeta.playing = false;
                cx.notify();
            })),
        )
        .child(
            button("Mapped grid", phase == 0.5).on_click(cx.listener(|lab, _, _, cx| {
                lab.zeta.phase = 0.5;
                lab.zeta.playing = false;
                cx.notify();
            })),
        )
        .child(
            button("Trace end", phase == 1.).on_click(cx.listener(|lab, _, _, cx| {
                lab.zeta.phase = 1.;
                lab.zeta.playing = false;
                cx.notify();
            })),
        )
        .child(
            button(format!("Tolerance: {tolerance:.1} px"), false).on_click(cx.listener(
                |lab, _, _, cx| {
                    lab.zeta.tolerance = if lab.zeta.tolerance < 0.3 {
                        0.6
                    } else if lab.zeta.tolerance < 1. {
                        1.5
                    } else {
                        0.2
                    };
                    cx.notify();
                },
            )),
        )
        .child(
            button("Sample points", dots).on_click(cx.listener(|lab, _, _, cx| {
                lab.zeta.dots = !lab.zeta.dots;
                cx.notify();
            })),
        )
        .child(
            button("Critical line σ = ½", (sigma - 0.5).abs() < 1e-9).on_click(cx.listener(
                |lab, _, _, cx| {
                    lab.zeta.set_sigma(0.5);
                    cx.notify();
                },
            )),
        )
        .child(view_button(cx, view, |lab: &mut CurveLab| {
            &mut lab.zeta.view
        }));
    let thickness = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child("Source line:")
        .children([0.75f32, 1., 2., 3.].into_iter().map(|width| {
            button(format!("{width} px"), source_width == width).on_click(cx.listener(
                move |lab, _, _, cx| {
                    lab.zeta.source_width = width;
                    cx.notify();
                },
            ))
        }))
        .child(div().ml_2().child("Trace:"))
        .children([0.75f32, 1.25, 2., 3.].into_iter().map(|width| {
            // Separate IDs from the source width controls.
            button(format!("Trace {width} px"), trace_width == width).on_click(cx.listener(
                move |lab, _, _, cx| {
                    lab.zeta.trace_width = width;
                    cx.notify();
                },
            ))
        }))
        .child(
            button("Tip guides", tip_guides).on_click(cx.listener(|lab, _, _, cx| {
                lab.zeta.tip_guides = !lab.zeta.tip_guides;
                cx.notify();
            })),
        );
    let curve_controls = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(
            button("Bézier trace", bezier).on_click(cx.listener(|lab, _, _, cx| {
                lab.zeta.bezier = !lab.zeta.bezier;
                cx.notify();
            })),
        )
        .child(
            button("Constant speed", constant_speed).on_click(cx.listener(|lab, _, _, cx| {
                let p = &mut lab.zeta;
                if p.phase >= 0.5 {
                    if let Some(trace) = &p.trace {
                        let progress = ((p.phase - 0.5) * 2.).clamp(0., 1.);
                        let t = if p.constant_speed {
                            trace
                                .fitted
                                .t_at_s(progress * trace.fitted.measured.length())
                        } else {
                            trace_t(p.phase)
                        };
                        p.constant_speed = !p.constant_speed;
                        p.phase = 0.5
                            + 0.5
                                * if p.constant_speed {
                                    trace.fitted.s_at_t(t) / trace.fitted.measured.length()
                                } else {
                                    t / TRACE_MAX
                                };
                    }
                } else {
                    p.constant_speed = !p.constant_speed;
                }
                cx.notify();
            })),
        )
        .child(
            button("Tangent & curvature", inspector).on_click(cx.listener(|lab, _, _, cx| {
                lab.zeta.inspector = !lab.zeta.inspector;
                cx.notify();
            })),
        )
        .child(
            button("Compare samples", compare).on_click(cx.listener(|lab, _, _, cx| {
                lab.zeta.compare = !lab.zeta.compare;
                cx.notify();
            })),
        );
    let source = div()
        .id("zeta-source")
        .absolute()
        .top_2()
        .right_2()
        .w(px(220.))
        .h(px(245.))
        .rounded_lg()
        .overflow_hidden()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|lab, e: &MouseDownEvent, _, cx| {
                lab.zeta.source_drag = true;
                lab.zeta.move_source(e.position);
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|lab, e: &MouseMoveEvent, _, cx| {
            if e.pressed_button != Some(MouseButton::Left) {
                lab.zeta.source_drag = false;
            }
            if lab.zeta.source_drag {
                lab.zeta.move_source(e.position);
                cx.stop_propagation();
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|lab, _, _, cx| {
                lab.zeta.source_drag = false;
                cx.stop_propagation();
            }),
        )
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .on_pinch(|_, _, cx| cx.stop_propagation())
        .child(
            canvas(record(&p.source_area), move |b, _, window, cx| {
                window.paint_quad(fill(b, rgb(0x060b13)));
                let left = f32::from(b.left()) as f64 + 26.;
                let right = f32::from(b.right()) as f64 - 18.;
                let top = f32::from(b.top()) as f64 + 46.;
                let bottom = f32::from(b.bottom()) as f64 - 37.;
                let source_point = |x: f64, t: f64| {
                    P::new(
                        left + (right - left) * x,
                        bottom - (bottom - top) * t / TRACE_MAX,
                    )
                };
                for i in 0..=4 {
                    let x = i as f64 / 4.;
                    stroke(
                        window,
                        &line(source_point(x, 0.), source_point(x, TRACE_MAX)),
                        Affine::IDENTITY,
                        1.,
                        rgb(0x243b50),
                    );
                    let v = i as f64 * 30.;
                    stroke(
                        window,
                        &line(source_point(0., v), source_point(1., v)),
                        Affine::IDENTITY,
                        1.,
                        rgb(0x243b50),
                    );
                    axis_label(
                        format!("{v:.0}"),
                        P::new(left - 24., source_point(0., v).y - 8.),
                        false,
                        window,
                        cx,
                    );
                }
                stroke(
                    window,
                    &line(source_point(0.5, 0.), source_point(0.5, TRACE_MAX)),
                    Affine::IDENTITY,
                    1.,
                    rgb(0x698099),
                );
                stroke(
                    window,
                    &line(source_point(sigma, 0.), source_point(sigma, TRACE_MAX)),
                    Affine::IDENTITY,
                    source_width,
                    rgb(0x84d8ff),
                );
                let current = current_t;
                if phase >= 0.5 {
                    handle(window, source_point(sigma, current), 4., rgb(0xffd166));
                }
                axis_label(
                    "Source s = σ + it".into(),
                    P::new(left - 17., top - 39.),
                    false,
                    window,
                    cx,
                );
                axis_label(
                    format!("σ = {sigma:.3}   t = {current:.1}"),
                    P::new(left - 17., top - 20.),
                    false,
                    window,
                    cx,
                );
                for (x, label) in [(0., "0"), (0.5, "½"), (1., "1")] {
                    axis_label(
                        label.into(),
                        P::new(source_point(x, 0.).x, bottom + 3.),
                        true,
                        window,
                        cx,
                    );
                }
                axis_label(
                    "Drag line left / right".into(),
                    P::new(left - 17., bottom + 20.),
                    false,
                    window,
                    cx,
                );
            })
            .size_full(),
        );
    let timeline = div()
        .id("zeta-timeline")
        .h(px(28.))
        .w_full()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|lab, e: &MouseDownEvent, _, cx| {
                lab.zeta.scrub(e.position);
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|lab, e: &MouseMoveEvent, _, cx| {
            if e.pressed_button == Some(MouseButton::Left) {
                lab.zeta.scrub(e.position);
                cx.notify();
            }
        }))
        .child(
            canvas(record(&p.timeline), move |b, _, window, _| {
                window.paint_quad(fill(b, rgb(0x24334a)));
                window.paint_quad(fill(
                    Bounds::new(b.origin, size(b.size.width * phase as f32, b.size.height)),
                    rgb(0x486fc2),
                ));
            })
            .size_full(),
        );
    let plot = interactive(
        "zeta-plot",
        cx,
        &p.area,
        |lab: &mut CurveLab| &mut lab.zeta.view,
        |_, _, _, _| false,
        |_, _, _| {},
        |_, _| {},
    )
    .overflow_hidden()
    .child(
        canvas(record(&p.area), move |bounds, _, window, cx| {
            window.paint_quad(fill(bounds, rgb(0x060b13)));
            let t = view.around(bounds)
                * fit(bounds, 900., 600.)
                * Affine::new([100., 0., 0., -100., 350., 300.]);
            let rect = Rect::new(
                f32::from(bounds.left()) as f64,
                f32::from(bounds.top()) as f64,
                f32::from(bounds.right()) as f64,
                f32::from(bounds.bottom()) as f64,
            );
            let model_a = t.inverse() * P::new(rect.x0, rect.y1);
            let model_b = t.inverse() * P::new(rect.x1, rect.y0);
            let step = tick_step((t * P::new(1., 0.) - t * P::ZERO).hypot());
            let xs = ((model_a.x / step).ceil() as i32)..=((model_b.x / step).floor() as i32);
            let ys = ((model_a.y / step).ceil() as i32)..=((model_b.y / step).floor() as i32);
            for i in xs.clone() {
                let x = i as f64 * step;
                stroke(
                    window,
                    &line(t * P::new(x, model_a.y), t * P::new(x, model_b.y)),
                    Affine::IDENTITY,
                    1.,
                    rgb(0x183544),
                );
            }
            for i in ys.clone() {
                let y = i as f64 * step;
                stroke(
                    window,
                    &line(t * P::new(model_a.x, y), t * P::new(model_b.x, y)),
                    Affine::IDENTITY,
                    1.,
                    rgb(0x183544),
                );
            }
            for path in [
                line(t * P::new(0., model_a.y), t * P::new(0., model_b.y)),
                line(t * P::new(model_a.x, 0.), t * P::new(model_b.x, 0.)),
            ] {
                stroke(
                    window,
                    &clip_to_rect(&path, rect),
                    Affine::IDENTITY,
                    1.5,
                    rgb(0x708095),
                );
            }
            for wire in wires.iter() {
                for points in paths(wire, (phase * 2.).min(1.), t, tolerance) {
                    let mut path = BezPath::new();
                    path.move_to(points[0]);
                    for pt in &points[1..] {
                        path.line_to(*pt);
                    }
                    stroke(
                        window,
                        &clip_to_rect(&path, rect),
                        Affine::IDENTITY,
                        1.,
                        hsla(
                            wire.hue,
                            (0.65 - 0.55 * ((phase - 0.5) * 10.).clamp(0., 1.)) as f32,
                            0.73,
                            (0.72 - 0.47 * ((phase - 0.5) * 10.).clamp(0., 1.)) as f32,
                        ),
                    );
                    if dots {
                        for pt in points {
                            if rect.contains(pt) {
                                dot(window, pt, 1.7, rgb(0xffffff));
                            }
                        }
                    }
                }
            }
            let end = current_t / TRACE_MAX * TRACE_STEPS as f64;
            if bezier {
                for (i, span) in trace.fitted.spans.iter().enumerate() {
                    if current_s <= span.start {
                        break;
                    }
                    let fraction =
                        ((current_s - span.start) / (span.end - span.start)).clamp(0., 1.);
                    let partial;
                    let path = if fraction >= 1. {
                        &span.path
                    } else {
                        partial = span
                            .measured
                            .apply(&PathEffect::new().trim(0. ..fraction).expect("ordered trim"))
                            .expect("bounded trim");
                        &partial
                    };
                    let screen = t * path;
                    stroke(
                        window,
                        &clip_to_rect(&screen, rect),
                        Affine::IDENTITY,
                        trace_width,
                        hsla(
                            (0.16
                                + (trace.fitted.fit.segments()[i].parameter_range.start / TRACE_MAX)
                                    as f32
                                    * 2.85)
                                % 1.,
                            0.7,
                            0.7,
                            1.,
                        ),
                    );
                }
            }
            if !bezier || compare {
                for i in 0..(end.ceil() as usize).min(TRACE_STEPS) {
                    let a = trace.points[i];
                    let b = a + (trace.points[i + 1] - a) * (end - i as f64).min(1.);
                    let color = if bezier {
                        hsla(0., 0., 1., 0.3)
                    } else {
                        hsla(
                            (0.16 + i as f32 / TRACE_STEPS as f32 * 2.85) % 1.,
                            0.7,
                            0.7,
                            1.,
                        )
                    };
                    stroke(
                        window,
                        &clip_to_rect(&line(t * a, t * b), rect),
                        Affine::IDENTITY,
                        if bezier { 0.75 } else { trace_width },
                        color,
                    );
                    if compare && i % 32 == 0 && rect.contains(t * a) {
                        dot(window, t * a, 1.5, rgb(0xffffff));
                    }
                }
            }
            if phase >= 0.5 {
                let current = current_t;
                let index = (current / TRACE_MAX * TRACE_STEPS as f64).floor() as usize;
                let a = trace.points[index.min(TRACE_STEPS)];
                let b = trace.points[(index + 1).min(TRACE_STEPS)];
                let fraction = (current / TRACE_MAX * TRACE_STEPS as f64).fract();
                let geometry = trace.fitted.measured.geometry_at(current_s);
                let model_tip = if bezier {
                    geometry.as_ref().map(|g| g.location.point).unwrap_or(a)
                } else {
                    a + (b - a) * fraction
                };
                let tip = t * model_tip;
                if inspector {
                    if let Some(geometry) = &geometry {
                        let location = &geometry.location;
                        if let Some(tangent) = location.tangent {
                            let direction =
                                (t * (location.point + tangent) - t * location.point).normalize();
                            stroke(
                                window,
                                &clip_to_rect(
                                    &line(tip - direction * 42., tip + direction * 42.),
                                    rect,
                                ),
                                Affine::IDENTITY,
                                1.5,
                                rgb(0x70e6ff),
                            );
                            if let Some(k) = geometry.curvature {
                                let scale = (t * P::new(1., 0.) - t * P::ZERO).hypot();
                                let radius = 1. / k.abs();
                                if k.abs() > 1e-6 && radius * scale < 2000. {
                                    if let Some(normal) = geometry.normal {
                                        let center = location.point + normal / k;
                                        let circle =
                                            xgpui_curve::kurbo::Circle::new(center, radius)
                                                .to_path(0.25 / scale);
                                        stroke(
                                            window,
                                            &clip_to_rect(&(t * &circle), rect),
                                            Affine::IDENTITY,
                                            1.,
                                            hsla(0.8, 0.65, 0.75, 0.65),
                                        );
                                        stroke(
                                            window,
                                            &clip_to_rect(
                                                &line(t * location.point, t * center),
                                                rect,
                                            ),
                                            Affine::IDENTITY,
                                            1.,
                                            hsla(0.8, 0.65, 0.75, 0.4),
                                        );
                                    }
                                }
                                axis_label(
                                    format!("κ = {k:.3} · radius = {radius:.3}"),
                                    P::new(rect.x0 + 10., rect.y1 - 26.),
                                    false,
                                    window,
                                    cx,
                                );
                            } else {
                                axis_label(
                                    "Curvature undefined at stationary point".into(),
                                    P::new(rect.x0 + 10., rect.y1 - 26.),
                                    false,
                                    window,
                                    cx,
                                );
                            }
                        }
                    }
                }
                if tip_guides {
                    let origin = t * P::ZERO;
                    let mut guide = BezPath::new();
                    guide.move_to(P::new(origin.x, tip.y));
                    guide.line_to(tip);
                    guide.line_to(P::new(tip.x, origin.y));
                    stroke(
                        window,
                        &clip_to_rect(&guide, rect),
                        Affine::IDENTITY,
                        1.,
                        hsla(0.14, 0.65, 0.7, 0.55),
                    );
                    // A small square corner makes the orthogonal projection
                    // readable without depending on the curve's tangent.
                    if (origin.x - tip.x).abs() > 10. && (origin.y - tip.y).abs() > 10. {
                        let dx = (origin.x - tip.x).signum() * 7.;
                        let dy = (origin.y - tip.y).signum() * 7.;
                        let mut corner = BezPath::new();
                        corner.move_to(P::new(tip.x + dx, tip.y));
                        corner.line_to(P::new(tip.x + dx, tip.y + dy));
                        corner.line_to(P::new(tip.x, tip.y + dy));
                        stroke(
                            window,
                            &clip_to_rect(&corner, rect),
                            Affine::IDENTITY,
                            1.,
                            rgb(0xffd166),
                        );
                    }
                }
                if rect.contains(tip) {
                    handle(window, tip, 4., rgb(0xffd166));
                }
                if let Some(zero) = trace.zeros.iter().rev().find(|zero| **zero <= current) {
                    let age = current - zero;
                    let origin = t * P::ZERO;
                    if age < 2. && rect.contains(origin) {
                        let fade = (1. - age / 2.) as f32;
                        let radius = if reduce_motion {
                            8.
                        } else {
                            8. + age as f32 * 14.
                        };
                        dot(window, origin, radius, hsla(0.14, 0.9, 0.6, fade * 0.25));
                        dot(window, origin, 4., hsla(0.14, 0.9, 0.8, fade));
                        axis_label(
                            format!("ζ = 0 · t ≈ {zero:.3}"),
                            P::new(origin.x + 12., origin.y - 28.),
                            false,
                            window,
                            cx,
                        );
                    }
                }
            }
            // Labels are painted last so the trace cannot obscure them. When
            // an axis leaves the viewport, its scale stays at the nearest edge.
            if rect.width() >= 120. && rect.height() >= 80. {
                let zero = t * P::ZERO;
                let label_y = (zero.y + 6.).clamp(rect.y0 + 24., rect.y1 - 22.);
                let label_x = (zero.x + 7.).clamp(rect.x0 + 7., rect.x1 - 60.);
                for i in xs {
                    let x = i as f64 * step;
                    let at = t * P::new(x, 0.);
                    if at.x > rect.x0 + 28. && at.x < rect.x1 - 75. {
                        axis_label(tick_text(x, step), P::new(at.x, label_y), true, window, cx);
                    }
                }
                for i in ys {
                    if i == 0 {
                        continue;
                    }
                    let y = i as f64 * step;
                    let at = t * P::new(0., y);
                    if at.y > rect.y0 + 32. && at.y < rect.y1 - 24. {
                        axis_label(
                            format!("{}i", tick_text(y, step)),
                            P::new(label_x, at.y - 9.),
                            false,
                            window,
                            cx,
                        );
                    }
                }
                axis_label(
                    "Re (x)".into(),
                    P::new(rect.x1 - 62., label_y),
                    false,
                    window,
                    cx,
                );
                axis_label(
                    "Im (y)".into(),
                    P::new(label_x, rect.y0 + 7.),
                    false,
                    window,
                    cx,
                );
            }
        })
        .size_full(),
    )
    .child(source);
    let navigation = if cfg!(target_family = "wasm") {
        "Wheel/pinch to zoom · two-finger scroll to pan"
    } else {
        "macOS: three-finger up/down to zoom · two fingers to pan"
    };
    div().size_full().flex().flex_col().gap_2().child(controls).child(thickness).child(curve_controls)
        .child(format!("ζ({sigma:.3} + it) · t = {:.1} / 120 · {zero_count} detected zeros · drag source line to change σ",current_t))
        .child(format!("{segment_count} Bézier segments / {} samples · length {total_length:.2} · paired sample error: max {fit_error:.5}, RMS {fit_rms:.5} · {fit_status}",TRACE_STEPS+1))
        .child(timeline).child(div().flex_1().min_h_0().child(plot))
        .child(div().text_xs().child(format!("{navigation} · Lab approximation · |z − 1| < 0.12 excluded · tolerance simplifies sampled geometry; it is not a certified function-error bound")))
        .into_any_element()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn known_values_and_conjugation() {
        for (s, want) in [
            (C(2., 0.), std::f64::consts::PI.powi(2) / 6.),
            (C(0., 0.), -0.5),
            (C(-1., 0.), -1. / 12.),
            (C(-2., 0.), 0.),
        ] {
            let got = zeta(s);
            assert!((got.0 - want).abs() < 1e-10, "{s:?}: {got:?}");
            assert!(got.1.abs() < 1e-10);
        }
        let a = zeta(C(0.5, 2.));
        let b = zeta(C(0.5, -2.));
        assert!((a.0 - b.0).abs() < 1e-12 && (a.1 + b.1).abs() < 1e-12);
        // First nontrivial zero lies beyond this demo's visible domain, but
        // checks that the continuation also behaves away from the real axis.
        let zero = zeta(C(0.5, 14.134725141734695));
        assert!(zero.0.hypot(zero.1) < 1e-9);
    }
    #[::core::prelude::v1::test]
    fn critical_line_passes_through_known_zeros() {
        let trace = sample_trace(0.5);
        assert!((trace[0].x + 1.460354508809587).abs() < 1e-11);
        assert!(trace.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        for t in [
            14.134725141734695,
            21.022039638771555,
            25.01085758014569,
            37.586178158825675,
        ] {
            let q = zeta(C(0.5, t));
            assert!(q.0.hypot(q.1) < 1e-9, "t={t}: {q:?}");
        }
    }
    #[::core::prelude::v1::test]
    fn detects_crossings_but_not_near_misses() {
        let critical = Trace::new(0.5);
        assert!(critical.zeros.len() > 20);
        assert!((critical.zeros[0] - 14.134725141734695).abs() < 1e-8);
        assert!(Trace::new(0.6).zeros.is_empty());
        for sigma in [0.1, 0.9] {
            assert!(
                sample_trace(sigma)
                    .iter()
                    .all(|p| p.x.is_finite() && p.y.is_finite())
            );
        }
    }
    #[::core::prelude::v1::test]
    fn fitted_trace_preserves_samples_and_arc_length_correspondence() {
        for sigma in [0.1, 0.5, 0.9] {
            let trace = Trace::new(sigma);
            let fit = &trace.fitted;
            assert!(
                fit.fit.diagnostics().max_sample_error <= FIT_TOLERANCE * 1.01,
                "sigma={sigma}: error {}",
                fit.fit.diagnostics().max_sample_error
            );
            assert!(fit.fit.diagnostics().segments < TRACE_STEPS / 4);
            assert!(fit.spans.windows(2).all(|w| w[1].start >= w[0].end));
            for t in [0., 14.134725141734695, 40., 83.2, 120.] {
                assert!((fit.t_at_s(fit.s_at_t(t)) - t).abs() < 1e-7);
            }
            for fraction in [0.17, 0.53, 0.91] {
                let s = fit.measured.length() * fraction;
                let partial = fit
                    .measured
                    .apply(&PathEffect::new().trim(0. ..fraction).unwrap())
                    .unwrap();
                let partial_measure = MeasuredPath::new(&partial, 1e-7).unwrap();
                assert!((partial_measure.length() - s).abs() < 1e-5);
                let end = partial_measure.point_at(partial_measure.length()).unwrap();
                assert!(end.distance(fit.measured.point_at(s).unwrap()) < 1e-6);
            }
        }
    }
    #[::core::prelude::v1::test]
    fn pole_is_split_and_frames_are_finite() {
        for wire in grid() {
            for phase in [0., 0.4, 1.] {
                for run in paths(&wire, phase, Affine::scale(100.), 0.6) {
                    assert!(run.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
                    for pair in run.windows(2) {
                        assert!(pair[0].distance(pair[1]) < 3000.);
                    }
                }
            }
        }
    }
}

#[cfg(not(target_family = "wasm"))]
#[path = "zeta_review.rs"]
mod review;
#[cfg(not(target_family = "wasm"))]
pub use review::benchmark;
