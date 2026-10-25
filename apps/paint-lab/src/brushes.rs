//! Fixed main-pass brush examples, independent of the legacy paint editor.
use gpui::{prelude::*, *};
use xgpui_shader::{BlendMode, Brush, BrushPoint, BrushStop, LayerEffect, Noise, painted};

pub fn gallery(lab: &super::Lab, cx: &mut Context<super::Lab>) -> impl IntoElement {
    let stops = vec![
        BrushStop::new(0., [0.1, 0.3, 1., 1.]),
        BrushStop::new(0.5, [0.7, 0.2, 0.9, 0.65]),
        BrushStop::new(1., [1., 0.5, 0.1, 1.]),
    ];
    let brushes = [
        ("Solid", Brush::solid([0.15, 0.3, 0.75, 1.]).unwrap()),
        ("Linear", Brush::linear(60., stops.clone()).unwrap()),
        (
            "Radial",
            Brush::radial([0.4, 0.4], [0.65, 0.8], stops).unwrap(),
        ),
    ];
    let ring = Brush::linear(
        90.,
        vec![
            BrushStop::new(0., [1., 1., 1., 0.15]),
            BrushStop::new(1., [1., 1., 1., 0.9]),
        ],
    )
    .unwrap();
    let material = Brush::linear(
        120.,
        vec![
            BrushStop::new(0., [0.12, 0.2, 0.4, 1.]),
            BrushStop::new(1., [0.55, 0.25, 0.65, 1.]),
        ],
    )
    .unwrap();
    let typography = Brush::linear(
        90.,
        vec![
            BrushStop::new(0., [0.05, 0.3, 1., 1.]),
            BrushStop::new(1., [1., 0.18, 0.02, 1.]),
        ],
    )
    .unwrap();
    let freeform = Brush::freeform(vec![
        BrushPoint::new([0.15, 0.2], [1., 0.05, 0.08, 1.]),
        BrushPoint::new([0.85, 0.2], [0.05, 0.3, 1., 1.]),
        BrushPoint::new([0.5, 0.85], [0.05, 0.9, 0.25, 1.]),
    ])
    .unwrap();
    let mesh = Brush::mesh([
        [0.05, 0.3, 1., 1.],
        [1., 0.18, 0.02, 1.],
        [0.9, 0.05, 0.7, 1.],
        [0., 0.8, 0.7, 1.],
    ])
    .unwrap();
    let point_count = [4, 8, 16][lab.freeform_step as usize];
    let expanded = Brush::freeform(expanded_demo_points(point_count)).unwrap();
    let spatial = [
        ("Freeform · three color points", freeform),
        (
            match point_count {
                4 => "Freeform · four points",
                8 => "Freeform · eight points",
                _ => "Freeform · sixteen points",
            },
            expanded,
        ),
        ("Hermite mesh · regular 2×2", mesh),
    ];
    let procedural = [
        ("Simplex · 3 octaves", Noise::simplex(0.035, 3, 0.8)),
        ("Worley · F1", Noise::worley(0.045, 0.8)),
        ("Worley · F2", Noise::worley_f2(0.045, 0.8)),
        ("Blue-noise dither", Noise::dither(0.15)),
    ];
    let modes = [
        ("Normal", BlendMode::Normal),
        ("Multiply", BlendMode::Multiply),
        ("Screen", BlendMode::Screen),
        ("Overlay", BlendMode::Overlay),
        ("Color dodge", BlendMode::ColorDodge),
        ("Soft light", BlendMode::SoftLight),
        ("Linear burn", BlendMode::LinearBurn),
    ];
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .child(backdrop_demo(lab, cx))
        .child(hermite_demo(lab, cx))
        .child(worley_demo())
        .child(blue_noise_demo())
        .child(path_brush_demo())
        .child(path_batch_demo())
        .child(group_filter_demo(lab, cx))
        .child(group_opacity_demo())
        .child("Main-pass brushes · solid, linear, radial")
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .children(brushes.into_iter().map(|(label, brush)| {
                    painted()
                        .w(px(208.))
                        .h(px(120.))
                        .rounded(px(24.))
                        .border_2()
                        .bg_brush(brush)
                        .border_brush(ring.clone())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(label)
                })),
        )
        .child("Spatial brushes · backgrounds, borders, and glyphs")
        .child(
            super::button(
                "freeform-points",
                format!("Larger freeform: {point_count} points · click to cycle"),
            )
            .on_click(cx.listener(|lab, _, _, cx| {
                lab.freeform_step = (lab.freeform_step + 1) % 3;
                cx.notify();
            })),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .children(spatial.into_iter().map(|(label, brush)| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(label)
                        .child(
                            painted()
                                .w(px(320.))
                                .h(px(160.))
                                .rounded(px(24.))
                                .bg_brush(brush.clone()),
                        )
                        .child(
                            painted()
                                .w(px(320.))
                                .h(px(88.))
                                .rounded(px(24.))
                                .border_8()
                                .border_brush(brush.clone())
                                .flex()
                                .items_center()
                                .justify_center()
                                .child("One continuous border field"),
                        )
                        .child(
                            painted()
                                .w(px(320.))
                                .min_h(px(96.))
                                .text_size(px(36.))
                                .font_weight(FontWeight::BOLD)
                                .text_brush(brush)
                                .child("Spatial color across glyphs"),
                        )
                })),
        )
        .child("Brush-painted text · one continuous field across glyphs")
        .child(
            painted()
                .w(px(640.))
                .min_h(px(104.))
                .text_size(px(40.))
                .font_weight(FontWeight::BOLD)
                .text_brush(typography.clone())
                .child("Electric blue to vivid orange · wrapped typography"),
        )
        .child(
            painted()
                .w(px(640.))
                .min_h(px(72.))
                .text_size(px(30.))
                .font_weight(FontWeight::BOLD)
                .text_brush(
                    typography
                        .clone()
                        .with_noise(Noise::simplex(0.08, 3, 0.6), BlendMode::Overlay)
                        .unwrap(),
                )
                .child("Textured glyphs · native emoji ✨"),
        )
        .child("Procedural brushes · modulation of the same linear gradient")
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .children(procedural.into_iter().map(|(label, noise)| {
                    painted()
                        .w(px(208.))
                        .h(px(120.))
                        .rounded(px(24.))
                        .bg_brush(
                            material
                                .clone()
                                .with_noise(noise, BlendMode::Overlay)
                                .unwrap(),
                        )
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(label)
                })),
        )
        .child("Brush-local blends · same Simplex signal and strength")
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .children(modes.into_iter().map(|(label, mode)| {
                    painted()
                        .w(px(160.))
                        .h(px(96.))
                        .rounded(px(16.))
                        .bg_brush(
                            material
                                .clone()
                                .with_noise(Noise::simplex(0.035, 3, 0.75), mode)
                                .unwrap(),
                        )
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(label)
                })),
        )
}

