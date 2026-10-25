use super::*;

fn blur_capture(radius: f32, scale: f32, opacity: f32) -> Arc<LayerCapture> {
    let mut c = capture(opacity);
    let c_mut = Arc::get_mut(&mut c).unwrap();
    c_mut.blur_radius = ScaledPixels(radius * scale);
    c_mut.bounds = c_mut.bounds.map(|v| ScaledPixels(v.0 * scale));
    c
}
#[test]
fn subtree_blur_validates_radius_and_plans() {
    for radius in [-0.1, 16.01, f32::NAN, f32::INFINITY] {
        assert!(LayerEffect::blur(px(radius)).is_err());
    }
    for radius in [0., 0.1, 8., 16.] {
        assert!(LayerEffect::blur(px(radius)).is_ok());
    }
    for radius in [-0.1, 64.01, f32::NAN, f32::INFINITY] {
        let mut scene = Scene::default();
        let c = blur_capture(radius, 1., 1.);
        scene.begin_layer_capture(c.clone(), viewport());
        scene.end_layer_capture(c, viewport());
        scene.finish();
        assert!(
            scene
                .layer_plans(size(DevicePixels(96), DevicePixels(64)))
                .is_err()
        );
    }
    let mut scene = Scene::default();
    let c = blur_capture(4., 2., 0.5);
    let mask = ContentMask {
        bounds: viewport().bounds.map(|v| ScaledPixels(v.0 * 2.)),
    };
    scene.begin_layer_capture(c.clone(), mask);
    scene.end_layer_capture(c, mask);
    scene.finish();
    let plan = scene
        .layer_plans(size(DevicePixels(192), DevicePixels(128)))
        .unwrap()
        .remove(0);
    assert_eq!(plan.blur_radius, 8.);
    assert!(plan.needs_blur(None));
    assert!(!plan.needs_blur(Some(&plan)));
    let mut changed = plan.clone();
    changed.blur_radius = 4.;
    assert!(changed.matches(&plan));
    assert!(changed.needs_blur(Some(&plan)));
    changed.blur_radius = 0.;
    assert!(!changed.needs_blur(Some(&plan)));
    changed.blur_radius = 8.;
    changed.revision = Arc::from([123]);
    assert!(changed.needs_blur(Some(&plan)));

    assert_eq!(plan.parameters[2][4], [1., 0., 8., 0.]);
    assert_eq!(plan.parameters[3][4], [0., 1., 8., 0.]);
}
fn quantize(value: [f64; 4]) -> [f64; 4] {
    value.map(|v| (v * 255.).round() / 255.)
}
fn over(color: [u8; 4], destination: [f64; 4]) -> [f64; 4] {
    let a = color[3] as f64 / 255.;
    quantize(std::array::from_fn(|i| {
        if i == 3 {
            a + destination[i] * (1. - a)
        } else {
            color[i] as f64 / 255. * a + destination[i] * (1. - a)
        }
    }))
}
fn cpu_blur(mut pixels: Vec<[f64; 4]>, width: usize, height: usize, radius: f64) -> Vec<[f64; 4]> {
    if radius == 0. {
        return pixels;
    }
    let n = radius.ceil() as isize;
    let sigma = (radius / 3.).max(0.1);
    let weights: Vec<_> = (-n..=n)
        .map(|i| (-0.5 * (i * i) as f64 / sigma.powi(2)).exp())
        .collect();
    let total: f64 = weights.iter().sum();
    for (dx, dy) in [(1, 0), (0, 1)] {
        let mut out = vec![[0.; 4]; pixels.len()];
        for y in 0..height {
            for x in 0..width {
                let mut value = [0.; 4];
                for (offset, weight) in (-n..=n).zip(&weights) {
                    let sx = x as isize + offset * dx;
                    let sy = y as isize + offset * dy;
                    if sx >= 0 && sy >= 0 && sx < width as isize && sy < height as isize {
                        for i in 0..4 {
                            value[i] +=
                                pixels[sy as usize * width + sx as usize][i] * weight / total;
                        }
                    }
                }
                out[y * width + x] = quantize(value);
            }
        }
        pixels = out;
    }
    pixels
}
#[cfg(feature = "wgpu")]
#[test]
fn subtree_blur_matches_gaussian_reference_at_fractional_dpi() {
    for mut renderer in renderers() {
        for scale in [1., 1.5, 2.] {
            let full = ContentMask {
                bounds: viewport().bounds.map(|v| ScaledPixels(v.0 * scale)),
            };
            let width = (80. * scale) as usize;
            let height = (48. * scale) as usize;
            let mut raw = vec![[0.; 4]; width * height];
            let shapes = [
                (8., 8., 36., 40., [230, 40, 20, 255]),
                (32., 24., 36., 16., [20, 80, 230, 128]),
            ];
            for y in 0..height {
                for x in 0..width {
                    let wx = (x as f32 + 0.5) / scale + 8.;
                    let wy = (y as f32 + 0.5) / scale + 8.;
                    for (left, top, w, h, color) in shapes {
                        if wx >= left && wx < left + w && wy >= top && wy < top + h {
                            raw[y * width + x] = over(color, raw[y * width + x]);
                        }
                    }
                }
            }
            for radius in [0., 1.25, 4., 16.] {
                let mut scene = Scene::default();
                scene.insert_primitive(Quad {
                    bounds: full.bounds,
                    content_mask: full,
                    background: gpui::rgb(0x336699).into(),
                    ..Default::default()
                });
                let c = blur_capture(radius, scale, 0.6);
                scene.begin_layer_capture(c.clone(), full);
                for (left, top, w, h, color) in shapes {
                    scene.insert_primitive(Quad {
                        bounds: Bounds::new(point(px(left), px(top)), size(px(w), px(h)))
                            .scale(scale),
                        content_mask: ContentMask { bounds: c.bounds },
                        background: gpui::rgba(u32::from_be_bytes(color)).into(),
                        ..Default::default()
                    });
                }
                scene.end_layer_capture(c, full);
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
                let expected = cpu_blur(raw.clone(), width, height, (radius * scale) as f64);
                for y in (0..height).step_by(3) {
                    for x in (0..width).step_by(3) {
                        let src = expected[y * width + x].map(|v| v * 0.6);
                        let base = [0.2, 0.4, 0.6, 1.];
                        let rgba = std::array::from_fn(|i| {
                            ((src[i] + base[i] * (1. - src[3])) * 255.).round() as u8
                        });
                        pixel(
                            &image,
                            x as u32 + (8. * scale) as u32,
                            y as u32 + (8. * scale) as u32,
                            rgba,
                        );
                    }
                }
                pixel(
                    &image,
                    (4. * scale) as u32,
                    (24. * scale) as u32,
                    [51, 102, 153, 255],
                );
            }
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn subtree_blur_radius_edits_reuse_raw_source_and_compose_over_live_destination() {
    let extent = size(DevicePixels(96), DevicePixels(64));
    let mut source = Scene::default();
    rect(&mut source, 32., 16., 32., 32., gpui::rgb(0xe62814).into());
    source.finish();
    let raw: Vec<_> = (0..48)
        .flat_map(|y| {
            (0..80).map(move |x| {
                if (24..56).contains(&x) && (8..40).contains(&y) {
                    [230. / 255., 40. / 255., 20. / 255., 1.]
                } else {
                    [0.; 4]
                }
            })
        })
        .collect();
    for mut renderer in renderers() {
        for (index, radius) in [4., 4., 8., 0., 4.].into_iter().enumerate() {
            let destination = if index % 2 == 0 { 0x336699 } else { 0xc06030 };
            let mut scene = Scene::default();
            rect(&mut scene, 0., 0., 96., 64., gpui::rgb(destination).into());
            let mut c = blur_capture(radius, 1., 0.5);
            let fields = Arc::get_mut(&mut c).unwrap();
            fields.invert = 1.;
            fields.blend_mode = gpui::LayerBlendMode::Multiply;
            scene.begin_layer_capture(c.clone(), viewport());
            scene.replay(0..source.len(), &source);
            scene.end_layer_capture(c, viewport());
            scene.finish();
            let image = renderer.render_scene_to_image(&scene, extent).unwrap();
            let expected = cpu_blur(raw.clone(), 80, 48, radius as f64);
            let bg = [
                ((destination >> 16) & 255) as f64 / 255.,
                ((destination >> 8) & 255) as f64 / 255.,
                (destination & 255) as f64 / 255.,
            ];
            for (x, y) in [(30, 24), (32, 24), (48, 32), (64, 24)] {
                let blurred = expected[(y - 8) * 80 + x - 8];
                let rgba = std::array::from_fn(|i| {
                    if i == 3 {
                        255
                    } else {
                        // Inversion on associated RGB, group opacity, then multiply
                        // against an opaque destination (all three terms matter).
                        let source = (blurred[3] - blurred[i]) * 0.5;
                        ((source * bg[i] + bg[i] * (1. - blurred[3] * 0.5)) * 255.).round() as u8
                    }
                });
                pixel(&image, x as u32, y as u32, rgba);
            }
        }
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 1,
                hits: 4
            }
        );
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn painted_subtree_blur_softens_children_and_respects_ancestor_clip() {
    use gpui::*;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(white()).child(
                div()
                    .w(px(40.))
                    .h(px(48.))
                    .overflow_hidden()
                    .opacity(0.5)
                    .child(
                        crate::painted()
                            .flex_shrink_0()
                            .w(px(80.))
                            .h(px(48.))
                            .layer_effect(
                                LayerEffect::opacity(0.5)
                                    .unwrap()
                                    .with_blur(px(8.))
                                    .unwrap(),
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
    let soft = image
        .get_pixel((34. * scale) as u32, (16. * scale) as u32)
        .0;
    assert!(
        soft[0] < 250 && soft[0] >= 190,
        "blur must spread blue child coverage with group opacity: {soft:?}"
    );
    assert_eq!(soft[2], 255);
    pixel(&image, (44. * scale) as u32, (16. * scale) as u32, [255; 4]);
    pixel(
        &image,
        (16. * scale) as u32,
        (16. * scale) as u32,
        [191, 191, 255, 255],
    );
}

#[cfg(feature = "wgpu")]
#[test]
fn subtree_blur_nested_radius_edit_invalidates_parent_but_reuses_inner_source() {
    let extent = size(DevicePixels(96), DevicePixels(64));
    let mut source = Scene::default();
    rect(&mut source, 32., 16., 32., 32., gpui::rgb(0xe62814).into());
    source.finish();
    let frame = |radius| {
        let mut scene = Scene::default();
        rect(&mut scene, 0., 0., 96., 64., gpui::white());
        let outer = blur_capture(4., 1., 1.);
        scene.begin_layer_capture(outer.clone(), viewport());
        let inner = blur_capture(radius, 1., 1.);
        scene.begin_layer_capture(inner.clone(), viewport());
        scene.replay(0..source.len(), &source);
        scene.end_layer_capture(inner, viewport());
        scene.end_layer_capture(outer, viewport());
        scene.finish();
        scene
    };
    let raw: Vec<_> = (0..48)
        .flat_map(|y| {
            (0..80).map(move |x| {
                if (24..56).contains(&x) && (8..40).contains(&y) {
                    [230. / 255., 40. / 255., 20. / 255., 1.]
                } else {
                    [0.; 4]
                }
            })
        })
        .collect();
    let expected = cpu_blur(cpu_blur(raw, 80, 48, 8.), 80, 48, 4.);
    for mut renderer in renderers() {
        renderer.render_scene_to_image(&frame(2.), extent).unwrap();
        let before = renderer.stats();
        let image = renderer.render_scene_to_image(&frame(8.), extent).unwrap();
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: before.captures + 1,
                hits: before.hits + 1
            }
        );
        for x in 27..38 {
            let source = expected[16 * 80 + x - 8];
            pixel(
                &image,
                x as u32,
                24,
                std::array::from_fn(|i| ((source[i] + 1. - source[3]) * 255.).round() as u8),
            );
        }
    }
}
