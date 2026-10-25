use super::*;

fn filtered_capture(saturation: f32, opacity: f32, scale: f32) -> Arc<LayerCapture> {
    Arc::new(LayerCapture {
        blur_radius: ScaledPixels(0.),
        blend_mode: gpui::LayerBlendMode::Normal,
        bounds: Bounds::new(point(px(8.25), px(8.25)), size(px(79.5), px(47.5))).scale(scale),
        opacity,
        saturation,
        brightness: 1.,
        contrast: 1.,
        invert: 0.,
        program: backdrop::shared_program().unwrap(),
    })
}
#[test]
fn saturation_and_grayscale_validate_api_and_scene_parameters() {
    for value in [f32::NAN, f32::INFINITY, -0.01, 2.01] {
        assert!(LayerEffect::saturation(value).is_err());
        assert!(
            LayerEffect::opacity(0.5)
                .unwrap()
                .with_saturation(value)
                .is_err()
        );
        let mut scene = Scene::default();
        let c = filtered_capture(value, 1., 1.);
        scene.begin_layer_capture(c.clone(), viewport());
        scene.end_layer_capture(c, viewport());
        scene.finish();
        assert!(
            scene
                .layer_plans(size(DevicePixels(96), DevicePixels(64)))
                .is_err()
        );
    }
    for value in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        assert!(LayerEffect::grayscale(value).is_err());
        assert!(
            LayerEffect::opacity(0.5)
                .unwrap()
                .with_grayscale(value)
                .is_err()
        );
    }
    for saturation in [0., 0.5, 1., 2.] {
        assert!(LayerEffect::saturation(saturation).is_ok());
    }
    for strength in [0., 0.5, 1.] {
        assert!(LayerEffect::grayscale(strength).is_ok());
    }
}
fn associated_over(source: [u8; 4], destination: [f64; 4]) -> [f64; 4] {
    let alpha = source[3] as f64 / 255.;
    std::array::from_fn(|i| {
        let value = if i == 3 {
            alpha
        } else {
            source[i] as f64 / 255. * alpha
        };
        ((value + destination[i] * (1. - alpha)) * 255.).round() / 255.
    })
}
// Independent reference explicitly unassociates and clamps straight RGB, then
// re-associates it. The shader uses a matrix directly on associated RGB instead.
fn filter_reference(
    source: [f64; 4],
    saturation: f64,
    opacity: f64,
    destination: [f64; 4],
) -> [u8; 4] {
    let alpha = source[3];
    let mut filtered = [0.; 4];
    if alpha > 0. {
        let rgb = [source[0] / alpha, source[1] / alpha, source[2] / alpha];
        let gray = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
        for i in 0..3 {
            filtered[i] = (gray + (rgb[i] - gray) * saturation).clamp(0., 1.) * alpha * opacity;
        }
    }
    filtered[3] = alpha * opacity;
    std::array::from_fn(|i| {
        ((filtered[i] + destination[i] * (1. - filtered[3])) * 255.).round() as u8
    })
}
#[cfg(feature = "wgpu")]
#[test]
fn group_filters_match_translucent_reference_at_fractional_dpi() {
    let pairs = [
        ([255, 0, 0, 255], [0, 0, 255, 128]),
        ([0, 255, 0, 64], [255, 30, 0, 191]),
        ([255, 0, 0, 0], [0, 80, 255, 96]),
    ];
    for mut renderer in renderers() {
        for (first, second) in pairs {
            for saturation in [0., 0.35, 1., 1.5, 2.] {
                for opacity in [0.4, 1.] {
                    for scale in [1., 1.5, 2.] {
                        let full = ContentMask {
                            bounds: viewport().bounds.map(|p| ScaledPixels(p.0 * scale)),
                        };
                        let mut scene = Scene::default();
                        scene.insert_primitive(Quad {
                            bounds: full.bounds,
                            content_mask: full,
                            background: gpui::rgb(0x336699).into(),
                            ..Default::default()
                        });
                        let capture = filtered_capture(saturation, opacity, scale);
                        scene.begin_layer_capture(capture.clone(), full);
                        for (x, color) in [(16., first), (40., second)] {
                            scene.insert_primitive(Quad {
                                bounds: Bounds::new(point(px(x), px(16.)), size(px(40.), px(32.)))
                                    .scale(scale),
                                content_mask: full,
                                background: gpui::rgba(u32::from_be_bytes(color)).into(),
                                ..Default::default()
                            });
                        }
                        scene.end_layer_capture(capture, full);
                        scene.finish();
                        let image = renderer
                            .render_scene_to_image(
                                &scene,
                                size(
                                    DevicePixels((96. * scale) as i32),
                                    DevicePixels((64. * scale) as i32),
                                ),
                            )
                            .unwrap();
                        let source = associated_over(second, associated_over(first, [0.; 4]));
                        let expected = filter_reference(
                            source,
                            saturation as f64,
                            opacity as f64,
                            [0.2, 0.4, 0.6, 1.],
                        );
                        pixel(&image, (48. * scale) as u32, (24. * scale) as u32, expected);
                        pixel(
                            &image,
                            (4. * scale) as u32,
                            (4. * scale) as u32,
                            [51, 102, 153, 255],
                        );
                        pixel(
                            &image,
                            (10. * scale) as u32,
                            (10. * scale) as u32,
                            [51, 102, 153, 255],
                        );
                    }
                }
            }
        }
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn changing_filters_reuses_raw_capture_and_nested_changes_invalidate_parent() {
    let extent = size(DevicePixels(96), DevicePixels(64));
    for mut renderer in renderers() {
        let mut original = Scene::default();
        rect(&mut original, 0., 0., 96., 64., gpui::white());
        let c = filtered_capture(0., 1., 1.);
        original.begin_layer_capture(c.clone(), viewport());
        let from = original.len();
        rect(
            &mut original,
            16.,
            16.,
            40.,
            32.,
            gpui::rgb(0xff0000).into(),
        );
        rect(
            &mut original,
            40.,
            16.,
            40.,
            32.,
            gpui::rgba(0x0000ff80).into(),
        );
        let to = original.len();
        original.end_layer_capture(c, viewport());
        original.finish();
        renderer.render_scene_to_image(&original, extent).unwrap();
        for saturation in [1., 2., 0.35] {
            let mut changed = Scene::default();
            rect(&mut changed, 0., 0., 96., 64., gpui::rgb(0x336699).into());
            let c = filtered_capture(saturation, 0.4, 1.);
            changed.begin_layer_capture(c.clone(), viewport());
            changed.replay(from..to, &original);
            changed.end_layer_capture(c, viewport());
            changed.finish();
            let image = renderer.render_scene_to_image(&changed, extent).unwrap();
            pixel(
                &image,
                48,
                24,
                filter_reference(
                    associated_over([0, 0, 255, 128], associated_over([255, 0, 0, 255], [0.; 4])),
                    saturation as f64,
                    0.4,
                    [0.2, 0.4, 0.6, 1.],
                ),
            );
        }
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 1,
                hits: 3
            },
            "color changes reuse unfiltered pixels"
        );
        let mut parent = Scene::default();
        rect(&mut parent, 0., 0., 96., 64., gpui::white());
        let outer = filtered_capture(0.5, 1., 1.);
        parent.begin_layer_capture(outer.clone(), viewport());
        let inner = filtered_capture(0., 1., 1.);
        parent.begin_layer_capture(inner.clone(), viewport());
        let from = parent.len();
        rect(&mut parent, 16., 16., 64., 32., gpui::rgb(0x0000ff).into());
        let to = parent.len();
        parent.end_layer_capture(inner, viewport());
        parent.end_layer_capture(outer, viewport());
        parent.finish();
        let image = renderer.render_scene_to_image(&parent, extent).unwrap();
        pixel(&image, 48, 24, [18, 18, 18, 255]);
        let before = renderer.stats();
        let mut changed = Scene::default();
        rect(&mut changed, 0., 0., 96., 64., gpui::white());
        let outer = filtered_capture(0.5, 1., 1.);
        changed.begin_layer_capture(outer.clone(), viewport());
        let inner = filtered_capture(2., 1., 1.);
        changed.begin_layer_capture(inner.clone(), viewport());
        changed.replay(from..to, &parent);
        changed.end_layer_capture(inner, viewport());
        changed.end_layer_capture(outer, viewport());
        changed.finish();
        let image = renderer.render_scene_to_image(&changed, extent).unwrap();
        pixel(&image, 48, 24, [9, 9, 137, 255]);
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: before.captures + 1,
                hits: before.hits + 1
            },
            "inner raw pixels reuse; filtered parent refreshes"
        );
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn painted_grayscale_filters_completed_children_before_group_opacity() {
    use gpui::*;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(white()).child(
                crate::painted()
                    .w(px(80.))
                    .h(px(48.))
                    .layer_effect(
                        LayerEffect::opacity(0.5)
                            .unwrap()
                            .with_saturation(2.)
                            .unwrap()
                            .with_grayscale(1.)
                            .unwrap(),
                    )
                    .child(
                        canvas(
                            |_, _, _| (),
                            |bounds, (), window, _| {
                                window.paint_quad(fill(bounds, rgb(0xff0000)));
                                window.paint_quad(fill(
                                    Bounds::new(
                                        bounds.origin + point(px(40.), px(0.)),
                                        size(px(40.), bounds.size.height),
                                    ),
                                    rgb(0x0000ff),
                                ));
                            },
                        )
                        .size_full(),
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
        (48. * scale) as u32,
        (24. * scale) as u32,
        [137, 137, 137, 255],
    );
    pixel(
        &image,
        (24. * scale) as u32,
        (24. * scale) as u32,
        [155, 155, 155, 255],
    );
}

#[path = "layer_tone_tests.rs"]
mod tone_tests;
