use super::*;
use gpui::{DevicePixels, Path, PathBuilder, PlatformHeadlessRenderer, Quad};

fn shape(kind: usize) -> Path<Pixels> {
    let mut p = if kind == 2 {
        PathBuilder::stroke(px(7.))
    } else {
        PathBuilder::fill()
    };
    p.move_to(point(px(12.), px(16.)));
    p.cubic_bezier_to(
        point(px(92.), px(24.)),
        point(px(32.), px(-2.)),
        point(px(76.), px(4.)),
    );
    p.curve_to(point(px(72.), px(64.)), point(px(104.), px(66.)));
    p.line_to(point(px(24.), px(58.)));
    p.close();
    if kind == 1 {
        // Opposite winding: a hole in the center of the fill.
        p.move_to(point(px(40.), px(28.)));
        p.line_to(point(px(40.), px(44.)));
        p.line_to(point(px(64.), px(44.)));
        p.line_to(point(px(64.), px(28.)));
        p.close();
    }
    p.build().unwrap()
}
fn clip() -> ContentMask<Pixels> {
    ContentMask {
        bounds: Bounds::new(point(px(0.), px(0.)), size(px(80.), px(72.))),
    }
}
#[test]
fn path_programs_keep_payload_and_split_coverage_batches() {
    let brush = Brush::linear(75., stops()).unwrap();
    let a = brush.path_draw(shape(0), clip(), 1., 1.).unwrap();
    let b = brush.path_draw(shape(2), clip(), 1., 1.).unwrap();
    assert!(a.program.uses_path_mask());
    assert!(!a.program.uses_glyph_mask());
    assert_eq!(a.program.id(), b.program.id());
    assert_eq!(a.data.len() * 16, 128);
    assert_eq!(std::mem::size_of_val(&a.instance_data()), 176);
    let mut invalid = a.clone();
    invalid.path_mask = None;
    assert!(invalid.validate_masks().is_err());
    let mut scene = Scene::default();
    scene.insert_primitive(Primitive::Shader(a));
    scene.insert_primitive(Primitive::Shader(b));
    let count = scene.len();
    scene.finish();
    assert_eq!(scene.batches().count(), 2);
    assert!(
        scene
            .batches()
            .all(|b| matches!(b,PrimitiveBatch::Shaders{range,..} if range.len()==1))
    );
    let mut replay = Scene::default();
    replay.replay(0..count, &scene);
    replay.finish();
    assert_eq!(replay.shader_draws[0].data, scene.shader_draws[0].data);
    assert!(std::sync::Arc::ptr_eq(
        replay.shader_draws[0].path_mask.as_ref().unwrap(),
        scene.shader_draws[0].path_mask.as_ref().unwrap()
    ));
    let mut invalid = shape(0);
    invalid.vertices[0].st_position.x = f32::NAN;
    assert!(brush.path_draw(invalid, clip(), 1., 1.).is_err());
    let mut oversized = shape(0);
    oversized
        .vertices
        .resize(65_538, oversized.vertices[0].clone());
    assert!(brush.path_draw(oversized, clip(), 1., 1.).is_err());
    for scale in [0., f32::NAN] {
        assert!(brush.path_draw(shape(0), clip(), 1., scale).is_err());
    }
    assert!(brush.path_draw(shape(0), clip(), f32::NAN, 1.).is_err());
}
#[cfg(feature = "wgpu")]
#[test]
fn path_brush_fields_match_rect_fields_times_native_coverage() {
    let mut renderers: Vec<Box<dyn PlatformHeadlessRenderer>> =
        vec![Box::new(gpui_wgpu::WgpuHeadlessRenderer::new().unwrap())];
    #[cfg(target_os = "macos")]
    renderers.push(Box::new(
        gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
    ));
    #[cfg(target_os = "windows")]
    if std::env::var("XGPUI_BRUSH_NATIVE").as_deref() == Ok("1") {
        renderers.push(Box::new(gpui_windows::Dx11HeadlessRenderer::new().unwrap()));
    }
    let brushes = [
        Brush::freeform(super::spatial_tests::expanded_points(16)).unwrap(),
        Brush::solid([0.8, 0.3, 0.1, 0.6]).unwrap(),
        Brush::linear(60., stops()).unwrap(),
        Brush::radial([0.4, 0.4], [0.7, 0.8], stops()).unwrap(),
        Brush::freeform(vec![
            crate::BrushPoint::new([0.2, 0.2], [1., 0., 0., 1.]),
            crate::BrushPoint::new([0.8, 0.8], [0., 0., 1., 0.4]),
        ])
        .unwrap(),
        Brush::mesh([
            [1., 0., 0., 1.],
            [0., 1., 0., 0.5],
            [0., 0., 1., 0.6],
            [1., 0., 1., 1.],
        ])
        .unwrap()
        .with_noise(
            crate::Noise::simplex(0.06, 2, 0.5),
            crate::BlendMode::Overlay,
        )
        .unwrap(),
    ];
    for renderer in &mut renderers {
        for kind in 0..3 {
            for scale in [1., 1.5, 2.] {
                let path = shape(kind);
                let full = Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.))).scale(scale);
                let mut native = Scene::default();
                native.insert_primitive(Quad {
                    bounds: full,
                    content_mask: ContentMask { bounds: full },
                    background: gpui::black().into(),
                    ..Default::default()
                });
                let mut mask = path.scale(scale);
                mask.color = gpui::white().into();
                mask.content_mask = clip().scale(scale);
                native.insert_primitive(mask);
                native.finish();
                let extent = size(
                    DevicePixels((120. * scale) as i32),
                    DevicePixels((80. * scale) as i32),
                );
                let coverage = renderer.render_scene_to_image(&native, extent).unwrap();
                if kind == 1 {
                    assert_eq!(
                        coverage
                            .get_pixel((52. * scale) as u32, (36. * scale) as u32)
                            .0[0],
                        0,
                        "native hole stays empty"
                    );
                }
                for brush in &brushes {
                    let mut brushed = Scene::default();
                    brushed.insert_primitive(Quad {
                        bounds: full,
                        content_mask: ContentMask { bounds: full },
                        background: gpui::black().into(),
                        ..Default::default()
                    });
                    brushed.insert_primitive(Primitive::Shader(
                        brush.path_draw(path.clone(), clip(), 0.5, scale).unwrap(),
                    ));
                    brushed.finish();
                    let image = renderer.render_scene_to_image(&brushed, extent).unwrap();
                    let quad = gpui::quad(
                        path.bounds,
                        Corners::default(),
                        gpui::transparent_black(),
                        gpui::Edges::default(),
                        gpui::transparent_black(),
                        BorderStyle::Solid,
                    );
                    let mut field = draw(brush, &quad, false, scale);
                    field.content_mask = clip().scale(scale);
                    field.opacity = 0.5;
                    let mut reference = Scene::default();
                    reference.insert_primitive(Quad {
                        bounds: full,
                        content_mask: ContentMask { bounds: full },
                        background: gpui::black().into(),
                        ..Default::default()
                    });
                    reference.insert_primitive(Primitive::Shader(field));
                    reference.finish();
                    let colors = renderer.render_scene_to_image(&reference, extent).unwrap();
                    let b = path.bounds.scale(scale);
                    let mut checked = 0;
                    for y in 0..image.height() {
                        for x in 0..image.width() {
                            // Avoid the reference quad's own analytic outer-edge coverage.
                            if (x as f32 + 0.5) <= b.left().0 + 1.
                                || (x as f32 + 0.5) >= b.right().0 - 1.
                                || (y as f32 + 0.5) <= b.top().0 + 1.
                                || (y as f32 + 0.5) >= b.bottom().0 - 1.
                            {
                                continue;
                            }
                            let mask = coverage.get_pixel(x, y).0[0] as f32 / 255.;
                            for c in 0..3 {
                                let expected = colors.get_pixel(x, y).0[c] as f32 * mask;
                                assert!(
                                    (image.get_pixel(x, y).0[c] as f32 - expected).abs() <= 3.,
                                    "kind {kind}, scale {scale}, {x},{y}, channel {c}"
                                );
                            }
                            checked += 1;
                        }
                    }
                    assert!(checked > 1000);
                    assert_eq!(
                        image
                            .get_pixel((100. * scale) as u32, (40. * scale) as u32)
                            .0[0],
                        0,
                        "clipped path does not leak"
                    );
                }
            }
        }
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn painted_path_uses_inherited_opacity_and_retained_group_filters() {
    use gpui::*;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(white()).child(
                div().opacity(0.5).child(
                    crate::painted()
                        .w(px(96.))
                        .h(px(72.))
                        .layer_effect(
                            crate::LayerEffect::opacity(0.5)
                                .unwrap()
                                .with_grayscale(1.)
                                .unwrap(),
                        )
                        .child(
                            canvas(
                                |_, _, _| (),
                                |_, (), window, _| {
                                    Brush::solid([1., 0., 0., 1.])
                                        .unwrap()
                                        .paint_path(window, shape(1))
                                        .unwrap();
                                },
                            )
                            .size_full(),
                        ),
                ),
            )
        }
    }
    let mut cx = HeadlessAppContext::with_platform(
        std::sync::Arc::new(NoopTextSystem::new()),
        std::sync::Arc::new(()),
        || Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?))),
    );
    let window = cx
        .open_window(size(px(120.), px(80.)), |_, cx| cx.new(|_| View))
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let scale = cx
        .update_window(window.into(), |_, window, _| window.scale_factor())
        .unwrap();
    let image = cx.capture_screenshot(window.into()).unwrap();
    let actual = image
        .get_pixel((28. * scale) as u32, (36. * scale) as u32)
        .0;
    for channel in &actual[..3] {
        assert!(
            channel.abs_diff(205) <= 2,
            "inherited opacity and grayscale: {actual:?}"
        );
    }
    assert_eq!(
        image
            .get_pixel((52. * scale) as u32, (36. * scale) as u32)
            .0,
        [255; 4]
    );
}

