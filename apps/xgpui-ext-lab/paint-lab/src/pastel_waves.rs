//! A layered silk-ribbon composition using the public brush and path APIs.
use gpui::{prelude::*, *};
use std::{rc::Rc, time::Instant};
use xgpui_shader::{
    BlendMode, Brush, BrushPoint, BrushStop, Noise, WindowShaderExt,
    instanced::{InstanceField, ShaderInstance, ShaderProgram},
    painted,
};

// Native producers, chosen at build time by target: Metal on macOS, D3D11 on
// Windows, wgpu on Linux (and under the headless wgpu renderer).
#[cfg(target_os = "windows")]
mod dx11;
// Used by the D3D11 producer; elsewhere only its translation test runs.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod hlsl;
#[cfg(target_os = "macos")]
mod metal;
mod native;
mod wgpu_producer;

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
    /// Which implementation draws the composition.
    renderer: Renderer,
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
            renderer: Renderer::default(),
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
                        super::button(
                            "waves-renderer",
                            format!("Renderer: {}", self.renderer.label()),
                        )
                        .text_color(rgb(0xffffff))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.renderer = this.renderer.next();
                            cx.notify();
                        })),
                    )
                    .child(
                        super::button("waves-reset", "Reset")
                            .text_color(rgb(0xffffff))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let renderer = this.renderer;
                                *this = Self::default();
                                this.renderer = renderer;
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
                            self.renderer,
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
                                "Frame {} · {} · {} · {}",
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
                                },
                                self.renderer.label()
                            )),
                    ),
            )
    }
}

/// Interchangeable implementations of the same composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    /// This build's native producer (Metal, D3D11 or wgpu): per-column
    /// compute pass plus one fragment pass.
    Native,
    /// One portable WGSL instanced shader draw.
    Wgsl,
    /// The original brush/path reference implementation.
    Paths,
}
impl Default for Renderer {
    fn default() -> Self {
        // Runtime-verified on macOS; elsewhere the native producer is one
        // click away until verified on Windows and Linux hardware.
        if cfg!(target_os = "macos") {
            Self::Native
        } else {
            Self::Wgsl
        }
    }
}
impl Renderer {
    fn label(self) -> &'static str {
        match self {
            Self::Native => native::BACKEND,
            Self::Wgsl => "wgsl",
            Self::Paths => "paths",
        }
    }
    fn next(self) -> Self {
        match self {
            Self::Native => Self::Wgsl,
            Self::Wgsl => Self::Paths,
            Self::Paths => Self::Native,
        }
    }
}

pub fn composition(phase: f32, grain: bool, renderer: Renderer) -> impl IntoElement + Styled {
    painted()
        .relative()
        .w_full()
        .overflow_hidden()
        .bg_brush(background_brush())
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        if renderer == Renderer::Native {
                            paint_waves_native(bounds, phase, grain, window);
                            return;
                        }
                        if renderer != Renderer::Paths
                            && paint_waves_shader(bounds, phase, grain, window)
                        {
                            return;
                        }
                        let brushes = brushes();
                        paint_background_fields(&brushes, bounds, window);
                        paint_ribbons(&brushes, bounds, phase, grain, window);
                    });
                },
            )
            .size_full(),
        )
}

/// Boundary samples per ribbon edge and translucent veils per foreground ribbon.
const STEPS: usize = 256;
const VEIL_LAYERS: usize = 24;

