use super::*;
use gpui::LayerBlendMode;

const MODES: [LayerBlendMode; 7] = [
    LayerBlendMode::Normal,
    LayerBlendMode::Multiply,
    LayerBlendMode::Screen,
    LayerBlendMode::Overlay,
    LayerBlendMode::ColorDodge,
    LayerBlendMode::SoftLight,
    LayerBlendMode::LinearBurn,
];

// Independent straight-color reference, following W3C Compositing and Blending.
fn reference(source: [f64; 4], destination: [f64; 4], mode: LayerBlendMode) -> [f64; 4] {
    let a = source[3];
    let b = destination[3];
    let mut out = [0.; 4];
    for i in 0..3 {
        let s = if a > 0. { source[i] / a } else { 0. };
        let d = if b > 0. { destination[i] / b } else { 0. };
        let mixed = match mode {
            LayerBlendMode::Normal => s,
            LayerBlendMode::Multiply => s * d,
            LayerBlendMode::Screen => 1. - (1. - s) * (1. - d),
            LayerBlendMode::Overlay => {
                if d <= 0.5 {
                    2. * s * d
                } else {
                    1. - 2. * (1. - s) * (1. - d)
                }
            }
            LayerBlendMode::ColorDodge => {
                if d == 0. {
                    0.
                } else if s == 1. {
                    1.
                } else {
                    (d / (1. - s)).min(1.)
                }
            }
            LayerBlendMode::SoftLight => {
                if s <= 0.5 {
                    d - (1. - 2. * s) * d * (1. - d)
                } else {
                    let curve = if d <= 0.25 {
                        16. * d.powi(3) - 12. * d.powi(2) + 4. * d
                    } else {
                        d.sqrt()
                    };
                    d + (2. * s - 1.) * (curve - d)
                }
            }
            LayerBlendMode::LinearBurn => (s + d - 1.).max(0.),
        };
        out[i] = (1. - b) * source[i] + (1. - a) * destination[i] + a * b * mixed;
    }
    out[3] = a + b - a * b;
    out
}
fn associated(rgba: [u8; 4]) -> [f64; 4] {
    let a = rgba[3] as f64 / 255.;
    [
        rgba[0] as f64 / 255. * a,
        rgba[1] as f64 / 255. * a,
        rgba[2] as f64 / 255. * a,
        a,
    ]
}
fn blend_capture(mode: LayerBlendMode, opacity: f32) -> Arc<LayerCapture> {
    let mut capture = capture(opacity);
    Arc::get_mut(&mut capture).unwrap().blend_mode = mode;
    capture
}
#[test]
fn destination_blend_transport_and_api() {
    let api_modes = [
        crate::BlendMode::Normal,
        crate::BlendMode::Multiply,
        crate::BlendMode::Screen,
        crate::BlendMode::Overlay,
        crate::BlendMode::ColorDodge,
        crate::BlendMode::SoftLight,
        crate::BlendMode::LinearBurn,
    ];
    for (mode, api_mode) in MODES.into_iter().zip(api_modes) {
        assert!(LayerEffect::blend(api_mode).is_ok());
        let mut scene = Scene::default();
        let capture = blend_capture(mode, 0.5);
        scene.begin_layer_capture(capture.clone(), viewport());
        scene.end_layer_capture(capture, viewport());
        scene.finish();
        let plans = scene
            .layer_plans(size(DevicePixels(96), DevicePixels(64)))
            .unwrap();
        assert_eq!(plans[0].parameters[1][5][3], mode as u8 as f32);
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn destination_blends_match_reference_with_partial_and_zero_alpha() {
    let extent = size(DevicePixels(96), DevicePixels(64));
    for mut renderer in renderers() {
        for mode in MODES {
            for (source, destination) in [
                ([220, 64, 140, 255], [40, 160, 230, 255]),
                ([220, 64, 140, 128], [40, 160, 230, 96]),
                ([255, 0, 255, 255], [0, 255, 32, 128]),
                ([200, 40, 120, 0], [80, 120, 200, 128]),
                ([200, 40, 120, 128], [80, 120, 200, 0]),
            ] {
                for opacity in [0., 0.4, 1.] {
                    let mut scene = Scene::default();
                    rect(&mut scene, 0., 0., 96., 64., gpui::rgb(0x336699).into());
                    // An outer isolation gives the blend a truly translucent
                    // destination even when the presentation target is opaque.
                    let outer = super::capture(1.);
                    scene.begin_layer_capture(outer.clone(), viewport());
                    rect(
                        &mut scene,
                        8.,
                        8.,
                        80.,
                        48.,
                        gpui::rgba(u32::from_be_bytes(destination)).into(),
                    );
                    let inner = blend_capture(mode, opacity);
                    scene.begin_layer_capture(inner.clone(), viewport());
                    rect(
                        &mut scene,
                        16.,
                        16.,
                        64.,
                        32.,
                        gpui::rgba(u32::from_be_bytes(source)).into(),
                    );
                    scene.end_layer_capture(inner, viewport());
                    scene.end_layer_capture(outer, viewport());
                    scene.finish();
                    let image = renderer.render_scene_to_image(&scene, extent).unwrap();
                    let src = associated(source).map(|v| v * opacity as f64);
                    let dst = associated(destination);
                    let blended = reference(src, dst, mode);
                    let base = [0.2, 0.4, 0.6, 1.];
                    let expected = std::array::from_fn(|i| {
                        ((blended[i] + base[i] * (1. - blended[3])) * 255.).round() as u8
                    });
                    pixel(&image, 48, 24, expected);
                    pixel(&image, 4, 4, [51, 102, 153, 255]);
                }
            }
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn destination_blend_edits_reuse_source_and_refresh_destination() {
    let extent = size(DevicePixels(96), DevicePixels(64));
    for mut renderer in renderers() {
        let mut original = Scene::default();
        rect(&mut original, 0., 0., 96., 64., gpui::white());
        let capture = blend_capture(LayerBlendMode::Normal, 0.7);
        original.begin_layer_capture(capture.clone(), viewport());
        let from = original.len();
        rect(
            &mut original,
            16.,
            16.,
            64.,
            32.,
            gpui::rgba(0xc0408080).into(),
        );
        let to = original.len();
        original.end_layer_capture(capture, viewport());
        original.finish();
        renderer.render_scene_to_image(&original, extent).unwrap();
        for (index, mode) in MODES.into_iter().enumerate() {
            let destination = if index % 2 == 0 {
                [32, 180, 240, 255]
            } else {
                [230, 180, 40, 255]
            };
            let mut scene = Scene::default();
            rect(
                &mut scene,
                0.,
                0.,
                96.,
                64.,
                gpui::rgba(u32::from_be_bytes(destination)).into(),
            );
            let capture = blend_capture(mode, 0.7);
            scene.begin_layer_capture(capture.clone(), viewport());
            scene.replay(from..to, &original);
            scene.end_layer_capture(capture, viewport());
            scene.finish();
            let image = renderer.render_scene_to_image(&scene, extent).unwrap();
            let expected = reference(
                associated([192, 64, 128, 128]).map(|v| v * 0.7),
                associated(destination),
                mode,
            )
            .map(|v| (v * 255.).round() as u8);
            pixel(&image, 48, 24, expected);
        }
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 1,
                hits: 7
            }
        );
        // The containing group's raw pixels do depend on a nested blend mode.
        let mut parent = Scene::default();
        rect(&mut parent, 0., 0., 96., 64., gpui::white());
        let outer = super::capture(1.);
        parent.begin_layer_capture(outer.clone(), viewport());
        rect(&mut parent, 8., 8., 80., 48., gpui::rgb(0x2090d0).into());
        let inner = blend_capture(LayerBlendMode::Multiply, 1.);
        parent.begin_layer_capture(inner.clone(), viewport());
        let from = parent.len();
        rect(&mut parent, 16., 16., 64., 32., gpui::rgb(0xc04080).into());
        let to = parent.len();
        parent.end_layer_capture(inner, viewport());
        parent.end_layer_capture(outer, viewport());
        parent.finish();
        renderer.render_scene_to_image(&parent, extent).unwrap();
        let before = renderer.stats();
        let mut changed = Scene::default();
        rect(&mut changed, 0., 0., 96., 64., gpui::white());
        let outer = super::capture(1.);
        changed.begin_layer_capture(outer.clone(), viewport());
        // Replay destination and source independently, retaining both identities.
        changed.replay(2..3, &parent);
        let inner = blend_capture(LayerBlendMode::Screen, 1.);
        changed.begin_layer_capture(inner.clone(), viewport());
        changed.replay(from..to, &parent);
        changed.end_layer_capture(inner, viewport());
        changed.end_layer_capture(outer, viewport());
        changed.finish();
        let image = renderer.render_scene_to_image(&changed, extent).unwrap();
        let expected = reference(
            associated([192, 64, 128, 255]),
            associated([32, 144, 208, 255]),
            LayerBlendMode::Screen,
        )
        .map(|v| (v * 255.).round() as u8);
        pixel(&image, 48, 24, expected);
        assert_eq!(renderer.stats().captures, before.captures + 1);
        assert_eq!(renderer.stats().hits, before.hits + 1);
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn painted_destination_blend_applies_filters_opacity_and_ancestor_clip() {
    use gpui::*;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0x336699)).child(
                div()
                    .w(px(64.))
                    .h(px(48.))
                    .overflow_hidden()
                    .opacity(0.5)
                    .child(
                        crate::painted()
                            .flex_shrink_0()
                            .w(px(80.))
                            .h(px(48.))
                            .bg(rgb(0xff0000))
                            .layer_effect(
                                LayerEffect::opacity(0.5)
                                    .unwrap()
                                    .with_invert(1.)
                                    .unwrap()
                                    .with_blend_mode(crate::BlendMode::Multiply),
                            )
                            .child(div().w(px(32.)).h(px(32.)).bg(rgb(0x0000ff))),
                    ),
            )
        }
    }
    let mut cx =
        HeadlessAppContext::with_platform(Arc::new(NoopTextSystem::new()), Arc::new(()), || {
            Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?)))
        });
    let window = cx
        .open_window(size(px(96.), px(64.)), |_, cx| cx.new(|_| View))
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let scale = cx
        .update_window(window.into(), |_, window, _| window.scale_factor())
        .unwrap();
    let image = cx.capture_screenshot(window.into()).unwrap();
    pixel(
        &image,
        (16. * scale) as u32,
        (16. * scale) as u32,
        [51, 102, 115, 255],
    );
    pixel(
        &image,
        (48. * scale) as u32,
        (16. * scale) as u32,
        [38, 102, 153, 255],
    );
    pixel(
        &image,
        (72. * scale) as u32,
        (16. * scale) as u32,
        [51, 102, 153, 255],
    );
}