fn backdrop_demo(lab: &super::Lab, cx: &mut Context<super::Lab>) -> impl IntoElement {
    let changed = lab.backdrop_changed;
    let moved = lab.backdrop_moved;
    let disabled = lab.backdrop_disabled;
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child("Backdrop blur · interactive visual check")
        .child(
            div().flex().flex_wrap().gap_2()
                .child(super::button("backdrop-source", "Swap stripe colors")
                    .on_click(cx.listener(|lab, _, _, cx| {
                        lab.backdrop_changed = !lab.backdrop_changed;
                        cx.notify();
                    })))
                .child(super::button("backdrop-foreground", "Move white marker")
                    .on_click(cx.listener(|lab, _, _, cx| {
                        lab.backdrop_moved = !lab.backdrop_moved;
                        cx.notify();
                    })))
                .child(super::button("backdrop-toggle", if disabled { "Blur: off" } else { "Blur: on" })
                    .on_click(cx.listener(|lab, _, _, cx| {
                        lab.backdrop_disabled = !lab.backdrop_disabled;
                        cx.notify();
                    }))),
        )
        .child("Swap: the frosted panel must change color. Move: the marker stays sharp. Toggle blur to compare.")
        .child(
            div()
                .relative()
                .w(px(360.))
                .h(px(144.))
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, (), window, _| {
                            for i in 0..45 {
                                let color = match (changed, i % 2 == 0) {
                                    (false, true) => rgb(0x165dff),
                                    (false, false) => rgb(0xff6408),
                                    (true, true) => rgb(0xff0020),
                                    (true, false) => rgb(0xffff00),
                                };
                                window.paint_quad(fill(
                                    Bounds::new(
                                        bounds.origin + point(px(i as f32 * 8.), px(0.)),
                                        size(px(8.), bounds.size.height),
                                    ),
                                    color,
                                ));
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    painted()
                        .absolute()
                        .left(px(96.))
                        .top(px(16.))
                        .w(px(248.))
                        .h(px(112.))
                        .rounded(px(16.))
                        .backdrop_blur(xgpui_shader::BackdropBlur::new(px(if disabled { 0. } else { 16. })).unwrap())
                        .bg(gpui::rgba(0xffffff24))
                        .border_1()
                        .border_color(gpui::rgba(0xffffff90))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .text_color(white())
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(22.))
                                .child("Frosted backdrop"),
                        )
                        .child("Foreground stays sharp"),
                )
                .child(
                    div().absolute()
                        .left(px(if moved { 302. } else { 110. }))
                        .top(px(42.))
                        .w(px(18.)).h(px(60.))
                        .bg(white()),
                ),
        )
}

