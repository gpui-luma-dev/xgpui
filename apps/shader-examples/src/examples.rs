use gpui::{prelude::*, *};
use web_time::Instant;
use xgpui_shader::{
    WindowShaderExt,
    instanced::{ShaderProgram, presets::*},
};

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Minimal,
    Glow,
    Particles,
    Composition,
    Shadows,
    Borders,
}
impl Mode {
    pub fn parse(query: &str) -> Self {
        if query.contains("borders") {
            Self::Borders
        } else if query.contains("shadows") {
            Self::Shadows
        } else if query.contains("particles") {
            Self::Particles
        } else if query.contains("glow") {
            Self::Glow
        } else if query.contains("composition") {
            Self::Composition
        } else {
            Self::Minimal
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Minimal => "Minimal",
            Self::Glow => "Glow",
            Self::Particles => "Particles",
            Self::Composition => "Composition",
            Self::Shadows => "Shadows",
            Self::Borders => "Borders",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Minimal => "01 / A custom fragment shader",
            Self::Glow => "02 / One hundred glowing badges",
            Self::Particles => "03 / Instanced particle field",
            Self::Composition => "04 / Order, clipping and opacity",
            Self::Shadows => "05 / Shadow falloff and color",
            Self::Borders => "06 / Soft animated borders",
        }
    }
}
pub struct Examples {
    mode: Mode,
    pub(crate) shadow: crate::shadows::ShadowDemo,
    rounded: ShaderProgram<EffectInstance>,
    minimal: ShaderProgram<EffectInstance>,
    glow: ShaderProgram<EffectInstance>,
    particles: ShaderProgram<EffectInstance>,
    started: Instant,
    paused_at: f32,
    animated: bool,
    count: usize,
    translucent: bool,
    clip_composition: bool,
    last_frame: Instant,
    interval_ms: f32,
}
impl Examples {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            shadow: crate::shadows::ShadowDemo::new(),
            minimal: ShaderProgram::new(include_str!("minimal.wgsl")).expect("minimal shader"),
            rounded: ShaderProgram::new(ROUNDED_BOX).expect("rounded preset"),
            glow: ShaderProgram::new(PULSE_GLOW).expect("glow preset"),
            particles: ShaderProgram::new(PARTICLE).expect("particle preset"),
            started: Instant::now(),
            paused_at: 0.,
            animated: true,
            count: 5000,
            translucent: false,
            clip_composition: true,
            last_frame: Instant::now(),
            interval_ms: 0.,
        }
    }
}
fn random(seed: u32) -> f32 {
    let mut value = seed.wrapping_add(0x9e3779b9);
    value = (value ^ (value >> 16)).wrapping_mul(0x85ebca6b);
    value = (value ^ (value >> 13)).wrapping_mul(0xc2b2ae35);
    ((value ^ (value >> 16)) >> 8) as f32 / 16777216.
}
fn effect(color: [f32; 4], radius: f32, phase: f32) -> EffectInstance {
    EffectInstance {
        color,
        effect: [radius, phase, 1., 0.],
    }
}
impl Render for Examples {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.interval_ms =
            self.interval_ms * 0.9 + now.duration_since(self.last_frame).as_secs_f32() * 100.;
        self.last_frame = now;
        let t = if self.animated {
            self.started.elapsed().as_secs_f32()
        } else {
            self.paused_at
        };
        if self.animated && !matches!(self.mode, Mode::Composition | Mode::Shadows) {
            window.request_animation_frame();
        }
        let mode = self.mode;
        let clip_composition = self.clip_composition;
        let program = match mode {
            Mode::Minimal => self.minimal.clone(),
            Mode::Composition | Mode::Shadows | Mode::Borders => self.rounded.clone(),
            Mode::Glow => self.glow.clone(),
            Mode::Particles => self.particles.clone(),
        };
        let count = match mode {
            Mode::Minimal => 1,
            Mode::Glow => 100,
            Mode::Particles => self.count,
            Mode::Composition => 3,
            Mode::Shadows | Mode::Borders => 1,
        };
        let opacity = if self.translucent { 0.45 } else { 1. };
        div().size_full().bg(rgb(0x0b1020)).text_color(rgb(0xe5edff)).font_family("IBM Plex Sans")
            .flex().flex_col().gap_4().p_6()
            .child(div().flex().gap_3().children(
                [Mode::Minimal, Mode::Glow, Mode::Particles, Mode::Composition, Mode::Shadows, Mode::Borders].into_iter().map(|choice|
                    div().id(choice.label()).px_3().py_2().rounded_md()
                        .bg(rgb(if choice == mode { 0x365e9d } else { 0x202c46 }))
                        .cursor_pointer().child(choice.label())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.mode = choice;
                            this.last_frame = Instant::now();
                            this.interval_ms = 0.;
                            cx.notify();
                        })))))
            .child(div().text_sm().child(if cfg!(target_family = "wasm") { "WASM · wgpu" } else { "Local app · native renderer" }))
            .child(div().text_2xl().child(mode.title()))
            .child(if mode == Mode::Borders { "Soft blue and orange highlights orbit a neutral pill border. Normal button content stays interactive." } else if mode == Mode::Shadows { "Standard GPUI on the left; the extension helper on the right. Both cards contain normal interactive children." } else if mode == Mode::Composition { "Compare the same overlapping shapes with clipping on and off. The cyan frame marks the clip boundary." } else { "WGSL → typed instances → GPUI's main render pass. No per-instance textures." })
            .child(div().flex().flex_wrap().gap_3()
                .when(!matches!(mode, Mode::Composition | Mode::Shadows), |el| el.child(div().id("pause").p_2().bg(rgb(0x263455)).cursor_pointer().child(if self.animated { "Pause" } else { "Resume" })
                    .on_click(cx.listener(|this, _, _, cx| { if this.animated { this.paused_at = this.started.elapsed().as_secs_f32(); }
                        else { this.started = Instant::now() - std::time::Duration::from_secs_f32(this.paused_at); }
                        this.animated = !this.animated; cx.notify(); }))))
                .child(div().id("opacity").p_2().bg(rgb(0x263455)).cursor_pointer().child(if self.translucent { "Opacity 45%" } else { "Opacity 100%" })
                    .on_click(cx.listener(|this, _, _, cx| { this.translucent = !this.translucent; cx.notify(); })))
                .when(mode == Mode::Composition, |el| el.child(
                    div().id("composition-clip").p_2().bg(rgb(if clip_composition { 0x155e75 } else { 0x263455 })).cursor_pointer()
                        .child(if clip_composition { "Clipping: ON" } else { "Clipping: OFF" })
                        .on_click(cx.listener(|this, _, _, cx| { this.clip_composition = !this.clip_composition; cx.notify(); }))))
                .when(mode == Mode::Shadows, |el| el
                    .child(div().id("shadow-card-opacity").p_2().bg(rgb(0x263455)).cursor_pointer().child(if self.shadow.card_dim { "Card: 50%" } else { "Card: 100%" })
                        .on_click(cx.listener(|this, _, _, cx| { this.shadow.card_dim = !this.shadow.card_dim; cx.notify(); })))
                    .child(div().id("shadow-gradient").p_2().bg(rgb(0x263455)).cursor_pointer().child(if self.shadow.gradient { "Gradient: ON" } else { "Gradient: OFF" })
                        .on_click(cx.listener(|this, _, _, cx| { this.shadow.gradient = !this.shadow.gradient; cx.notify(); })))
                    .child(div().id("shadow-inset").p_2().bg(rgb(0x263455)).cursor_pointer().child(if self.shadow.inset { "Inset" } else { "Outset" })
                        .on_click(cx.listener(|this, _, _, cx| { this.shadow.inset = !this.shadow.inset; cx.notify(); })))
                    .child(div().id("shadow-blur").p_2().bg(rgb(0x263455)).cursor_pointer().child(format!("Blur: {}", self.shadow.blur))
                        .on_click(cx.listener(|this, _, _, cx| { this.shadow.blur = match this.shadow.blur as u32 { 0 => 8., 8 => 24., 24 => 40., _ => 0. }; cx.notify(); })))
                    .child(div().id("shadow-spread").p_2().bg(rgb(0x263455)).cursor_pointer().child(format!("Spread: {}", self.shadow.spread))
                        .on_click(cx.listener(|this, _, _, cx| { this.shadow.spread = if this.shadow.spread < 0. { 0. } else if this.shadow.spread == 0. { 8. } else { -4. }; cx.notify(); })))
                    .child(div().id("shadow-falloff").p_2().bg(rgb(0x263455)).cursor_pointer().child(format!("Falloff: {}", self.shadow.falloff))
                        .on_click(cx.listener(|this, _, _, cx| { this.shadow.falloff = if this.shadow.falloff < 1. { 1. } else if this.shadow.falloff == 1. { 2. } else if this.shadow.falloff == 2. { 4. } else { 0.5 }; cx.notify(); })))
                    .child(div().id("shadow-offset").p_2().bg(rgb(0x263455)).cursor_pointer().child(format!("Offset: {}", self.shadow.offset))
                        .on_click(cx.listener(|this, _, _, cx| { this.shadow.offset = match this.shadow.offset as u32 { 0 => 12., 12 => 24., _ => 0. }; cx.notify(); })))
                )
                .when(mode == Mode::Particles, |el| el.children([100usize, 1000, 5000, 10000].into_iter().map(|n|
                    div().id(ElementId::Name(format!("count-{n}").into())).p_2().bg(rgb(0x263455)).cursor_pointer().child(n.to_string())
                        .on_click(cx.listener(move |this, _, _, cx| { this.count = n; cx.notify(); })))))
            )
            .when(!matches!(mode, Mode::Composition | Mode::Shadows | Mode::Borders), |el| el.child(format!("{count} instances · {:.1} ms redraw interval · {}", self.interval_ms, if mode == Mode::Composition { "shader runs split at the native quad" } else { "one compatible run in the effect canvas" })))
            .when(!matches!(mode, Mode::Composition | Mode::Shadows | Mode::Borders), |el| el.child("Redraw interval includes frame scheduling; it is not GPU time. Use a GPU capture to inspect draw calls."))
            .when(mode == Mode::Composition, |el| el.child(div().flex().gap_4().children([
                ("1 · Red shader", 0xff667f), ("2 · Gold native quad", 0xffdf80),
                ("3 · Green shader", 0x4de69f), ("4 · Blue shader", 0x779bff),
            ].into_iter().map(|(label, color)| div().text_color(rgb(color)).child(label)))))
            .when(mode == Mode::Shadows, |el| el.child(div().flex()
                .child(div().w_1_2().child("Native GPUI · one color / fixed falloff"))
                .child(div().w_1_2().child("Extension helper · gradient shadow / normal children"))))
            .when(mode == Mode::Borders, |el| el.child(div().w_full().flex_1().min_h(px(240.)).bg(rgb(0xf8f9fb)).flex().flex_col().items_center().justify_center().gap_6().text_color(rgb(0x22304a))
                .child(div().opacity(opacity).child(self.shadow.border_button(t / 2.5, cx)))
                .child("Blurred highlights · 2.5s rotation · click Generate")
                .child("Pause freezes motion; Resume continues from the same phase.")))
            .when(mode == Mode::Shadows, |el| el.child(self.shadow.render(opacity, cx)))
            .when(!matches!(mode, Mode::Shadows | Mode::Borders), |el| el.child(div().id("effect-clip").w_full().flex_1().min_h(px(180.)).overflow_hidden().bg(rgb(if mode == Mode::Shadows { 0xe6ebf3 } else { 0x121c32 })).opacity(opacity)
                .child(canvas(move |_, _, _| (), move |bounds, _, window, _| {
                    if mode == Mode::Composition {
                        paint_composition(bounds, &program, clip_composition, window);
                        return;
                    }
                    let width = f32::from(bounds.size.width); let height = f32::from(bounds.size.height);
                    for i in 0..count {
                        let fi = i as f32;
                        let (x, y, w, h, tint) = match mode {
                            Mode::Minimal => (width * 0.2, height * 0.25, width * 0.6, height * 0.5, [0.25,0.65,1.,1.]),
                            Mode::Glow => { let w = width / 10.; let h = height / 10.;
                                ((i % 10) as f32 * w, (i / 10) as f32 * h, w, h, [0.3 + 0.2 * (fi * 0.3).sin(),0.55,1.,1.]) },
                            Mode::Particles => { let x = ((random(i as u32) + t * 0.025) % 1.) * width;
                                let y = ((random((i as u32).wrapping_add(123456)) - t * 0.06).rem_euclid(1.)) * height;
                                (x - 6., y - 6., 12., 12., [0.4 + 0.4 * (fi * 0.7).sin(),0.65,1.,0.8]) },
                            Mode::Composition | Mode::Shadows | Mode::Borders => unreachable!(),
                        };
                        let rect = Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w), px(h)));
                        window.paint_custom_shader(rect, &program, effect(tint, 12., t * 2. + fi * 0.1)).expect("finite typed shader instance");

                    }
                }).size_full())))
            .when(mode == Mode::Shadows, |el| el.child("Click either child button to verify interaction. Card opacity affects each card and its shadow; Opacity affects the whole panel. Analytic softness differs from CSS blur."))
            .when(mode == Mode::Composition, |el| el.child(if clip_composition {
                "Clipping ON · Shapes stop at the cyan frame. Switch it off to reveal their full outlines; their positions stay fixed."
            } else {
                "Clipping OFF · Shapes extend beyond the cyan frame. Gold covers red; green and blue cover gold where they overlap."
            }))
    }
}

