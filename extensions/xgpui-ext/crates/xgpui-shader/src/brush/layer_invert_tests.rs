use super::*;

fn inverted_capture(amount: f32, opacity: f32, scale: f32) -> Arc<LayerCapture> {
    let mut capture = tone_capture(0.35, 0.6, 1.7, opacity, scale);
    Arc::get_mut(&mut capture).unwrap().invert = amount;
    capture
}
// Independent reference unassociates before each color operation; the shader
// complements associated RGB around alpha without division.
fn reference(source: [f64; 4], amount: f64, opacity: f64, destination: [f64; 4]) -> [u8; 4] {
    let alpha = source[3];
    let rgb = if alpha > 0. {
        [source[0] / alpha, source[1] / alpha, source[2] / alpha]
    } else {
        [0.; 3]
    };
    let gray = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
    let filtered: [f64; 4] = std::array::from_fn(|i| {
        if i == 3 {
            return alpha * opacity;
        }
        let saturated = (gray + (rgb[i] - gray) * 0.35).clamp(0., 1.);
        let brightened = (saturated * 0.6).clamp(0., 1.);
        let contrasted = ((brightened - 0.5) * 1.7 + 0.5).clamp(0., 1.);
        (contrasted * (1. - amount) + (1. - contrasted) * amount) * alpha * opacity
    });
    std::array::from_fn(|i| {
        ((filtered[i] + destination[i] * (1. - filtered[3])) * 255.).round() as u8
    })
}
#[test]
fn inversion_validates_api_and_capture_and_preserves_uniform_size() {
    for amount in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        assert!(LayerEffect::invert(amount).is_err());
        assert!(
            LayerEffect::opacity(0.5)
                .unwrap()
                .with_invert(amount)
                .is_err()
        );
        let mut scene = Scene::default();
        let capture = inverted_capture(amount, 1., 1.);
        scene.begin_layer_capture(capture.clone(), viewport());
        scene.end_layer_capture(capture, viewport());
        scene.finish();
        assert!(
            scene
                .layer_plans(size(DevicePixels(96), DevicePixels(64)))
                .is_err()
        );
    }
    for amount in [0., 0.25, 0.5, 1.] {
        assert!(LayerEffect::invert(amount).is_ok());
        let mut scene = Scene::default();
        let capture = inverted_capture(amount, 1., 1.);
        scene.begin_layer_capture(capture.clone(), viewport());
        scene.end_layer_capture(capture, viewport());
        scene.finish();
        let plans = scene
            .layer_plans(size(DevicePixels(96), DevicePixels(64)))
            .unwrap();
        assert_eq!(std::mem::size_of_val(&plans[0].parameters[1]), 96);
        assert_eq!(plans[0].parameters[1][5][2], amount);
    }
    assert_eq!(gpui::LayerColorFilter::default().invert, 0.);
}
#[cfg(feature = "wgpu")]
#[test]
fn inversion_matches_completed_translucent_subtree_and_fixed_filter_order() {
    for mut renderer in renderers() {
        for (first, second) in [
            ([255, 0, 0, 255], [0, 0, 255, 128]),
            ([20, 180, 80, 64], [255, 80, 10, 191]),
            ([255, 0, 0, 0], [0, 80, 255, 0]),
        ] {
            for amount in [0., 0.25, 0.5, 1.] {
                for opacity in [0.4, 1.] {
                    for scale in [1., 1.5, 2.] {
                        let full = ContentMask {
                            bounds: viewport().bounds.map(|p| ScaledPixels(p.0 * scale)),
                        };
                        let mut scene = Scene::default();
                        scene.insert_primitive(Quad {
                            bounds: full.bounds,
                            content_mask: full,
                            background: gpui::rgba(0x33669980).into(),
                            ..Default::default()
                        });
                        let capture = inverted_capture(amount, opacity, scale);
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
                        let destination = associated_over([51, 102, 153, 128], [0., 0., 0., 1.]);
                        pixel(
                            &image,
                            (48. * scale) as u32,
                            (24. * scale) as u32,
                            reference(
                                associated_over(second, associated_over(first, [0.; 4])),
                                amount as f64,
                                opacity as f64,
                                destination,
                            ),
                        );
                        // Empty captured pixels remain transparent even at full inversion.
                        pixel(
                            &image,
                            (10. * scale) as u32,
                            (10. * scale) as u32,
                            destination.map(|v| (v * 255.).round() as u8),
                        );
                    }
                }
            }
        }
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn inversion_changes_reuse_raw_capture_and_invalidate_parent() {
    let extent = size(DevicePixels(96), DevicePixels(64));
    for mut renderer in renderers() {
        let mut source = Scene::default();
        rect(&mut source, 16., 16., 64., 32., gpui::rgb(0x406080).into());
        source.finish();
        let frame = |amount, nested| {
            let mut scene = Scene::default();
            rect(&mut scene, 0., 0., 96., 64., gpui::white());
            let outer = capture(1.);
            if nested {
                scene.begin_layer_capture(outer.clone(), viewport());
            }
            let inner = inverted_capture(amount, 0.4, 1.);
            scene.begin_layer_capture(inner.clone(), viewport());
            scene.replay(0..source.len(), &source);
            scene.end_layer_capture(inner, viewport());
            if nested {
                scene.end_layer_capture(outer, viewport());
            }
            scene.finish();
            scene
        };
        renderer
            .render_scene_to_image(&frame(0., false), extent)
            .unwrap();
        for amount in [0.5, 1., 0.] {
            let image = renderer
                .render_scene_to_image(&frame(amount, false), extent)
                .unwrap();
            pixel(
                &image,
                48,
                24,
                reference(
                    [64. / 255., 96. / 255., 128. / 255., 1.],
                    amount as f64,
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
        renderer
            .render_scene_to_image(&frame(0., true), extent)
            .unwrap();
        let before = renderer.stats();
        let image = renderer
            .render_scene_to_image(&frame(1., true), extent)
            .unwrap();
        pixel(
            &image,
            48,
            24,
            reference([64. / 255., 96. / 255., 128. / 255., 1.], 1., 0.4, [1.; 4]),
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
fn painted_invert_builder_filters_the_surface_and_children() {
    use gpui::*;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(white()).child(
                crate::painted()
                    .w(px(80.))
                    .h(px(48.))
                    .bg(rgb(0xff0000))
                    .layer_effect(LayerEffect::invert(1.).unwrap())
                    .child(div().w(px(32.)).h(px(32.)).bg(rgb(0x0000ff))),
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
        [255, 255, 0, 255],
    );
    pixel(
        &image,
        (48. * scale) as u32,
        (16. * scale) as u32,
        [0, 255, 255, 255],
    );
}
