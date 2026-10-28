//! Browser acceptance gallery for issue #22's public brush/layer APIs.
use gpui::{prelude::*, *};
use xgpui_shader::{
    BackdropBlur, BlendMode, Brush, BrushMeshPoint, BrushPoint, BrushStop, LayerEffect, Noise,
    painted,
};

const MODES: [(&str, BlendMode); 7] = [
    ("Normal", BlendMode::Normal),
    ("Multiply", BlendMode::Multiply),
    ("Screen", BlendMode::Screen),
    ("Overlay", BlendMode::Overlay),
    ("Color dodge", BlendMode::ColorDodge),
    ("Soft light", BlendMode::SoftLight),
    ("Linear burn", BlendMode::LinearBurn),
];
pub struct BrushDemo {
    layers: bool,
    light: bool,
    blend: usize,
    radius: usize,
    filter: usize,
    full: bool,
    native: bool,
    bent: bool,
    backdrop: bool,
    swapped: bool,
    effect: bool,
    clicks: u32,
}
impl BrushDemo {
    pub fn new(layers: bool) -> Self {
        Self {
            layers,
            light: false,
            blend: 0,
            radius: 0,
            filter: 0,
            full: false,
            native: false,
            bent: true,
            backdrop: true,
            swapped: false,
            effect: true,
            clicks: 0,
        }
    }
}
fn button(id: &'static str, text: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0x344158))
        .text_color(white())
        .cursor_pointer()
        .child(text.into())
}
fn gradient() -> Brush {
    Brush::linear(
        65.,
        vec![
            BrushStop::new(0., [0.04, 0.35, 1., 1.]),
            BrushStop::new(0.5, [0.90, 0.05, 0.55, 0.8]),
            BrushStop::new(1., [1., 0.65, 0.04, 1.]),
        ],
    )
    .unwrap()
}
fn card(label: &'static str, brush: Brush) -> impl IntoElement {
    div().flex().flex_col().gap_2().child(label).child(
        painted()
            .w(px(216.))
            .h(px(116.))
            .rounded(px(18.))
            .border_2()
            .bg_brush(brush)
            .border_brush(gradient()),
    )
}
fn sample() -> impl IntoElement {
    div()
        .relative()
        .w(px(216.))
        .h(px(116.))
        // Continuous background through the sampling halo; final output clips.
        .child(
            div()
                .absolute()
                .left(px(-64.))
                .top(px(-64.))
                .w(px(344.))
                .h(px(244.))
                .bg(rgb(0x213c54)),
        )
        .child(
            div()
                .absolute()
                .left(px(12.))
                .top(px(12.))
                .w(px(100.))
                .h(px(80.))
                .bg(rgb(0xff651f)),
        )
        .child(
            div()
                .absolute()
                .left(px(86.))
                .top(px(40.))
                .w(px(112.))
                .h(px(64.))
                .bg(rgba(0x2080ffc0)),
        )
        .child(
            div()
                .absolute()
                .left(px(22.))
                .top(px(44.))
                .text_color(white())
                .text_lg()
                .child("Subtree text"),
        )
}
impl Render for BrushDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (mode_name, mode) = MODES[self.blend];
        let mut root = div()
            .id("brush-demo")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .pb(px(112.))
            .bg(rgb(if self.light { 0xf2f4fa } else { 0x111827 }))
            .text_color(rgb(if self.light { 0x18243b } else { 0xe2e8f0 }))
            .font_family("IBM Plex Sans")
            .flex()
            .flex_col()
            .gap_4()
            .child(if self.layers {
                "Layer effects · explicit captures"
            } else {
                "Universal brushes · main-pass color fields"
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        button("brush-tab", "Brushes").on_click(cx.listener(|s, _, _, cx| {
                            s.layers = false;
                            cx.notify();
                        })),
                    )
                    .child(button("layer-tab", "Layer effects").on_click(cx.listener(
                        |s, _, _, cx| {
                            s.layers = true;
                            cx.notify();
                        },
                    )))
                    .child(
                        button("brush-bg", "Swap light / dark").on_click(cx.listener(
                            |s, _, _, cx| {
                                s.light = !s.light;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        button("brush-blend", format!("Blend: {mode_name}")).on_click(cx.listener(
                            |s, _, _, cx| {
                                s.blend = (s.blend + 1) % 7;
                                cx.notify();
                            },
                        )),
                    ),
            );
        if !self.layers {
            let mesh_colors = [
                [0.1, 0.5, 1., 1.],
                [1., 0.15, 0.4, 1.],
                [0.1, 0.9, 0.7, 1.],
                [0.8, 0.2, 1., 1.],
            ];
            let points = (0..16)
                .map(|i| {
                    let x = (i % 4) as f32 / 3.;
                    let y = (i / 4) as f32 / 3.;
                    BrushPoint::new([x, y], [x, 0.2 + 0.6 * y, 1. - x * 0.7, 1.])
                })
                .collect();
            let angle = if self.bent { 0.16 } else { 0. };
            let patch = Brush::hermite_patch(std::array::from_fn(|i| {
                BrushMeshPoint::new(
                    [(i % 2) as f32, (i / 2) as f32],
                    [
                        1.,
                        if i == 0 {
                            angle
                        } else if i == 1 {
                            -angle
                        } else {
                            0.
                        },
                    ],
                    [0., 1.],
                    mesh_colors[i],
                )
            }))
            .unwrap();
            root = root.child("Backgrounds and borders · same public Brush API; no intermediate color texture")
                .child(div().flex().flex_wrap().gap_3()
                    .child(card("Solid",Brush::solid([0.2,0.5,0.9,1.]).unwrap()))
                    .child(card("Linear · three stops",gradient()))
                    .child(card("Radial",Brush::radial([0.35,0.4],[0.75,0.8],vec![
                        BrushStop::new(0.,[1.,0.7,0.2,1.]),BrushStop::new(1.,[0.2,0.1,0.8,0.2])]).unwrap()))
                    .child(card("Freeform · 16 points",Brush::freeform(points).unwrap()))
                    .child(card("Regular mesh",Brush::mesh(mesh_colors).unwrap()))
                    .child(card("Hermite · tangent geometry",patch)))
                .child(button("brush-tangent","Toggle Hermite tangent bend").on_click(cx.listener(|s,_,_,cx|{s.bent = !s.bent;cx.notify();})))
                .child("Procedural noise · Blend above changes brush-local modulation")
                .child(div().flex().flex_wrap().gap_3().children([
                    ("Simplex",Noise::simplex(0.04,3,0.65)),
                    ("Worley F1",Noise::worley(0.045,0.7)),
                    ("Worley F2",Noise::worley_f2(0.045,0.7)),
                    ("Blue-noise dither · amplified",Noise::dither(0.3)),
                ].into_iter().map(|(label,noise)|card(label,gradient().with_noise(noise,mode).unwrap()))))
                .child("Glyph atlas masks · gradient follows the whole text receiver")
                .child(painted().w(px(440.)).h(px(88.)).flex_shrink_0().text_3xl().text_brush(gradient())
                    .child("Gradient typography\nBrushes on glyphs"))
                .child(button("brush-path",if self.native {"Polygon: native coverage"} else {"Polygon: direct coverage"})
                    .on_click(cx.listener(|s,_,_,cx|{s.native = !s.native;cx.notify();})))
                .child("Left: polygon with hole (direct/native toggle). Right: curved path (native coverage).")
                .child({let native=self.native; canvas(|_,_,_|(),move|bounds,(),window,_|{
                    for curved in [false,true] {
                        let origin=bounds.origin+point(px(if curved {240.} else {0.}),px(0.));
                        let at=|x,y|origin+point(px(x),px(y));
                        let mut p=PathBuilder::fill();p.move_to(at(8.,12.));
                        if curved {p.cubic_bezier_to(at(200.,16.),at(65.,85.),at(135.,-40.));}
                        else {p.line_to(at(200.,16.));}
                        p.line_to(at(180.,108.));p.line_to(at(20.,100.));p.close();
                        if !curved {p.move_to(at(65.,40.));p.line_to(at(65.,76.));p.line_to(at(130.,76.));p.line_to(at(130.,40.));p.close();}
                        let p=p.build().unwrap();
                        if curved||native {gradient().paint_path_native(window,p).unwrap();}
                        else {gradient().paint_path_direct(window,p).unwrap();}
                    }
                }).w(px(456.)).h(px(124.)).flex_shrink_0()});
        } else {
            let radius = [0., 4., 8., 16.][self.radius];
            let filter_names = [
                "None",
                "Grayscale",
                "Saturation",
                "Brightness",
                "Contrast",
                "Invert",
                "Opacity",
            ];
            let effect = match self.filter {
                1 => LayerEffect::grayscale(1.),
                2 => LayerEffect::saturation(2.),
                3 => LayerEffect::brightness(1.5),
                4 => LayerEffect::contrast(1.7),
                5 => LayerEffect::invert(1.),
                6 => LayerEffect::opacity(0.5),
                _ => LayerEffect::opacity(1.),
            }
            .unwrap()
            .with_blur(px(radius))
            .unwrap()
            .with_blur_quality(if self.full {
                LayerBlurQuality::FullResolution
            } else {
                LayerBlurQuality::Adaptive
            })
            .with_blend_mode(mode);
            root=root.child("Compare the same overlapping shapes and text. Blend reads the destination; filters affect the completed subtree.")
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("layer-radius",format!("Subtree blur: {radius}px")).on_click(cx.listener(|s,_,_,cx|{s.radius=(s.radius+1)%4;cx.notify();})))
                    .child(button("layer-quality",if self.full {"Full resolution"} else {"Adaptive"}).on_click(cx.listener(|s,_,_,cx|{s.full = !s.full;cx.notify();})))
                    .child(button("layer-filter",format!("Filter: {}",filter_names[self.filter])).on_click(cx.listener(|s,_,_,cx|{s.filter=(s.filter+1)%7;cx.notify();})))
                    .child(button("layer-enable",if self.effect {"Effect: on"} else {"Effect: removed"}).on_click(cx.listener(|s,_,_,cx|{s.effect = !s.effect;cx.notify();}))))
                .child(div().flex().flex_wrap().gap_4()
                    .child(div().flex().flex_col().gap_2().child("Reference · no capture")
                        .child(div().w(px(216.)).h(px(116.)).overflow_hidden().child(sample())))
                    .child(div().flex().flex_col().gap_2().child("Filtered subtree · clipped after blur")
                        .child(painted().w(px(216.)).h(px(116.)).overflow_hidden()
                            .bg_brush(Brush::linear(90.,vec![BrushStop::new(0.,[1.,0.8,0.2,1.]),BrushStop::new(1.,[0.1,0.8,0.9,1.])]).unwrap())
                            .child(painted().w(px(216.)).h(px(116.)).when(self.effect,|el|el.layer_effect(effect)).child(sample())))))
                .child(button("layer-click",format!("Refresh scene · clicks: {}",self.clicks))
                    .on_click(cx.listener(|s,_,_,cx|{s.clicks+=1;cx.notify();})))
                .child("Backdrop · source stripes blur, foreground label stays sharp")
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("backdrop-toggle",if self.backdrop {"Backdrop: on"} else {"Backdrop: off"}).on_click(cx.listener(|s,_,_,cx|{s.backdrop = !s.backdrop;cx.notify();})))
                    .child(button("backdrop-source","Swap stripe colors").on_click(cx.listener(|s,_,_,cx|{s.swapped = !s.swapped;cx.notify();}))))
                .child({let swapped=self.swapped;div().relative().w(px(320.)).h(px(132.)).flex_shrink_0()
                    .child(canvas(|_,_,_|(),move|bounds,(),window,_|{
                        for i in 0..40 {
                            let color=if (i%2==0)^swapped {rgb(0x1669ff)} else {rgb(0xff8020)};
                            window.paint_quad(fill(Bounds::new(bounds.origin+point(px(i as f32*8.),px(0.)),size(px(8.),bounds.size.height)),color));
                        }
                    }).size_full())
                    .child(painted().absolute().left(px(24.)).top(px(20.)).w(px(264.)).h(px(88.)).rounded(px(16.))
                        .when(self.backdrop,|el|el.backdrop_blur(BackdropBlur::new(px(12.)).unwrap()))
                        .flex().items_center().justify_center().text_color(white()).child("Sharp foreground text"))});
        }
        root
    }
}
