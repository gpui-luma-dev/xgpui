//! A layered silk-ribbon composition using the public brush and path APIs.
use gpui::{prelude::*, *};
use std::time::Instant;
use xgpui_shader::{BlendMode, Brush, BrushPoint, BrushStop, Noise, painted};

pub struct PastelWaves {
    playing: bool,
    grain: bool,
    phase: f32,
    updated: Instant,
    frame_interval: f32,
    direction: f32,
    frames: u64,
    fps_frames: u32,
    fps_since: Instant,
    fps: f32,
}
impl Default for PastelWaves {
    fn default() -> Self {
        Self {
            playing: false,
            grain: true,
            phase: 0.,
            updated: Instant::now(),
            frame_interval: 1. / 60.,
            direction: 1.,
            frames: 0,
            fps_frames: 0,
            fps_since: Instant::now(),
            fps: 0.,
        }
    }
}
impl Render for PastelWaves {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.frames += 1;
        if self.playing {
            self.fps_frames += 1;
            let sample = now.duration_since(self.fps_since).as_secs_f32();
            if sample >= 0.5 {
                self.fps = self.fps_frames as f32 / sample;
                self.fps_frames = 0;
                self.fps_since = now;
            }
            // Smooth timing variation and avoid a large catch-up jump after a
            // slow frame or window interaction. This is gentle decorative motion.
            let elapsed = now.duration_since(self.updated).as_secs_f32().min(0.05);
            self.frame_interval += (elapsed - self.frame_interval) * 0.15;
            self.phase += self.frame_interval.min(1. / 30.) * 0.22 * self.direction;
            let end = std::f32::consts::PI;
            if self.phase > end {
                self.phase = 2. * end - self.phase;
                self.direction = -1.;
            } else if self.phase < 0. {
                self.phase = -self.phase;
                self.direction = 1.;
            }
            window.request_animation_frame();
        }
        self.updated = now;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xf9f5fa))
            .text_color(rgb(0x675875))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .child(
                        div()
                            .flex_1()
                            .child("Pastel waves · layered translucent ribbons"),
                    )
                    .child(
                        super::button("waves-play", if self.playing { "Pause" } else { "Animate" })
                            .text_color(rgb(0xffffff))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.playing = !this.playing;
                                this.updated = Instant::now();
                                this.frame_interval = 1. / 60.;
                                this.fps_since = this.updated;
                                this.fps_frames = 0;
                                this.fps = 0.;
                                cx.notify();
                            })),
                    )
                    .child(
                        super::button("waves-reverse", "Reverse")
                            .text_color(rgb(0xffffff))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.direction = -this.direction;
                                cx.notify();
                            })),
                    )
                    .child(
                        super::button(
                            "waves-grain",
                            if self.grain {
                                "Grain: on"
                            } else {
                                "Grain: off"
                            },
                        )
                        .text_color(rgb(0xffffff))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.grain = !this.grain;
                            cx.notify();
                        })),
                    )
                    .child(
                        super::button("waves-reset", "Reset")
                            .text_color(rgb(0xffffff))
                            .on_click(cx.listener(|this, _, _, cx| {
                                *this = Self::default();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .min_h(px(0.))
                    .child(
                        composition(
                            // Cosine easing makes each endpoint a gentle turn.
                            std::f32::consts::PI * 0.5 * (1. - self.phase.cos()),
                            self.grain,
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(12.))
                            .bottom(px(10.))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgba(0xffffffb0))
                            .text_xs()
                            .text_color(rgb(0x675875))
                            .child(format!(
                                "Frame {} · {} · {}",
                                self.frames,
                                if self.playing {
                                    format!("{:.0} FPS", self.fps)
                                } else {
                                    "paused".into()
                                },
                                if self.direction > 0. {
                                    "forward"
                                } else {
                                    "reverse"
                                }
                            )),
                    ),
            )
    }
}

pub fn composition(phase: f32, grain: bool) -> impl IntoElement + Styled {
    let background = Brush::freeform(vec![
        BrushPoint::new([0.03, 0.08], [0.95, 0.90, 0.99, 1.]),
        BrushPoint::new([0.58, 0.02], [1., 0.98, 0.96, 1.]),
        BrushPoint::new([0.98, 0.20], [1., 0.95, 0.87, 1.]),
        BrushPoint::new([0.07, 0.96], [0.73, 0.91, 1., 1.]),
        BrushPoint::new([0.96, 0.94], [0.95, 0.76, 0.96, 1.]),
    ])
    .unwrap();
    painted()
        .relative()
        .w_full()
        .overflow_hidden()
        .bg_brush(background)
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        paint_background_fields(bounds, window);
                        paint_ribbons(bounds, phase, grain, window);
                    });
                },
            )
            .size_full(),
        )
}

