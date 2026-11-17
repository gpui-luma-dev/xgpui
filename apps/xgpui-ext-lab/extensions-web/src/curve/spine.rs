//! Spine inspector: a quadratic field spine with its projection field drawn
//! on the GPU by `CURVE_SPINE_WGSL` (or on the CPU in f64), the inner-bend
//! seam rule, trimming, the T/N frame and the arrowhead.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_curve::kurbo::{Affine, BezPath, ParamCurve, Point as KPoint};
use xgpui_curve::spine::{CURVE_SPINE_WGSL, CurveSpine};
use xgpui_shader::WindowShaderExt;
use xgpui_shader::instanced::{InstanceField, ShaderInstance, ShaderProgram};

#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct SpineInstance {
    a: [f32; 4],
    b: [f32; 4],
    style: [f32; 4],
}

impl ShaderInstance for SpineInstance {
    const FIELDS: &'static [InstanceField] = &[
        InstanceField {
            name: "a",
            offset: std::mem::offset_of!(Self, a),
            components: 4,
        },
        InstanceField {
            name: "b",
            offset: std::mem::offset_of!(Self, b),
            components: 4,
        },
        InstanceField {
            name: "style",
            offset: std::mem::offset_of!(Self, style),
            components: 4,
        },
    ];
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Bands,
    Texture,
    Field,
}

impl Mode {
    fn index(self) -> f32 {
        match self {
            Mode::Bands => 0.,
            Mode::Texture => 1.,
            Mode::Field => 2.,
        }
    }
}

const TRIMS: [(f64, f64); 4] = [(0., 1.), (0.2, 0.8), (0., 0.5), (0.35, 1.)];
const RADIUS: f64 = 70.;

pub struct SpinePanel {
    /// Control points in 0..1 of the canvas.
    points: [KPoint; 3],
    mode: Mode,
    gpu: bool,
    limit: bool,
    trim: usize,
    frame_t: f64,
    drag: Option<usize>,
    hover: Option<KPoint>,
    area: Area,
    view: Camera,
    program: Option<Result<ShaderProgram<SpineInstance>, String>>,
    cpu: Option<CpuFrame>,
}

/// A cached CPU rendering and the state it was drawn for.
struct CpuFrame {
    points: Vec<f64>,
    camera: [f64; 6],
    bounds: Bounds<Pixels>,
    mode: Mode,
    limit: bool,
    trim: usize,
    image: Arc<RenderImage>,
}

impl SpinePanel {
    pub fn new() -> Self {
        Self {
            points: [
                KPoint::new(0.18, 0.7),
                KPoint::new(0.5, 0.05),
                KPoint::new(0.84, 0.62),
            ],
            mode: Mode::Bands,
            gpu: true,
            limit: false,
            trim: 0,
            frame_t: 0.35,
            drag: None,
            hover: None,
            area: area(),
            view: Camera::default(),
            program: None,
            cpu: None,
        }
    }

    /// The spine in pixels relative to the canvas origin, after the seam rule.
    fn spine(&self, bounds: Bounds<Pixels>) -> CurveSpine {
        spine_in(self.points, self.limit, bounds)
    }

    fn program(&mut self) -> Result<ShaderProgram<SpineInstance>, String> {
        self.program
            .get_or_insert_with(|| {
                ShaderProgram::new(&format!(
                    "{CURVE_SPINE_WGSL}\n{}",
                    include_str!("spine_field.wgsl")
                ))
                .map_err(|e| format!("{e:#}"))
            })
            .clone()
    }