/// Every brush in the composition is immutable and independent of `phase`;
/// only geometry moves. Freeform fields with more than three points specialize
/// a shader program, so rebuilding the ~100 veil/rim fields every frame used to
/// recompile shaders continuously. Build them once per thread instead.
struct WaveBrushes {
    fields: Vec<Brush>,
    ribbons: Vec<RibbonPaint>,
    thread: Brush,
}
struct RibbonPaint {
    fill: Brush,
    fill_grain: Brush,
    /// Widest, faintest layer first; empty for background ribbons.
    veils: Vec<Brush>,
    rim: Option<Brush>,
    lip: Brush,
    /// Static edge positions and fold strength at `step / STEPS`.
    samples: Vec<Sample>,
}
struct Sample {
    top: f32,
    bottom: f32,
    strength: f32,
}
const BACKGROUND_POINTS: [([f32; 2], [f32; 4]); 5] = [
    ([0.03, 0.08], [0.95, 0.90, 0.99, 1.]),
    ([0.58, 0.02], [1., 0.98, 0.96, 1.]),
    ([0.98, 0.20], [1., 0.95, 0.87, 1.]),
    ([0.07, 0.96], [0.73, 0.91, 1., 1.]),
    ([0.96, 0.94], [0.95, 0.76, 0.96, 1.]),
];
const FOLD_CRESTS: [f32; 4] = [0.18, 0.67, 0.69, 0.35];
thread_local! {
    // Path-mode brushes are only built when that renderer is first used.
    static BRUSHES: Rc<WaveBrushes> = Rc::new(WaveBrushes::new());
    static BACKGROUND: Brush = Brush::freeform(
        BACKGROUND_POINTS
            .iter()
            .map(|&(position, color)| BrushPoint::new(position, color))
            .collect(),
    )
    .unwrap();
    static WAVES_PROGRAM: Option<ShaderProgram<WavesParams>> =
        match ShaderProgram::new(&waves_source()) {
            Ok(program) => Some(program),
            Err(error) => {
                eprintln!("pastel waves shader failed to compile; using paths: {error:#}");
                None
            }
        };
}
fn background_brush() -> Brush {
    BACKGROUND.with(Clone::clone)
}
fn brushes() -> Rc<WaveBrushes> {
    BRUSHES.with(Rc::clone)
}

/// One radial tint field: normalized center, normalized radii, RGB color and
/// peak alpha.
type Field = ([f32; 2], [f32; 2], [f32; 3], f32);