// Fixed, overlapping veils make the pale background carry color even above
// the ribbons. Each radial field fades completely into the underlying surface.
fn paint_background_fields(bounds: Bounds<Pixels>, window: &mut Window) {
    let fields = [
        ([0.09, 0.13], [0.42, 0.48], [0.82, 0.68, 0.97], 0.20),
        ([0.29, 0.32], [0.37, 0.30], [0.95, 0.72, 0.86], 0.18),
        ([0.44, 0.06], [0.30, 0.32], [1., 0.84, 0.77], 0.16),
        ([0.76, 0.18], [0.42, 0.43], [1., 0.92, 0.70], 0.23),
        ([0.02, 0.48], [0.36, 0.35], [0.69, 0.80, 1.], 0.19),
        ([0.54, 0.50], [0.42, 0.29], [0.96, 0.70, 0.90], 0.17),
        ([0.87, 0.58], [0.33, 0.39], [1., 0.76, 0.84], 0.18),
        ([0.28, 0.89], [0.51, 0.29], [0.59, 0.88, 1.], 0.23),
        ([0.73, 0.92], [0.40, 0.37], [0.85, 0.68, 0.98], 0.20),
    ];
    for (center, radii, color, alpha) in fields {
        Brush::radial(
            center,
            radii,
            vec![
                BrushStop::new(0., [color[0], color[1], color[2], alpha]),
                BrushStop::new(0.48, [color[0], color[1], color[2], alpha * 0.45]),
                BrushStop::new(1., [color[0], color[1], color[2], 0.]),
            ],
        )
        .unwrap()
        .paint_background(window, bounds, Corners::default())
        .unwrap();
    }
}