    /// CPU rendering of the same field, at half resolution, cached per state.
    fn cpu_image(&mut self, bounds: Bounds<Pixels>) -> Option<Arc<RenderImage>> {
        let key: Vec<f64> = self.points.iter().flat_map(|p| [p.x, p.y]).collect();
        if let Some(frame) = &self.cpu
            && frame.points == key
            && frame.bounds == bounds
            && frame.mode == self.mode
            && frame.limit == self.limit
            && frame.trim == self.trim
            && frame.camera == self.view.around(bounds).as_coeffs()
        {
            return Some(frame.image.clone());
        }
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        if w < 2. || h < 2. {
            return None;
        }
        let (iw, ih) = ((w / 2.).ceil() as u32, (h / 2.).ceil() as u32);
        let spine = self.spine(bounds);
        let (lo, hi) = TRIMS[self.trim];
        // Canvas-local pixel → unzoomed spine space.
        let o = kp(bounds.origin).to_vec2();
        let inverse =
            (Affine::translate(-o) * self.view.around(bounds) * Affine::translate(o)).inverse();
        let mut bytes = Vec::with_capacity((iw * ih * 4) as usize);
        for y in 0..ih {
            for x in 0..iw {
                let p = inverse * KPoint::new(x as f64 * 2. + 1., y as f64 * 2. + 1.);
                let hit = spine.project_range(p, lo, hi);
                let [r, g, b] = shade(self.mode, hit.distance, hit.s);
                // BGRA, as GPUI render images expect.
                bytes.extend([b, g, r].map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
                bytes.push(255);
            }
        }
        let buffer = image::ImageBuffer::from_raw(iw, ih, bytes)?;
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        self.cpu = Some(CpuFrame {
            points: key,
            camera: self.view.around(bounds).as_coeffs(),
            bounds,
            mode: self.mode,
            limit: self.limit,
            trim: self.trim,
            image: image.clone(),
        });
        Some(image)
    }
}

fn spine_in(points: [KPoint; 3], limit: bool, bounds: Bounds<Pixels>) -> CurveSpine {
    let (w, h) = (
        f64::from(f32::from(bounds.size.width)),
        f64::from(f32::from(bounds.size.height)),
    );
    let px = |p: KPoint| KPoint::new(p.x * w, p.y * h);
    let mut spine =
        CurveSpine::new(px(points[0]), px(points[1]), px(points[2])).expect("finite spine");
    if limit {
        spine.limit_bend(RADIUS);
    }
    spine
}

/// CPU mirror of `spine_field.wgsl`.
fn shade(mode: Mode, d: f64, along: f64) -> [f64; 3] {
    let mix = |a: [f64; 3], b: [f64; 3], t: f64| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    match mode {
        Mode::Bands => {
            let side = if d > 0. {
                [1., 0.55, 0.25]
            } else {
                [0.25, 0.55, 1.]
            };
            let k = (1. - (-d.abs() / 90.).exp()) * (0.78 + 0.22 * (d.abs() * 0.35).cos());
            side.map(|c| c * k)
        }
        Mode::Texture => {
            let cells = (along * 12.).floor() + (d / 22.).floor();
            let parity = cells - 2. * (cells * 0.5).floor();
            let c = mix([0.93, 0.80, 0.45], [0.18, 0.30, 0.52], parity);
            let fade = (-d.abs() / (3. * RADIUS)).exp();
            c.map(|v| v * fade)
        }
        Mode::Field => {
            let w = (-(d * d) / (RADIUS * RADIUS) - ((along - 0.5) / 0.6).powi(2)).exp();
            mix(
                [0.03, 0.04, 0.07],
                mix([1., 0.35, 0.45], [0.3, 0.8, 1.], along),
                w,
            )
        }
    }
}

pub fn render(lab: &mut CurveLab, window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let panel = &mut lab.spine;
    let bounds = panel.area.get();
    let spine = panel.spine(bounds);
    let (lo, hi) = TRIMS[panel.trim];
    let program = panel.program();
    let cpu = if panel.gpu {
        None
    } else {
        panel.cpu_image(bounds)
    };
    let view = panel.view;
    let (mode, gpu, limit, frame_t, points) = (
        panel.mode,
        panel.gpu,
        panel.limit,
        panel.frame_t,
        panel.points,
    );
    let hover = panel.hover.map(|h| spine.project_range(h, lo, hi));
    if bounds.size.width <= px(0.) {
        // No layout yet: draw once more with real bounds for the readout and CPU image.
        window.request_animation_frame();
    }

    let mode_button = |m: Mode, label: &'static str, cx: &mut Context<CurveLab>| {
        button(label, mode == m).on_click(cx.listener(move |this, _, _, cx| {
            this.spine.mode = m;
            cx.notify();
        }))
    };
    let renderer = match (&program, gpu) {
        (Err(e), true) => format!("GPU program failed: {e}"),
        (_, true) => "Renderer: GPU (CURVE_SPINE_WGSL)".into(),
        (_, false) => "Renderer: CPU f64 (half resolution)".into(),
    };
    let controls = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(mode_button(Mode::Bands, "Signed distance", cx))
        .child(mode_button(Mode::Texture, "(s, perp) texture", cx))
        .child(mode_button(Mode::Field, "Bent field", cx))
        .child(
            button(renderer, gpu).on_click(cx.listener(|this, _, _, cx| {
                this.spine.gpu = !this.spine.gpu;
                cx.notify();
            })),
        )
        .child(
            button(format!("Seam rule: reach ≥ {RADIUS}px"), limit).on_click(cx.listener(
                |this, _, _, cx| {
                    this.spine.limit = !this.spine.limit;
                    cx.notify();
                },
            )),
        )
        .child(
            button(format!("Trim {lo:.2}–{hi:.2}"), panel.trim != 0).on_click(cx.listener(
                |this, _, _, cx| {
                    this.spine.trim = (this.spine.trim + 1) % TRIMS.len();
                    cx.notify();
                },
            )),
        )
        .child(
            button(format!("Frame at t = {frame_t:.2}"), false).on_click(cx.listener(
                |this, _, _, cx| {
                    this.spine.frame_t = next_frame_t(this.spine.frame_t);
                    cx.notify();
                },
            )),
        )
        .child(view_button(cx, view, |lab: &mut CurveLab| {
            &mut lab.spine.view
        }));
    let reach = spine.reach();
    let readout = format!(
        "Length {:.1}px · max curvature {:.4}/px · reach {} · {}",
        spine.length(),
        spine.max_curvature(),
        if reach.is_finite() {
            format!("{reach:.1}px")
        } else {
            "∞".into()
        },
        match hover {
            Some(h) => format!(
                "pointer: t {:.3}, s {:.3}, perp {:+.1}px",
                h.t, h.s, h.distance
            ),
            None => "hover the field for t, s and perpendicular distance".into(),
        }
    );
    let surface = interactive(
        "spine-surface",
        cx,
        &lab.spine.area,
        |lab: &mut CurveLab| &mut lab.spine.view,
        |this: &mut CurveLab, at, _, _| {
            let b = this.spine.area.get();
            let t = this.spine.view.around(b) * unit(b);
            this.spine.drag = (0..3).min_by(|&a, &c| {
                (t * this.spine.points[a])
                    .distance(kp(at))
                    .total_cmp(&(t * this.spine.points[c]).distance(kp(at)))
            });
            if let Some(i) = this.spine.drag
                && (t * this.spine.points[i]).distance(kp(at)) > 18.
            {
                this.spine.drag = None;
            }
            this.spine.drag.is_some()
        },
        |this: &mut CurveLab, at, _| {
            if let Some(i) = this.spine.drag {
                let b = this.spine.area.get();
                let m = to_model(this.spine.view.around(b) * unit(b), at);
                this.spine.points[i] = KPoint::new(m.x.clamp(0.02, 0.98), m.y.clamp(0.02, 0.98));
            }
        },
        |this: &mut CurveLab, _| this.spine.drag = None,
    )
    .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
        let b = this.spine.area.get();
        let o = kp(b.origin).to_vec2();
        // Window → unzoomed canvas-local pixels.
        let world = (this.spine.view.around(b) * Affine::translate(o)).inverse() * kp(e.position);
        this.spine.hover = Some(world);
        cx.notify();
    }))
    .rounded_lg()
    .overflow_hidden()
    .bg(rgb(0x05070c))
    .when_some(cpu, |el, image| {
        el.child(
            img(ImageSource::Render(image))
                .absolute()
                .inset_0()
                .size_full(),
        )
    })
    .child(
        canvas(record(&lab.spine.area), move |bounds, _, window, _| {
            let spine = spine_in(points, limit, bounds);
            // Unzoomed canvas-local pixels → window, through the camera.
            let t = view.around(bounds) * Affine::translate(kp(bounds.origin).to_vec2());
            if gpu && let Ok(program) = &program {
                // Fill the visible canvas; the spine goes in camera space (canvas-local
                // pixels after pan and zoom) and the shader divides distances by the
                // zoom so the pattern scales with the view.
                let zoom = view.zoom();
                let local = Affine::translate(-kp(bounds.origin).to_vec2()) * t;
                let q = spine.quad();
                let [p0, c, p1] = [q.p0, q.p1, q.p2].map(|p| local * p);
                let instance = SpineInstance {
                    a: [p0.x as f32, p0.y as f32, c.x as f32, c.y as f32],
                    b: [p1.x as f32, p1.y as f32, lo as f32, hi as f32],
                    style: [mode.index(), RADIUS as f32, zoom as f32, 0.],
                };
                if let Err(e) = window.paint_custom_shader(bounds, program, instance) {
                    log::error!("spine field: {e:#}");
                }
            }
            let q = spine.quad();
            let mut full = BezPath::new();
            full.move_to(q.p0);
            full.quad_to(q.p1, q.p2);
            stroke(window, &full, t, 1., hsla(0., 0., 1., 0.35));
            let mut trimmed = BezPath::new();
            let part = q.subsegment(lo..hi);
            trimmed.move_to(part.p0);
            trimmed.quad_to(part.p1, part.p2);
            contrast(window, &trimmed, t, 2.5);
            let mut polygon = line(q.p0, q.p1);
            polygon.line_to(q.p2);
            stroke(window, &polygon, t, 1., hsla(0.6, 0.6, 0.8, 0.6));
            if let Some([l, tip, r]) = spine.arrowhead(16., 0.45) {
                let mut head = line(l, tip);
                head.line_to(r);
                contrast(window, &head, t, 2.5);
            }
            // Frame at frame_t.
            let seg = xgpui_curve::kurbo::PathSeg::Quad(q);
            let p = q.eval(frame_t);
            if let (Some(tan), Some(nrm)) = (
                xgpui_curve::geom::tangent(seg, frame_t),
                xgpui_curve::geom::normal(seg, frame_t),
            ) {
                stroke(window, &line(p, p + tan * 40.), t, 2.5, rgb(0xff5a5a));
                stroke(window, &line(p, p + nrm * 40.), t, 2.5, rgb(0x5aff8a));
                dot(window, t * p, 4., rgb(0xffffff));
            }
            for (i, c) in [q.p0, q.p1, q.p2].into_iter().enumerate() {
                handle(
                    window,
                    t * c,
                    if i == 1 { 5. } else { 7. },
                    if i == 1 { rgb(0x9ecbff) } else { rgb(0xffffff) },
                );
            }
            for e in [q.eval(lo), q.eval(hi)] {
                dot(window, t * e, 4., rgb(0xffa040));
            }
            if let Some(h) = hover {
                dot(window, t * h.point, 3., rgb(0xffe066));
            }
        })
        .absolute()
        .inset_0()
        .size_full(),
    );
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(controls)
        .child(div().text_color(rgb(0x9fb0cc)).child(readout))
        .child(div().flex_1().min_h_0().child(surface))
        .into_any_element()
}

/// The next frame position: steps of 0.15 that stop at t = 1 before wrapping to 0.
fn next_frame_t(t: f64) -> f64 {
    if t >= 1. {
        0.
    } else {
        (((t + 0.15) * 100.).round() / 100.).min(1.)
    }
}