fn group_opacity_demo() -> impl IntoElement {
    let shapes = || {
        div()
            .relative()
            .w(px(176.))
            .h(px(112.))
            .child(
                div()
                    .absolute()
                    .left(px(16.))
                    .top(px(16.))
                    .w(px(96.))
                    .h(px(72.))
                    .rounded(px(12.))
                    .bg(rgb(0xff3808)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(64.))
                    .top(px(32.))
                    .w(px(96.))
                    .h(px(64.))
                    .rounded(px(12.))
                    .bg(rgb(0x164dff)),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child("Group opacity · compare the overlap")
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Ordinary opacity: 50% per shape")
                        .child(
                            div()
                                .w(px(176.))
                                .h(px(112.))
                                .bg(white())
                                .child(div().opacity(0.5).child(shapes())),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Layer effect: 50% after overlap")
                        .child(
                            div().w(px(176.)).h(px(112.)).bg(white()).child(
                                painted()
                                    .w(px(176.))
                                    .h(px(112.))
                                    .layer_effect(LayerEffect::opacity(0.5).unwrap())
                                    .child(shapes()),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .child("The blue shape fully covers red before the captured group fades."),
        )
}

fn group_filter_demo(lab: &super::Lab, cx: &mut Context<super::Lab>) -> impl IntoElement {
    let (label, effect) = match lab.layer_filter_step {
        0 => (
            "Invert 100% · click to cycle",
            LayerEffect::invert(1.).unwrap(),
        ),
        1 => (
            "Invert 50% · click to cycle",
            LayerEffect::invert(0.5).unwrap(),
        ),
        _ => (
            "Saturation 2 · click to cycle",
            LayerEffect::saturation(2.).unwrap(),
        ),
    };
    let sample = || {
        div()
            .relative()
            .w(px(176.))
            .h(px(112.))
            .bg(rgb(0x213c54))
            .child(
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(12.))
                    .w(px(100.))
                    .h(px(76.))
                    .rounded(px(12.))
                    .bg(rgb(0xe86a47)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(64.))
                    .top(px(32.))
                    .w(px(100.))
                    .h(px(68.))
                    .rounded(px(12.))
                    .bg(rgb(0x4f87c5)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(18.))
                    .top(px(42.))
                    .text_color(rgb(0xffdf28))
                    .text_size(px(20.))
                    .font_weight(FontWeight::BOLD)
                    .child("Color + text"),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child("Captured color filters · the whole card and text")
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Original · saturation 1")
                        .child(sample()),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Grayscale · saturation 0")
                        .child(
                            painted()
                                .w(px(176.))
                                .h(px(112.))
                                .layer_effect(LayerEffect::grayscale(1.).unwrap())
                                .child(sample()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            super::button("layer-filter-cycle", label).on_click(cx.listener(
                                |lab, _, _, cx| {
                                    lab.layer_filter_step = (lab.layer_filter_step + 1) % 3;
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            painted()
                                .w(px(176.))
                                .h(px(112.))
                                .layer_effect(effect)
                                .child(sample()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Brightness 0.55 · contrast 1.6")
                        .child(
                            painted()
                                .w(px(176.))
                                .h(px(112.))
                                .layer_effect(
                                    LayerEffect::brightness(0.55)
                                        .unwrap()
                                        .with_contrast(1.6)
                                        .unwrap(),
                                )
                                .child(sample()),
                        ),
                ),
        )
}

fn path_brush_demo() -> impl IntoElement {
    let gradient = Brush::linear(
        65.,
        vec![
            BrushStop::new(0., [0.05, 0.3, 1., 1.]),
            BrushStop::new(1., [1., 0.22, 0.02, 1.]),
        ],
    )
    .unwrap();
    let mesh = Brush::mesh([
        [1., 0.1, 0., 1.],
        [0., 0.8, 1., 1.],
        [0.4, 0., 1., 1.],
        [1., 0.7, 0., 1.],
    ])
    .unwrap()
    .with_noise(Noise::simplex(0.07, 2, 0.4), BlendMode::Overlay)
    .unwrap();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child("Vector brushes · curved fill, a hole, and a stroked path")
        .child(
            div().flex().flex_wrap().gap_4().children(
                [(false, gradient), (true, mesh)]
                    .into_iter()
                    .map(|(stroke, brush)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(if stroke {
                                "Noisy mesh stroke"
                            } else {
                                "Blue/orange fill with a hole"
                            })
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    move |bounds, (), window, _| {
                                        let mut p = if stroke {
                                            PathBuilder::stroke(px(12.))
                                        } else {
                                            PathBuilder::fill()
                                        };
                                        p.move_to(point(px(16.), px(26.)));
                                        p.cubic_bezier_to(
                                            point(px(188.), px(32.)),
                                            point(px(62.), px(-4.)),
                                            point(px(162.), px(4.)),
                                        );
                                        p.curve_to(
                                            point(px(166.), px(114.)),
                                            point(px(208.), px(112.)),
                                        );
                                        p.line_to(point(px(38.), px(108.)));
                                        p.close();
                                        if !stroke {
                                            p.move_to(point(px(78.), px(48.)));
                                            p.line_to(point(px(78.), px(82.)));
                                            p.line_to(point(px(130.), px(82.)));
                                            p.line_to(point(px(130.), px(48.)));
                                            p.close();
                                        }
                                        p.translate(bounds.origin);
                                        brush.paint_path(window, p.build().unwrap()).unwrap();
                                    },
                                )
                                .w(px(224.))
                                .h(px(144.)),
                            )
                    }),
            ),
        )
}

/// Keep the normal, subtle material beside an intentionally exaggerated signal.
/// Enlarged cells read the exact asset embedded in the GPU shader, not a new seed.
fn blue_noise_demo() -> impl IntoElement {
    let gradient = Brush::linear(
        90.,
        vec![
            BrushStop::new(0., [0.25, 0.25, 0.25, 1.]),
            BrushStop::new(1., [0.75, 0.75, 0.75, 1.]),
        ],
    )
    .unwrap();
    let swatches = [
        ("No noise", gradient.clone()),
        (
            "Visible grain · strength 0.6",
            gradient
                .with_noise(Noise::dither(0.6), BlendMode::Overlay)
                .unwrap(),
        ),
        (
            "Noise only · strength 1.0",
            Brush::solid([0.5, 0.5, 0.5, 1.])
                .unwrap()
                .with_noise(Noise::dither(1.), BlendMode::Normal)
                .unwrap(),
        ),
    ];
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child("Blue-noise visual check · exaggerated for visibility")
        .child(
            div().flex().flex_wrap().gap_4()
                .children(swatches.into_iter().map(|(label, brush)| {
                    div().flex().flex_col().gap_2()
                        .child(label)
                        .child(painted().w(px(200.)).h(px(144.)).bg_brush(brush))
                })),
        )
        .child("Below: one 16×16 tile enlarged 8×. The actual shader uses one sample per physical pixel.")
        .child(
            div().flex().flex_wrap().gap_4()
                .children([false, true].into_iter().map(|threshold| {
                    div().flex().flex_col().gap_2()
                        .child(if threshold { "50% threshold · dispersed dots" } else { "Full grayscale rank tile" })
                        .child(
                            canvas(
                                |_, _, _| (),
                                move |bounds, (), window, _| {
                                    let ranks = include_bytes!("../../../extensions/xgpui-ext/crates/xgpui-shader/src/brush/blue_noise.bin");
                                    for (i, rank) in ranks.iter().enumerate() {
                                        let value = if threshold {
                                            if *rank < 128 { 0 } else { 255 }
                                        } else { *rank as u32 };
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                bounds.origin + point(px((i % 16) as f32 * 8.), px((i / 16) as f32 * 8.)),
                                                size(px(8.), px(8.)),
                                            ),
                                            rgb(value * 0x010101),
                                        ));
                                    }
                                },
                            ).w(px(128.)).h(px(128.)),
                        )
                })),
        )
}

fn worley_demo() -> impl IntoElement {
    div().flex().flex_col().gap_2()
        .child("Worley cellular noise · same features, different distance")
        .child("F1 measures the closest point; F2 measures the second closest. Both use the same scale and seed.")
        .child(div().flex().flex_wrap().gap_4().children([
            ("F1 · closest", Noise::worley(0.035, 1.)),
            ("F2 · second closest", Noise::worley_f2(0.035, 1.)),
        ].into_iter().map(|(label, noise)| {
            div().flex().flex_col().gap_2().child(label)
                .child(painted().w(px(288.)).h(px(176.))
                    .bg_brush(Brush::solid([0.5,0.5,0.5,1.]).unwrap()
                        .with_noise(noise, BlendMode::Normal).unwrap()))
        })))
}

fn expanded_demo_points(count: usize) -> Vec<BrushPoint> {
    let colors = [
        [1., 0.03, 0.08, 1.],
        [1., 0.5, 0., 1.],
        [1., 0.95, 0., 1.],
        [0.05, 1., 0.2, 1.],
        [0., 0.9, 1., 1.],
        [0.05, 0.2, 1., 1.],
        [0.65, 0.05, 1., 1.],
        [1., 0., 0.65, 1.],
    ];
    (0..count)
        .map(|i| {
            let position = if count == 8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.;
                [0.5 + 0.42 * angle.cos(), 0.5 + 0.42 * angle.sin()]
            } else {
                let side = if count == 4 { 2 } else { 4 };
                [
                    0.08 + (i % side) as f32 * 0.84 / (side - 1) as f32,
                    0.08 + (i / side) as f32 * 0.84 / (side - 1) as f32,
                ]
            };
            BrushPoint::new(position, colors[i % 8])
        })
        .collect()
}

fn hermite_demo(lab: &super::Lab, cx: &mut Context<super::Lab>) -> impl IntoElement {
    use super::Control;
    let editor = &lab.hermite;
    let label = [
        "Regular",
        "Bend U tangents",
        "Bend V tangents",
        "Inset patch",
        "Move center point",
    ][editor.preset as usize];
    let mut chips = div().flex().flex_wrap().gap_2();
    for index in 0..editor.points.len() {
        let name = format!(
            "{} · row {}, col {}",
            index + 1,
            index / editor.side as usize + 1,
            index % editor.side as usize + 1
        );
        chips = chips.child(
            super::button(("hermite-point", index), name)
                .bg(rgb(if index == editor.selected {
                    0x2766b0
                } else {
                    0x343a45
                }))
                .on_click(cx.listener(move |lab, _, _, cx| {
                    lab.hermite.selected = index;
                    if lab.hermite.side == 3 {
                        lab.mesh_selected = index;
                    }
                    cx.notify();
                })),
        );
    }
    let mut controls = div().flex().flex_wrap().gap_4();
    for group in [
        [Control::HermiteX, Control::HermiteY],
        [Control::HermiteUAngle, Control::HermiteULength],
        [Control::HermiteVAngle, Control::HermiteVLength],
    ] {
        let mut column = div().w(px(320.)).flex().flex_col().gap_2();
        for control in group {
            column = column.child(lab.slider(control, cx));
        }
        controls = controls.child(column);
    }
    let direct = match &editor.brush {
        Ok(brush) => div()
            .bg(rgb(0x45454f))
            .child(painted().w(px(288.)).h(px(176.)).bg_brush(brush.clone())),
        Err(_) => div()
            .w(px(288.))
            .h(px(176.))
            .p_4()
            .bg(rgb(0x45454f))
            .flex()
            .items_center()
            .justify_center()
            .text_color(white())
            .child("Direct brush unavailable for this edit. Adjust controls or reset."),
    };
    let mut demo = div().flex().flex_col().gap_2()
        .child("Hermite mesh · live 2×2 / 3×3 editor")
        .child(div().flex().flex_wrap().gap_2()
            .child(super::button("hermite-size",format!("Grid: {}×{} · switch",editor.side,editor.side))
                .on_click(cx.listener(|lab,_,_,cx| {
                    lab.freeze();
                    let side=if lab.hermite.side==2 {3} else {2};
                    lab.hermite.set_side(side);
                    if side==3 {
                        lab.effect=super::Effect::Mesh;
                        lab.native=false;
                        lab.hermite.selected=lab.mesh_selected;
                    }
                    cx.notify();
                })))
            .child(super::button("hermite-preset",format!("{label}{} · next preset",if editor.edited { " (edited)" } else { "" }))
                .on_click(cx.listener(|lab,_,_,cx| {
                    lab.freeze();
                    lab.hermite.load_preset((lab.hermite.preset+1)%lab.hermite.preset_count());
                    lab.publish_hermite_edit();
                    cx.notify();
                })))
            .child(super::button("hermite-reset-point","Reset selected point")
                .on_click(cx.listener(|lab,_,_,cx| {
                    lab.freeze(); lab.hermite.reset_selected(); lab.publish_hermite_edit(); cx.notify();
                })))
            .child(super::button("hermite-reset-all","Reset all points")
                .on_click(cx.listener(|lab,_,_,cx| {
                    lab.freeze(); lab.hermite.load_preset(lab.hermite.preset); lab.publish_hermite_edit(); cx.notify();
                }))))
        .child(chips)
        .child(controls)
        .child("Angles are clockwise from right; lengths use normalized mesh space. 3×3 mode shares points and selection with the main Mesh editor.");
    if let Err(error) = &editor.brush {
        demo = demo.child(div().max_w(px(900.)).p_3().bg(rgb(0x6c2630)).text_color(white())
            .child(format!("Outside the direct brush limit: {error}. The legacy preview continues to show this edit.")));
    }
    demo = demo.child(
        div()
            .flex()
            .flex_wrap()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child("Existing mesh · offscreen tessellation")
                    .child(
                        div().bg(rgb(0x45454f)).child(
                            painted()
                                .w(px(288.))
                                .h(px(176.))
                                .bg_paint(editor.legacy.clone()),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child("Direct brush · current edit")
                    .child(direct),
            ),
    );
    if let Ok(brush) = &editor.brush {
        demo = demo
            .child(
                painted()
                    .w(px(592.))
                    .h(px(72.))
                    .border_8()
                    .border_brush(brush.clone())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("Same patch field on a border"),
            )
            .child(
                painted()
                    .w(px(592.))
                    .text_size(px(36.))
                    .font_weight(FontWeight::BOLD)
                    .text_brush(brush.clone())
                    .child("Hermite patch across glyphs"),
            );
    }
    demo
}

fn path_batch_demo() -> impl IntoElement {
    let brush = Brush::linear(
        45.,
        vec![
            BrushStop::new(0., [0., 0.8, 1., 0.8]),
            BrushStop::new(1., [1., 0.1, 0.6, 0.8]),
        ],
    )
    .unwrap();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child("Path batching · 24 separate curved shapes")
        .child("Planned: 24 paths → 1 coverage batch; overlapping bounds split batches")
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    for index in 0..24 {
                        let mut path = PathBuilder::fill();
                        path.move_to(point(px(4.), px(4.)));
                        path.cubic_bezier_to(
                            point(px(40.), px(36.)),
                            point(px(48.), px(0.)),
                            point(px(0.), px(44.)),
                        );
                        path.line_to(point(px(4.), px(36.)));
                        path.close();
                        path.translate(
                            bounds.origin
                                + point(px((index % 8) as f32 * 56.), px((index / 8) as f32 * 48.)),
                        );
                        brush.paint_path(window, path.build().unwrap()).unwrap();
                    }
                },
            )
            .w(px(448.))
            .h(px(144.)),
        )
}