// Fixed, overlapping veils make the pale background carry color even above
// the ribbons. Each radial field fades completely into the underlying surface.
const FIELDS: [Field; 9] = [
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

impl WaveBrushes {
    fn new() -> Self {
        let fields = FIELDS
            .iter()
            .map(|&(center, radii, color, alpha)| {
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
            })
            .collect();
        let ribbons = RIBBONS
            .iter()
            .enumerate()
            .map(|(index, ribbon)| RibbonPaint::new(index, ribbon))
            .collect();
        Self {
            fields,
            ribbons,
            thread: Brush::solid([1., 0.96, 1., 0.045]).unwrap(),
        }
    }
}

impl RibbonPaint {
    fn new(index: usize, ribbon: &Ribbon) -> Self {
        // Keep each ribbon's translucent gradient associated with its geometry.
        let color = ribbon.color;
        let fill = Brush::mesh([
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
        let fill_grain = fill
            .clone()
            .with_noise(Noise::dither(0.035), BlendMode::SoftLight)
            .unwrap();
        let foreground = index >= 5;
        let strength = |x: f32| fold_strength(ribbon, index, x);
        let samples = (0..=STEPS)
            .map(|step| {
                let x = step as f32 / STEPS as f32;
                Sample {
                    top: edge_y(ribbon.top, x),
                    bottom: edge_y(ribbon.bottom, x),
                    strength: if foreground { strength(x) } else { 0. },
                }
            })
            .collect();
        let mut veils = Vec::new();
        let mut rim = None;
        if foreground {
            // Overlapping translucent veils follow the ribbon's own cross-section.
            // Their accumulated opacity falls smoothly from the rim into the fill,
            // unlike a screen-space gradient or a blurred outline outside the sheet.
            for layer in (1..=VEIL_LAYERS).rev() {
                let t = layer as f32 / VEIL_LAYERS as f32;
                // Difference of the desired exponential profile yields each
                // veil's contribution; the widest, faintest layers draw first.
                let profile =
                    |v: f32| ((-3.5 * v).exp() - (-3.5_f32).exp()) / (1. - (-3.5_f32).exp());
                let weight = profile((layer - 1) as f32 / VEIL_LAYERS as f32) - profile(t);
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
                veils.push(Brush::freeform(points).unwrap());
            }
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
            rim = Some(Brush::freeform(colors).unwrap());
        }
        Self {
            fill,
            fill_grain,
            veils,
            rim,
            lip: Brush::solid([0.98, 0.98, 1., if foreground { 0.10 } else { 0.20 }]).unwrap(),
            samples,
        }
    }
}

fn paint_background_fields(brushes: &WaveBrushes, bounds: Bounds<Pixels>, window: &mut Window) {
    for field in &brushes.fields {
        field
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
/// Fold emphasis along a foreground ribbon's top edge: a Gaussian crest plus
/// local curvature. Depends only on static ribbon data.
fn fold_strength(ribbon: &Ribbon, index: usize, x: f32) -> f32 {
    let crest = FOLD_CRESTS[index - 5];
    let e = 0.008;
    let bend = (edge_y(ribbon.top, x + e) - 2. * edge_y(ribbon.top, x)
        + edge_y(ribbon.top, x - e))
    .abs()
        / (e * e);
    let curvature = bend / (bend + 22.);
    let fold = (-0.5 * ((x - crest) / 0.105).powi(2)).exp();
    (0.68 * fold + 0.32 * curvature).clamp(0., 1.)
}
fn paint_ribbons(
    brushes: &WaveBrushes,
    bounds: Bounds<Pixels>,
    phase: f32,
    grain: bool,
    window: &mut Window,
) {
    for (index, (ribbon, paint)) in RIBBONS.iter().zip(&brushes.ribbons).enumerate() {
        let position = |x: f32, y: f32| {
            let motion = 0.012
                * ((phase + x * 5. + index as f32 * 0.7).sin()
                    - (x * 5. + index as f32 * 0.7).sin());
            bounds.origin + point(bounds.size.width * x, bounds.size.height * (y + motion))
        };
        let x_at = |step: usize| step as f32 / STEPS as f32;
        let boundary = |step: usize, fraction: f32| {
            let s = &paint.samples[step];
            position(x_at(step), s.top + (s.bottom - s.top) * fraction)
        };
        // Append one closed band between two fractions of the ribbon's thickness.
        let band = |p: &mut PathBuilder, fraction_a: f32, fraction_b: f32| {
            p.move_to(boundary(0, fraction_a));
            for step in 1..=STEPS {
                p.line_to(boundary(step, fraction_a));
            }
            for step in (0..=STEPS).rev() {
                p.line_to(boundary(step, fraction_b));
            }
            p.close();
        };
        let path = |fraction_a: f32, fraction_b: f32| {
            let mut p = PathBuilder::fill();
            band(&mut p, fraction_a, fraction_b);
            p.build().unwrap()
        };
        // Native coverage reuses brush programs as the geometry animates;
        // specializing direct shaders per moving ribbon would cause compilation churn.
        let fill = if grain { &paint.fill_grain } else { &paint.fill };
        fill.paint_path_native(window, path(0., 1.)).unwrap();
        // Build a separate variable-width strip along the actual edge. Offsets
        // follow its screen-space normal so steep folds do not appear thinner.
        if let Some(rim_brush) = &paint.rim {
            for (layer_index, veil_brush) in paint.veils.iter().enumerate() {
                let t = (VEIL_LAYERS - layer_index) as f32 / VEIL_LAYERS as f32;
                let mut veil = PathBuilder::fill();
                veil.move_to(boundary(0, 0.));
                for step in 1..=STEPS {
                    veil.line_to(boundary(step, 0.));
                }
                for step in (0..=STEPS).rev() {
                    let s = &paint.samples[step];
                    let thickness = (s.bottom - s.top).max(0.);
                    let depth = (0.12 + 0.43 * s.strength) * t;
                    veil.line_to(position(x_at(step), s.top + thickness * depth));
                }
                veil.close();
                veil_brush
                    .paint_path_native(window, veil.build().unwrap())
                    .unwrap();
            }
            let edge = |step: usize, side: f32| {
                let x = x_at(step);
                let e = 0.001;
                let lo = (x - e).max(0.);
                let hi = (x + e).min(1.);
                let a = position(lo, edge_y(ribbon.top, lo));
                let b = position(hi, edge_y(ribbon.top, hi));
                let dx = f32::from(b.x - a.x);
                let dy = f32::from(b.y - a.y);
                let length = dx.hypot(dy).max(0.001);
                let s = &paint.samples[step];
                let width = 0.35 + 2.25 * s.strength;
                position(x, s.top)
                    + point(
                        px(-dy / length * width * side),
                        px(dx / length * width * side),
                    )
            };
            let mut rim = PathBuilder::fill();
            rim.move_to(edge(0, -0.2));
            for step in 1..=STEPS {
                rim.line_to(edge(step, -0.2));
            }
            for step in (0..=STEPS).rev() {
                rim.line_to(edge(step, 0.8));
            }
            rim.close();
            rim_brush
                .paint_path_native(window, rim.build().unwrap())
                .unwrap();
        }
        paint
            .lip
            .paint_path_native(window, path(0.006, 0.016))
            .unwrap();
        if grain {
            // The threads share one brush and are disjoint bands, so one path
            // with five subpaths paints the same pixels with one coverage pass.
            let mut threads = PathBuilder::fill();
            for f in [0.15, 0.32, 0.52, 0.73, 0.88] {
                band(&mut threads, f, f + 0.004);
            }
            brushes
                .thread
                .paint_path_native(window, threads.build().unwrap())
                .unwrap();
        }
    }
}

/// Same drift as `position` in `paint_ribbons`, in normalized height.
fn ribbon_motion(phase: f32, index: usize, x: f32) -> f32 {
    0.012 * ((phase + x * 5. + index as f32 * 0.7).sin() - (x * 5. + index as f32 * 0.7).sin())
}

/// WGSL constants generated from the Rust composition data, followed by the
/// single-pass shader, so both renderers share one source of truth.
fn waves_source() -> String {
    wgsl_constants() + include_str!("pastel_waves.wgsl")
}

/// The same constants followed by the native wgpu/D3D11 producer.
fn producer_source() -> String {
    wgsl_constants() + include_str!("pastel_waves/producer.wgsl")
}

fn wgsl_constants() -> String {
    fn floats(values: impl IntoIterator<Item = f32>) -> String {
        values
            .into_iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(",")
    }
    fn vectors<const N: usize>(values: impl IntoIterator<Item = [f32; N]>) -> String {
        values
            .into_iter()
            .map(|v| format!("vec{}<f32>({})", N, floats(v)))
            .collect::<Vec<_>>()
            .join(",")
    }
    let edges: Vec<f32> = RIBBONS
        .iter()
        .flat_map(|r| r.top.into_iter().chain(r.bottom))
        .collect();
    let styles = RIBBONS
        .iter()
        .map(|r| [r.color[0], r.color[1], r.color[2], r.alpha]);
    let geometry = FIELDS
        .iter()
        .map(|&(center, radii, _, _)| [center[0], center[1], radii[0], radii[1]]);
    let tints = FIELDS
        .iter()
        .map(|&(_, _, color, alpha)| [color[0], color[1], color[2], alpha]);
    let positions = BACKGROUND_POINTS.iter().map(|&(position, _)| position);
    let colors = BACKGROUND_POINTS
        .iter()
        .map(|&(_, color)| [color[0], color[1], color[2]]);
    let ribbons = RIBBONS.len();
    format!(
        "const RIB_EDGES: array<f32,{}> = array<f32,{}>({});\n\
         const RIB_STYLE: array<vec4<f32>,{ribbons}> = array<vec4<f32>,{ribbons}>({});\n\
         const FOLD_CRESTS: array<f32,4> = array<f32,4>({});\n\
         const FIELD_GEOM: array<vec4<f32>,9> = array<vec4<f32>,9>({});\n\
         const FIELD_TINT: array<vec4<f32>,9> = array<vec4<f32>,9>({});\n\
         const BG_POS: array<vec2<f32>,5> = array<vec2<f32>,5>({});\n\
         const BG_COLOR: array<vec3<f32>,5> = array<vec3<f32>,5>({});\n",
        edges.len(),
        edges.len(),
        floats(edges),
        vectors(styles),
        floats(FOLD_CRESTS),
        vectors(geometry),
        vectors(tints),
        vectors(positions),
        vectors(colors),
    )
}

/// Normalized y bounds (min, max) of each ribbon's fill path, the bounds its
/// mesh brush is evaluated in.
fn fill_bounds(phase: f32) -> [f32; 18] {
    let mut bounds = [0f32; 18];
    for (index, ribbon) in RIBBONS.iter().enumerate() {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for step in 0..=STEPS {
            let x = step as f32 / STEPS as f32;
            let drift = ribbon_motion(phase, index, x);
            for y in [edge_y(ribbon.top, x), edge_y(ribbon.bottom, x)] {
                lo = lo.min(y + drift);
                hi = hi.max(y + drift);
            }
        }
        bounds[index * 2] = lo;
        bounds[index * 2 + 1] = hi;
    }
    bounds
}

/// `Parameters` of pastel_waves.wgsl: eight `vec4<f32>` slots, `data0`..`data7`,
/// packed by `waves_instance`.
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct WavesParams {
    data: [[f32; 4]; 8],
}
impl ShaderInstance for WavesParams {
    const FIELDS: &'static [InstanceField] = &{
        const NAMES: [&str; 8] = [
            "data0", "data1", "data2", "data3", "data4", "data5", "data6", "data7",
        ];
        let mut fields = [InstanceField {
            name: "",
            offset: 0,
            components: 4,
        }; 8];
        let mut i = 0;
        while i < 8 {
            fields[i].name = NAMES[i];
            fields[i].offset = i * 16;
            i += 1;
        }
        fields
    };
}

/// Phase, grain and the per-ribbon fill bounds the mesh fill is evaluated in.
fn waves_instance(phase: f32, grain: bool) -> WavesParams {
    let mut values = [0f32; 32];
    values[0] = phase;
    values[1] = if grain { 1. } else { 0. };
    values[4..22].copy_from_slice(&fill_bounds(phase));
    WavesParams {
        data: std::array::from_fn(|i| std::array::from_fn(|j| values[i * 4 + j])),
    }
}

/// One full-bounds draw. Returns false if the program failed to compile, so
/// the caller can fall back to the path renderer.
fn paint_waves_shader(
    bounds: Bounds<Pixels>,
    phase: f32,
    grain: bool,
    window: &mut Window,
) -> bool {
    WAVES_PROGRAM.with(|program| {
        let Some(program) = program else {
            return false;
        };
        if let Err(error) = window.paint_custom_shader(bounds, program, waves_instance(phase, grain)) {
            eprintln!("pastel waves shader draw failed: {error:#}");
            return false;
        }
        true
    })
}

/// Metal constants generated from the Rust composition data, followed by the
/// native shader source.
#[cfg(target_os = "macos")]
fn metal_source() -> String {
    fn floats(values: impl IntoIterator<Item = f32>) -> String {
        values
            .into_iter()
            .map(|v| format!("{v:?}f"))
            .collect::<Vec<_>>()
            .join(",")
    }
    fn vectors<const N: usize>(values: impl IntoIterator<Item = [f32; N]>) -> String {
        values
            .into_iter()
            .map(|v| format!("float{}({})", N, floats(v)))
            .collect::<Vec<_>>()
            .join(",")
    }
    let edges: Vec<f32> = RIBBONS
        .iter()
        .flat_map(|r| r.top.into_iter().chain(r.bottom))
        .collect();
    let styles = RIBBONS
        .iter()
        .map(|r| [r.color[0], r.color[1], r.color[2], r.alpha]);
    let geometry = FIELDS
        .iter()
        .map(|&(center, radii, _, _)| [center[0], center[1], radii[0], radii[1]]);
    let tints = FIELDS
        .iter()
        .map(|&(_, _, color, alpha)| [color[0], color[1], color[2], alpha]);
    let positions = BACKGROUND_POINTS.iter().map(|&(position, _)| position);
    let colors = BACKGROUND_POINTS
        .iter()
        .map(|&(_, color)| [color[0], color[1], color[2]]);
    let ribbons = RIBBONS.len();
    format!(
        "#include <metal_stdlib>\nusing namespace metal;\n\
         constant float RIB_EDGES[{}] = {{{}}};\n\
         constant float4 RIB_STYLE[{ribbons}] = {{{}}};\n\
         constant float FOLD_CRESTS[4] = {{{}}};\n\
         constant float4 FIELD_GEOM[9] = {{{}}};\n\
         constant float4 FIELD_TINT[9] = {{{}}};\n\
         constant float2 BG_POS[5] = {{{}}};\n\
         constant float3 BG_COLOR[5] = {{{}}};\n{}",
        edges.len(),
        floats(edges),
        vectors(styles),
        floats(FOLD_CRESTS),
        vectors(geometry),
        vectors(tints),
        vectors(positions),
        vectors(colors),
        include_str!("pastel_waves/pastel_waves.metal"),
    )
}

/// One native producer snapshot per frame, composited over the full bounds.
fn paint_waves_native(bounds: Bounds<Pixels>, phase: f32, grain: bool, window: &mut Window) {
    let content: std::sync::Arc<dyn CustomContent> =
        std::sync::Arc::new(native::PastelWavesNative {
            phase,
            grain,
            scale: window.scale_factor(),
            fill_bounds: fill_bounds(phase),
        });
    window.paint_custom(
        CustomGeometry::RoundedRect {
            bounds,
            corner_radii: Corners::default(),
        },
        content,
    );
}