/// Keep the full geometry inside the canvas; only the inset cyan frame is optional clipping.
fn paint_composition(
    bounds: Bounds<Pixels>,
    program: &ShaderProgram<EffectInstance>,
    clipped: bool,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let clip = Bounds::new(
        bounds.origin + point(px(width * 0.12), px(height * 0.16)),
        size(px(width * 0.72), px(height * 0.60)),
    );
    window.paint_quad(fill(clip, rgb(0x1c2b46)));
    let geometry = |x: f32, y: f32, w: f32, h: f32| {
        Bounds::new(
            clip.origin + point(clip.size.width * x, clip.size.height * y),
            size(clip.size.width * w, clip.size.height * h),
        )
    };
    window.with_content_mask(clipped.then_some(ContentMask { bounds: clip }), |window| {
        window
            .paint_custom_shader(
                geometry(0.04, 0.12, 0.56, 0.76),
                program,
                effect([1., 0.2, 0.3, 0.65], 12., 0.),
            )
            .unwrap();
        window.paint_quad(fill(geometry(0.36, -0.12, 0.025, 1.40), rgb(0xffdf80)));
        window
            .paint_custom_shader(
                geometry(0.30, 0.25, 0.56, 0.96),
                program,
                effect([0.15, 0.9, 0.55, 0.65], 12., 0.),
            )
            .unwrap();
        window
            .paint_custom_shader(
                geometry(0.56, -0.10, 0.64, 0.90),
                program,
                effect([0.3, 0.5, 1., 0.65], 12., 0.),
            )
            .unwrap();
    });
    // Draw the boundary last so it remains visible over every overlap, in both modes.
    window.paint_quad(
        fill(clip, transparent_black())
            .border_widths(px(2.))
            .border_color(rgb(0x67e8f9)),
    );
}
