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
    for value in [f32::NAN, f32::INFINITY, -1.] {
        let mut c = blur_capture(8., 1., 1.);
        Arc::get_mut(&mut c).unwrap().clip = Some(Bounds::new(
            point(ScaledPixels(8.), ScaledPixels(8.)),
            size(ScaledPixels(value), ScaledPixels(16.)),
        ));
        let mut scene = Scene::default();
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

    assert_eq!(plan.blur_size, [40, 24]);
    assert_eq!(plan.parameters[2][4], [1., 0., 2., 0.]);
    assert_eq!(plan.parameters[3][4], [0., 1., 2., 0.]);
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
fn cpu_gaussian(
    mut pixels: Vec<[f64; 4]>,
    width: usize,
    height: usize,
    radii: [f64; 2],
) -> Vec<[f64; 4]> {
    let radius = radii[0];
    if radius == 0. {
        return pixels;
    }
    for (axis, (dx, dy)) in [(1, 0), (0, 1)].into_iter().enumerate() {
        let radius = radii[axis];
        let n = radius.ceil() as isize;
        let sigma = (radius / 3.).max(0.1);
        let weights: Vec<_> = (-n..=n)
            .map(|i| (-0.5 * (i * i) as f64 / sigma.powi(2)).exp())
            .collect();
        let total: f64 = weights.iter().sum();
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
fn cpu_blur(pixels: Vec<[f64; 4]>, width: usize, height: usize, radius: f64) -> Vec<[f64; 4]> {
    cpu_blur_quality(pixels, width, height, radius, true)
}
fn cpu_blur_quality(
    pixels: Vec<[f64; 4]>,
    width: usize,
    height: usize,
    radius: f64,
    adaptive: bool,
) -> Vec<[f64; 4]> {
    let factor = if !adaptive || radius < 4. {
        1
    } else if radius < 8. {
        2
    } else {
        4
    };
    let rw = width.div_ceil(factor);
    let rh = height.div_ceil(factor);
    let sx = width as f64 / rw as f64;
    let sy = height as f64 / rh as f64;
    let mut reduced = vec![[0.; 4]; rw * rh];
    // Independent area resampling, expressed as overlap with source texel boxes.
    for y in 0..rh {
        for x in 0..rw {
            let left = x as f64 * sx;
            let right = (x + 1) as f64 * sx;
            let top = y as f64 * sy;
            let bottom = (y + 1) as f64 * sy;
            for iy in top.floor() as usize..(bottom.ceil() as usize).min(height) {
                for ix in left.floor() as usize..(right.ceil() as usize).min(width) {
                    let area = (right.min((ix + 1) as f64) - left.max(ix as f64))
                        * (bottom.min((iy + 1) as f64) - top.max(iy as f64));
                    for c in 0..4 {
                        reduced[y * rw + x][c] += pixels[iy * width + ix][c] * area / (sx * sy);
                    }
                }
            }
        }
    }
    let filtered = cpu_gaussian(reduced, rw, rh, [radius / sx, radius / sy]);
    let mut output = vec![[0.; 4]; width * height];
    for y in 0..height {
        for x in 0..width {
            let fx = (x as f64 + 0.5) / sx - 0.5;
            let fy = (y as f64 + 0.5) / sy - 0.5;
            let ix = fx.floor() as isize;
            let iy = fy.floor() as isize;
            for (ox, wx) in [(0, 1. - (fx - fx.floor())), (1, fx - fx.floor())] {
                for (oy, wy) in [(0, 1. - (fy - fy.floor())), (1, fy - fy.floor())] {
                    let xx = (ix + ox).clamp(0, rw as isize - 1) as usize;
                    let yy = (iy + oy).clamp(0, rh as isize - 1) as usize;
                    for c in 0..4 {
                        output[y * width + x][c] += filtered[yy * rw + xx][c] * wx * wy;
                    }
                }
            }
        }
    }
    output
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
    let expected = cpu_blur(
        cpu_blur(raw, 80, 48, 8.)
            .into_iter()
            .map(quantize)
            .collect(),
        80,
        48,
        4.,
    );
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

#[cfg(feature = "wgpu")]
#[test]
fn subtree_blur_quality_switches_resize_only_filter_buffers_on_odd_crops() {
    use gpui::LayerBlurQuality::{Adaptive, FullResolution};
    let extent = size(DevicePixels(96), DevicePixels(64));
    let (width, height) = (79, 47);
    let mut source = Scene::default();
    // Fine one-pixel stripes expose aliasing and center/extent mistakes.
    for x in 0..width {
        rect(
            &mut source,
            (8 + x) as f32,
            8.,
            1.,
            height as f32,
            gpui::rgba(if x % 2 == 0 { 0xff402080 } else { 0x2060e0c0 }).into(),
        );
    }
    source.finish();
    let raw: Vec<_> = (0..height)
        .flat_map(|_| {
            (0..width).map(|x| {
                over(
                    if x % 2 == 0 {
                        [255, 64, 32, 128]
                    } else {
                        [32, 96, 224, 192]
                    },
                    [0.; 4],
                )
            })
        })
        .collect();
    let settings = [
        (0., Adaptive),
        (3.9, Adaptive),
        (4., Adaptive),
        (7.9, Adaptive),
        (8., Adaptive),
        (8., FullResolution),
        (8., Adaptive),
        (0., Adaptive),
        (8., Adaptive),
    ];
    for mut renderer in renderers() {
        for (index, (radius, quality)) in settings.into_iter().enumerate() {
            let mut scene = Scene::default();
            rect(&mut scene, 0., 0., 96., 64., gpui::rgb(0x336699).into());
            let mut c = blur_capture(radius, 1., 0.7);
            let fields = Arc::get_mut(&mut c).unwrap();
            fields.bounds = Bounds::new(
                point(px(8.), px(8.)),
                size(px(width as f32), px(height as f32)),
            )
            .scale(1.);
            fields.blur_quality = quality;
            scene.begin_layer_capture(c.clone(), viewport());
            scene.replay(0..source.len(), &source);
            scene.end_layer_capture(c, viewport());
            scene.finish();
            let image = renderer.render_scene_to_image(&scene, extent).unwrap();
            let plan = scene.layer_plans(extent).unwrap().remove(0);
            let expected_bytes = 2 * width as u64 * height as u64 * 4
                + if radius > 0. {
                    2 * plan.blur_size[0] as u64 * plan.blur_size[1] as u64 * 4
                } else {
                    0
                };
            assert_eq!(
                renderer.layer_texture_bytes(),
                expected_bytes,
                "actual texture extents must follow the plan"
            );
            assert_eq!(
                renderer.stats(),
                LayerCacheStats {
                    captures: 1,
                    hits: index as u64
                }
            );
            let expected = cpu_blur_quality(
                raw.clone(),
                width,
                height,
                radius as f64,
                quality == Adaptive,
            );
            for y in (0..height).step_by(3) {
                for x in (0..width).step_by(3) {
                    let value = expected[y * width + x].map(|v| v * 0.7);
                    let bg = [0.2, 0.4, 0.6, 1.];
                    pixel(
                        &image,
                        x as u32 + 8,
                        y as u32 + 8,
                        std::array::from_fn(|i| {
                            ((value[i] + bg[i] * (1. - value[3])) * 255.).round() as u8
                        }),
                    );
                }
            }
            if radius == 8. && quality == Adaptive {
                assert_eq!(plan.blur_size, [20, 12]);
            }
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn subtree_blur_halo_clips_after_filtering_and_restores_destination() {
    for mut renderer in renderers() {
        for quality in [
            gpui::LayerBlurQuality::Adaptive,
            gpui::LayerBlurQuality::FullResolution,
        ] {
            for mode in [gpui::LayerBlendMode::Normal, gpui::LayerBlendMode::Multiply] {
                let mut scene = Scene::default();
                rect(&mut scene, 0., 0., 96., 64., gpui::rgb(0xc0a060).into());
                let mut c = blur_capture(16., 1., 1.);
                let fields = Arc::get_mut(&mut c).unwrap();
                fields.bounds = viewport().bounds;
                fields.clip =
                    Some(Bounds::new(point(px(24.), px(24.)), size(px(48.), px(16.))).scale(1.));
                fields.blur_quality = quality;
                fields.blend_mode = mode;
                scene.begin_layer_capture(c.clone(), viewport());
                rect(&mut scene, 0., 0., 96., 64., gpui::rgb(0x204060).into());
                scene.end_layer_capture(c, viewport());
                scene.finish();
                let image = renderer
                    .render_scene_to_image(&scene, size(DevicePixels(96), DevicePixels(64)))
                    .unwrap();
                for y in 0..64 {
                    for x in 0..96 {
                        let inside = (24..72).contains(&x) && (24..40).contains(&y);
                        let expected = if !inside {
                            [192, 160, 96, 255]
                        } else if mode == gpui::LayerBlendMode::Multiply {
                            [24, 40, 36, 255]
                        } else {
                            [32, 64, 96, 255]
                        };
                        pixel(&image, x, y, expected);
                    }
                }
            }
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn painted_subtree_blur_halo_samples_beyond_ancestor_clip() {
    use gpui::*;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0xc0a060)).child(
                div()
                    .relative()
                    .ml(px(24.))
                    .mt(px(24.))
                    .w(px(48.))
                    .h(px(24.))
                    .overflow_hidden()
                    .child(
                        crate::painted()
                            .absolute()
                            .left(px(-16.))
                            .top(px(-16.))
                            .w(px(80.))
                            .h(px(56.))
                            .bg(rgb(0x204060))
                            .layer_effect(
                                LayerEffect::blur(px(8.))
                                    .unwrap()
                                    .with_blend_mode(crate::BlendMode::Multiply),
                            )
                            .child(
                                canvas(
                                    |bounds, window, _| {
                                        let hitbox =
                                            window.insert_hitbox(bounds, HitboxBehavior::Normal);
                                        assert_eq!(
                                            hitbox.content_mask.bounds,
                                            Bounds::new(
                                                point(px(24.), px(24.)),
                                                size(px(48.), px(24.))
                                            )
                                        );
                                    },
                                    |_, _, _, _| {},
                                )
                                .size_full(),
                            ),
                    ),
            )
        }
    }
    let mut cx =
        HeadlessAppContext::with_platform(Arc::new(NoopTextSystem::new()), Arc::new(()), || {
            Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?)))
        });
    let window = cx
        .open_window(size(px(96.), px(80.)), |_, cx| cx.new(|_| View))
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let scale = cx
        .update_window(window.into(), |_, window, _| window.scale_factor())
        .unwrap();
    let image = cx.capture_screenshot(window.into()).unwrap();
    for (x, y) in [(24, 24), (71, 24), (24, 47), (71, 47), (48, 36)] {
        pixel(
            &image,
            (x as f32 * scale) as u32,
            (y as f32 * scale) as u32,
            [24, 40, 36, 255],
        );
    }
    for (x, y) in [(23, 24), (72, 24), (24, 23), (24, 48)] {
        pixel(
            &image,
            (x as f32 * scale) as u32,
            (y as f32 * scale) as u32,
            [192, 160, 96, 255],
        );
    }
}