#[cfg(feature = "wgpu")]
#[test]
fn path_masks_do_not_bleed_between_draws_and_native_siblings() {
    let mut renderers: Vec<Box<dyn PlatformHeadlessRenderer>> =
        vec![Box::new(gpui_wgpu::WgpuHeadlessRenderer::new().unwrap())];
    #[cfg(target_os = "macos")]
    renderers.push(Box::new(
        gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
    ));
    #[cfg(target_os = "windows")]
    if std::env::var("XGPUI_BRUSH_NATIVE").as_deref() == Ok("1") {
        renderers.push(Box::new(gpui_windows::Dx11HeadlessRenderer::new().unwrap()));
    }
    for renderer in &mut renderers {
        let mut scenes = [Scene::default(), Scene::default()];
        for (native, scene) in scenes.iter_mut().enumerate() {
            let full = Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.))).scale(1.);
            scene.insert_primitive(Quad {
                bounds: full,
                content_mask: ContentMask { bounds: full },
                background: gpui::black().into(),
                ..Default::default()
            });
            for (kind, rgba) in [(1, [1., 0., 0., 0.5]), (2, [0., 0., 1., 0.5])] {
                let path = shape(kind);
                if native == 0 {
                    scene.insert_primitive(Primitive::Shader(
                        Brush::solid(rgba)
                            .unwrap()
                            .path_draw(path, clip(), 1., 1.)
                            .unwrap(),
                    ));
                } else {
                    let mut path = path.scale(1.);
                    path.content_mask = clip().scale(1.);
                    path.color = gpui::Rgba {
                        r: rgba[0],
                        g: rgba[1],
                        b: rgba[2],
                        a: rgba[3],
                    }
                    .into();
                    scene.insert_primitive(path);
                }
                if kind == 1 {
                    let bounds =
                        Bounds::new(point(px(40.), px(28.)), size(px(24.), px(16.))).scale(1.);
                    scene.insert_primitive(Quad {
                        bounds,
                        content_mask: ContentMask { bounds },
                        background: gpui::rgb(0x00ff00).into(),
                        ..Default::default()
                    });
                }
            }
            scene.finish();
        }
        let extent = size(DevicePixels(120), DevicePixels(80));
        let actual = renderer.render_scene_to_image(&scenes[0], extent).unwrap();
        let expected = renderer.render_scene_to_image(&scenes[1], extent).unwrap();
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            for channel in 0..4 {
                assert!(a.0[channel].abs_diff(b.0[channel]) <= 2, "{a:?} != {b:?}");
            }
        }
    }
}
