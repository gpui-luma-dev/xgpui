use super::*;

fn tone_capture(
    saturation: f32,
    brightness: f32,
    contrast: f32,
    opacity: f32,
    scale: f32,
) -> Arc<LayerCapture> {
    let mut capture = filtered_capture(saturation, opacity, scale);
    let values = Arc::get_mut(&mut capture).unwrap();
    values.brightness = brightness;
    values.contrast = contrast;
    capture
}
fn tone_reference(
    source: [f64; 4],
    saturation: f64,
    brightness: f64,
    contrast: f64,
    opacity: f64,
    destination: [f64; 4],
) -> [u8; 4] {
    let alpha = source[3];
    let mut filtered = [0.; 4];
    if alpha > 0. {
        let rgb = [source[0] / alpha, source[1] / alpha, source[2] / alpha];
        let gray = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
        for i in 0..3 {
            let saturated = (gray + (rgb[i] - gray) * saturation).clamp(0., 1.);
            let brightened = (saturated * brightness).clamp(0., 1.);
            let contrasted = ((brightened - 0.5) * contrast + 0.5).clamp(0., 1.);
            filtered[i] = contrasted * alpha * opacity;
        }
    }
    filtered[3] = alpha * opacity;
    std::array::from_fn(|i| {
        ((filtered[i] + destination[i] * (1. - filtered[3])) * 255.).round() as u8
    })
}
#[test]
fn brightness_contrast_validate_api_and_core_bounds() {
    for value in [f32::NAN, f32::INFINITY, -0.01, 2.01] {
        assert!(LayerEffect::brightness(value).is_err());
        assert!(LayerEffect::contrast(value).is_err());
        assert!(
            LayerEffect::opacity(0.5)
                .unwrap()
                .with_brightness(value)
                .is_err()
        );
        assert!(
            LayerEffect::opacity(0.5)
                .unwrap()
                .with_contrast(value)
                .is_err()
        );
        for (b, c) in [(value, 1.), (1., value)] {
            let mut scene = Scene::default();
            let capture = tone_capture(1., b, c, 1., 1.);
            scene.begin_layer_capture(capture.clone(), viewport());
            scene.end_layer_capture(capture, viewport());
            scene.finish();
            assert!(
                scene
                    .layer_plans(size(DevicePixels(96), DevicePixels(64)))
                    .is_err()
            );
        }
    }
    for value in [0., 0.5, 1., 2.] {
        assert!(LayerEffect::brightness(value).is_ok());
        assert!(LayerEffect::contrast(value).is_ok());
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn tone_filters_match_ordered_straight_color_reference_and_preserve_alpha() {
    let pairs = [
        ([255, 0, 0, 255], [0, 0, 255, 128]),
        ([20, 180, 80, 64], [255, 80, 10, 191]),
        ([255, 0, 0, 0], [0, 80, 255, 96]),
    ];
    let settings = [
        (1., 0., 1.),
        (1., 1., 0.),
        (0.35, 0.6, 1.7),
        (2., 2., 0.5),
        (1., 1., 1.),
    ];
    for mut renderer in renderers() {
        for (first, second) in pairs {
            for (s, b, c) in settings {
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
                        let capture = tone_capture(s, b, c, opacity, scale);
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
                        pixel(
                            &image,
                            (48. * scale) as u32,
                            (24. * scale) as u32,
                            tone_reference(
                                associated_over(second, associated_over(first, [0.; 4])),
                                s as f64,
                                b as f64,
                                c as f64,
                                opacity as f64,
                                [0.2, 0.4, 0.6, 1.],
                            ),
                        );
                        pixel(
                            &image,
                            (10. * scale) as u32,
                            (10. * scale) as u32,
                            [51, 102, 153, 255],
                        );
                        pixel(
                            &image,
                            (4. * scale) as u32,
                            (4. * scale) as u32,
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
fn tone_changes_reuse_raw_capture_but_invalidate_filtered_parents() {
    let extent = size(DevicePixels(96), DevicePixels(64));
    for mut renderer in renderers() {
        let mut original = Scene::default();
        rect(&mut original, 0., 0., 96., 64., gpui::white());
        let capture = tone_capture(1., 0.5, 1.5, 1., 1.);
        original.begin_layer_capture(capture.clone(), viewport());
        let from = original.len();
        rect(
            &mut original,
            16.,
            16.,
            64.,
            32.,
            gpui::rgb(0x406080).into(),
        );
        let to = original.len();
        original.end_layer_capture(capture, viewport());
        original.finish();
        renderer.render_scene_to_image(&original, extent).unwrap();
        for (b, c) in [(1., 0.), (2., 0.5), (0.6, 1.8)] {
            let mut changed = Scene::default();
            rect(&mut changed, 0., 0., 96., 64., gpui::white());
            let capture = tone_capture(1., b, c, 0.4, 1.);
            changed.begin_layer_capture(capture.clone(), viewport());
            changed.replay(from..to, &original);
            changed.end_layer_capture(capture, viewport());
            changed.finish();
            let image = renderer.render_scene_to_image(&changed, extent).unwrap();
            pixel(
                &image,
                48,
                24,
                tone_reference(
                    [64. / 255., 96. / 255., 128. / 255., 1.],
                    1.,
                    b as f64,
                    c as f64,
                    0.4,
                    [1.; 4],
                ),
            );
        }
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 1,
                hits: 3
            }
        );
        let mut parent = Scene::default();
        rect(&mut parent, 0., 0., 96., 64., gpui::white());
        let outer = tone_capture(1., 1., 0.5, 1., 1.);
        parent.begin_layer_capture(outer.clone(), viewport());
        let inner = tone_capture(1., 0.5, 1., 1., 1.);
        parent.begin_layer_capture(inner.clone(), viewport());
        let from = parent.len();
        rect(&mut parent, 16., 16., 64., 32., gpui::rgb(0x406080).into());
        let to = parent.len();
        parent.end_layer_capture(inner, viewport());
        parent.end_layer_capture(outer, viewport());
        parent.finish();
        renderer.render_scene_to_image(&parent, extent).unwrap();
        let before = renderer.stats();
        let mut changed = Scene::default();
        rect(&mut changed, 0., 0., 96., 64., gpui::white());
        let outer = tone_capture(1., 1., 0.5, 1., 1.);
        changed.begin_layer_capture(outer.clone(), viewport());
        let inner = tone_capture(1., 1.5, 1., 1., 1.);
        changed.begin_layer_capture(inner.clone(), viewport());
        changed.replay(from..to, &parent);
        changed.end_layer_capture(inner, viewport());
        changed.end_layer_capture(outer, viewport());
        changed.finish();
        let image = renderer.render_scene_to_image(&changed, extent).unwrap();
        let inner = tone_reference(
            [64. / 255., 96. / 255., 128. / 255., 1.],
            1.,
            1.5,
            1.,
            1.,
            [0.; 4],
        );
        pixel(
            &image,
            48,
            24,
            tone_reference(inner.map(|v| v as f64 / 255.), 1., 1., 0.5, 1., [1.; 4]),
        );
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: before.captures + 1,
                hits: before.hits + 1
            }
        );
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn painted_tone_builders_preserve_fixed_filter_order() {
    use gpui::*;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(white()).child(
                crate::painted()
                    .w(px(80.))
                    .h(px(48.))
                    .bg(rgb(0xe86020))
                    .layer_effect(
                        LayerEffect::opacity(0.5)
                            .unwrap()
                            .with_contrast(0.5)
                            .unwrap()
                            .with_brightness(2.)
                            .unwrap(),
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
        (40. * scale) as u32,
        (24. * scale) as u32,
        tone_reference(
            [232. / 255., 96. / 255., 32. / 255., 1.],
            1.,
            2.,
            0.5,
            0.5,
            [1.; 4],
        ),
    );
}

#[path = "layer_invert_tests.rs"]
mod invert_tests;