// Seven samples across each boundary. Cubic interpolation gives the ribbons
// independent crests and folds instead of a stack of parallel sine waves.
type Edge = [f32; 7];
struct Ribbon {
    top: Edge,
    bottom: Edge,
    color: [f32; 3],
    alpha: f32,
}
const RIBBONS: &[Ribbon] = &[
    Ribbon {
        top: [0.65, 0.47, 0.54, 0.59, 0.42, 0.34, 0.45],
        bottom: [0.82, 0.62, 0.72, 0.70, 0.64, 0.52, 0.61],
        color: [0.65, 0.69, 0.94],
        alpha: 0.21,
    },
    Ribbon {
        top: [0.72, 0.59, 0.65, 0.49, 0.33, 0.27, 0.44],
        bottom: [0.88, 0.79, 0.83, 0.64, 0.49, 0.49, 0.51],
        color: [0.92, 0.47, 0.72],
        alpha: 0.23,
    },
    Ribbon {
        top: [0.69, 0.67, 0.64, 0.24, 0.29, 0.47, 0.52],
        bottom: [0.88, 0.83, 0.79, 0.55, 0.41, 0.56, 0.48],
        color: [0.80, 0.40, 0.78],
        alpha: 0.30,
    },
    Ribbon {
        top: [0.57, 0.44, 0.58, 0.63, 0.46, 0.62, 0.48],
        bottom: [0.77, 0.66, 0.77, 0.78, 0.61, 0.76, 0.66],
        color: [0.58, 0.66, 0.94],
        alpha: 0.26,
    },
    Ribbon {
        top: [0.53, 0.66, 0.72, 0.77, 0.53, 0.68, 0.76],
        bottom: [0.82, 0.84, 0.95, 0.98, 0.74, 0.84, 0.92],
        color: [0.69, 0.49, 0.87],
        alpha: 0.26,
    },
    Ribbon {
        top: [0.78, 0.63, 0.72, 0.81, 0.59, 0.68, 0.68],
        bottom: [0.90, 0.78, 0.83, 0.94, 0.73, 0.80, 0.79],
        color: [0.33, 0.63, 0.91],
        alpha: 0.55,
    },
    Ribbon {
        top: [0.89, 0.69, 0.66, 0.77, 0.44, 0.59, 0.50],
        bottom: [1.02, 0.82, 0.84, 0.79, 0.55, 0.68, 0.63],
        color: [0.49, 0.40, 0.77],
        alpha: 0.55,
    },
    Ribbon {
        top: [0.87, 0.66, 0.73, 0.80, 0.66, 0.61, 0.68],
        bottom: [1.03, 0.78, 0.84, 0.89, 0.77, 0.74, 0.76],
        color: [0.64, 0.38, 0.80],
        alpha: 0.27,
    },
    Ribbon {
        top: [0.96, 0.73, 0.66, 0.79, 0.64, 0.69, 0.45],
        bottom: [1.09, 0.82, 0.81, 0.83, 0.69, 0.78, 0.58],
        color: [0.41, 0.79, 0.95],
        alpha: 0.40,
    },
];
fn edge_y(edge: Edge, x: f32) -> f32 {
    let t = x.clamp(0., 1.) * 6.;
    let i = (t.floor() as usize).min(5);
    let t = t - i as f32;
    let a = edge[i.saturating_sub(1)];
    let b = edge[i];
    let c = edge[i + 1];
    let d = edge[(i + 2).min(6)];
    0.5 * ((2. * b)
        + (-a + c) * t
        + (2. * a - 5. * b + 4. * c - d) * t * t
        + (-a + 3. * b - 3. * c + d) * t * t * t)
}
fn paint_ribbons(bounds: Bounds<Pixels>, phase: f32, grain: bool, window: &mut Window) {
    for (index, ribbon) in RIBBONS.iter().enumerate() {
        let position = |x: f32, y: f32| {
            let motion = 0.012
                * ((phase + x * 5. + index as f32 * 0.7).sin()
                    - (x * 5. + index as f32 * 0.7).sin());
            bounds.origin + point(bounds.size.width * x, bounds.size.height * (y + motion))
        };
        // Keep each ribbon's translucent gradient associated with its geometry.
        let color = ribbon.color;
        let mut brush = Brush::mesh([
            [
                color[0].min(0.75),
                (color[1] + 0.14).min(0.90),
                (color[2] + 0.10).min(1.),
                ribbon.alpha * 0.40,
            ],
            [
                (color[0] + 0.25).min(1.),
                color[1] * 0.82,
                (color[2] + 0.03).min(1.),
                ribbon.alpha * 0.65,
            ],
            [color[0] * 0.65, color[1] * 0.93, color[2], ribbon.alpha],
            [
                (color[0] + 0.13).min(1.),
                color[1] * 0.67,
                color[2] * 0.94,
                ribbon.alpha * 0.90,
            ],
        ])
        .unwrap();
        if grain {
            brush = brush
                .with_noise(Noise::dither(0.035), BlendMode::SoftLight)
                .unwrap();
        }
        // Native coverage reuses brush programs as the geometry animates;
        // specializing direct shaders per moving ribbon would cause compilation churn.
        let path = |fraction_a: f32, fraction_b: f32| {
            let boundary = |x: f32, fraction: f32| {
                let top = edge_y(ribbon.top, x);
                let bottom = edge_y(ribbon.bottom, x);
                position(x, top + (bottom - top) * fraction)
            };
            let mut p = PathBuilder::fill();
            p.move_to(boundary(0., fraction_a));
            for step in 1..=256 {
                p.line_to(boundary(step as f32 / 256., fraction_a));
            }
            for step in (0..=256).rev() {
                p.line_to(boundary(step as f32 / 256., fraction_b));
            }
            p.close();
            p.build().unwrap()
        };
        brush.paint_path_native(window, path(0., 1.)).unwrap();
        // Build a separate variable-width strip along the actual edge. Offsets
        // follow its screen-space normal so steep folds do not appear thinner.
        if index >= 5 {
            let crest = [0.18, 0.67, 0.69, 0.35][index - 5];
            let strength = |x: f32| {
                let e = 0.008;
                let bend = (edge_y(ribbon.top, x + e) - 2. * edge_y(ribbon.top, x)
                    + edge_y(ribbon.top, x - e))
                .abs()
                    / (e * e);
                let curvature = bend / (bend + 22.);
                let fold = (-0.5 * ((x - crest) / 0.105).powi(2)).exp();
                (0.68 * fold + 0.32 * curvature).clamp(0., 1.)
            };
            // Overlapping translucent veils follow the ribbon's own cross-section.
            // Their accumulated opacity falls smoothly from the rim into the fill,
            // unlike a screen-space gradient or a blurred outline outside the sheet.
            for layer in (1..=24).rev() {
                let t = layer as f32 / 24.;
                let shade_edge = |x: f32| {
                    let top = edge_y(ribbon.top, x);
                    let thickness = (edge_y(ribbon.bottom, x) - top).max(0.);
                    let depth = (0.12 + 0.43 * strength(x)) * t;
                    position(x, top + thickness * depth)
                };
                let mut veil = PathBuilder::fill();
                veil.move_to(position(0., edge_y(ribbon.top, 0.)));
                for step in 1..=256 {
                    let x = step as f32 / 256.;
                    veil.line_to(position(x, edge_y(ribbon.top, x)));
                }
                for step in (0..=256).rev() {
                    veil.line_to(shade_edge(step as f32 / 256.));
                }
                veil.close();
                // Difference of the desired exponential profile yields each
                // veil's contribution; the widest, faintest layers draw first.
                let profile =
                    |v: f32| ((-3.5 * v).exp() - (-3.5_f32).exp()) / (1. - (-3.5_f32).exp());
                let weight = profile((layer - 1) as f32 / 24.) - profile(t);
                let points = (0..=12)
                    .map(|step| {
                        let x = step as f32 / 12.;
                        let dark = strength(x);
                        let cyan = [0.20, 0.43, 0.70];
                        let violet = [0.43, 0.19, 0.60];
                        let color: [f32; 3] =
                            std::array::from_fn(|i| cyan[i] * (1. - x) + violet[i] * x);
                        // Optical-depth accumulation keeps the center rich while
                        // the last layers disappear into the translucent material.
                        let alpha = 1. - (-weight * (0.32 + 0.95 * dark)).exp();
                        BrushPoint::new([x, 0.5], [color[0], color[1], color[2], alpha])
                    })
                    .collect();
                Brush::freeform(points)
                    .unwrap()
                    .paint_path_native(window, veil.build().unwrap())
                    .unwrap();
            }
            let edge = |x: f32, side: f32| {
                let e = 0.001;
                let lo = (x - e).max(0.);
                let hi = (x + e).min(1.);
                let a = position(lo, edge_y(ribbon.top, lo));
                let b = position(hi, edge_y(ribbon.top, hi));
                let dx = f32::from(b.x - a.x);
                let dy = f32::from(b.y - a.y);
                let length = dx.hypot(dy).max(0.001);
                let width = 0.35 + 2.25 * strength(x);
                position(x, edge_y(ribbon.top, x))
                    + point(
                        px(-dy / length * width * side),
                        px(dx / length * width * side),
                    )
            };
            let mut rim = PathBuilder::fill();
            rim.move_to(edge(0., -0.2));
            for step in 1..=256 {
                rim.line_to(edge(step as f32 / 256., -0.2));
            }
            for step in (0..=256).rev() {
                rim.line_to(edge(step as f32 / 256., 0.8));
            }
            rim.close();
            // Opacity and pigment vary independently of width. Closely spaced
            // field points keep the strongest dark accents local to each fold.
            let colors = (0..=12)
                .map(|step| {
                    let x = step as f32 / 12.;
                    let dark = strength(x);
                    let cyan = [0.18, 0.46, 0.66];
                    let rose = [0.63, 0.20, 0.53];
                    let color: [f32; 3] = std::array::from_fn(|i| {
                        (cyan[i] * (1. - x) + rose[i] * x) * (1. - dark * 0.28)
                    });
                    BrushPoint::new([x, 0.5], [color[0], color[1], color[2], 0.12 + 0.65 * dark])
                })
                .collect();
            Brush::freeform(colors)
                .unwrap()
                .paint_path_native(window, rim.build().unwrap())
                .unwrap();
        }
        let lip = Brush::solid([0.98, 0.98, 1., if index >= 5 { 0.10 } else { 0.20 }]).unwrap();
        lip.paint_path_native(window, path(0.006, 0.016)).unwrap();
        if grain {
            let thread = Brush::solid([1., 0.96, 1., 0.045]).unwrap();
            for f in [0.15, 0.32, 0.52, 0.73, 0.88] {
                thread
                    .paint_path_native(window, path(f, f + 0.004))
                    .unwrap();
            }
        }
    }
}
