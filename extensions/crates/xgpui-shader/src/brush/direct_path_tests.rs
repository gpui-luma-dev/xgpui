use super::path_tests::shape;
use super::*;
use gpui::{DevicePixels, Path, PlatformHeadlessRenderer, Quad};
fn clip() -> ContentMask<Pixels> {
    ContentMask {
        bounds: Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.))),
    }
}
#[test]
fn direct_path_geometry_programs_and_zero_mask_transport() {
    let brush = Brush::linear(75., stops()).unwrap();
    for kind in 0..3 {
        let path = shape(kind);
        let a = brush
            .direct_path_draw(&path, clip(), 1., 1.)
            .unwrap()
            .unwrap_or_else(|| {
                panic!(
                    "demo geometry {kind} must support direct coverage ({} vertices)",
                    path.vertices.len()
                )
            });
        assert!(!a.program.uses_path_mask());
        assert!(a.path_mask.is_none() && a.glyph_mask.is_none());
        assert_eq!(std::mem::size_of_val(&a.instance_data()), 176);
        assert!(!a.program.wgsl().contains("texture_2d"));
        assert_eq!(
            a.program.id(),
            brush
                .direct_path_draw(&path, clip(), 0.4, 2.)
                .unwrap()
                .unwrap()
                .program
                .id()
        );
        let mut scene = Scene::default();
        scene.insert_primitive(Primitive::Shader(a.clone()));
        scene.insert_primitive(Primitive::Shader(a));
        let count = scene.len();
        scene.finish();
        assert_eq!(scene.batches().count(), 1);
        let mut replay = Scene::default();
        replay.replay(0..count, &scene);
        replay.finish();
        assert!(replay.shader_draws.iter().all(|d| d.path_mask.is_none()));
    }
    let mut analytic = Path::new(point(px(0.), px(0.)));
    analytic.curve_to(point(px(40.), px(40.)), point(px(40.), px(0.)));
    assert!(
        brush
            .direct_path_draw(&analytic, clip(), 1., 1.)
            .unwrap()
            .is_none()
    );
    let mut overlapping = shape(0);
    overlapping.vertices.extend(overlapping.vertices.clone());
    assert!(
        brush
            .direct_path_draw(&overlapping, clip(), 1., 1.)
            .unwrap()
            .is_none()
    );
}

// Independent polygon/pixel clipping reference, without the shader's edge integral.
fn coverage(path: &Path<Pixels>, x: u32, y: u32, scale: f64) -> f64 {
    let mut area = 0.;
    for triangle in path.vertices.chunks_exact(3) {
        let mut polygon: Vec<[f64; 2]> = triangle
            .iter()
            .map(|v| {
                [
                    f32::from(v.xy_position.x) as f64 * scale,
                    f32::from(v.xy_position.y) as f64 * scale,
                ]
            })
            .collect();
        for (axis, bound, greater) in [
            (0, x as f64, true),
            (0, x as f64 + 1., false),
            (1, y as f64, true),
            (1, y as f64 + 1., false),
        ] {
            let old = std::mem::take(&mut polygon);
            if old.is_empty() {
                break;
            }
            for i in 0..old.len() {
                let a = old[i];
                let b = old[(i + 1) % old.len()];
                let inside = |p: [f64; 2]| {
                    if greater {
                        p[axis] >= bound
                    } else {
                        p[axis] <= bound
                    }
                };
                if inside(a) {
                    polygon.push(a);
                }
                if inside(a) != inside(b) {
                    let t = (bound - a[axis]) / (b[axis] - a[axis]);
                    polygon.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
                }
            }
        }
        if polygon.len() >= 3 {
            let sum: f64 = (0..polygon.len())
                .map(|i| {
                    let a = polygon[i];
                    let b = polygon[(i + 1) % polygon.len()];
                    a[0] * b[1] - a[1] * b[0]
                })
                .sum();
            area += sum.abs() * 0.5;
        }
    }
    area.clamp(0., 1.)
}
fn edge_cases() -> Vec<Path<Pixels>> {
    let mut thin = gpui::PathBuilder::fill();
    thin.move_to(point(px(20.25), px(16.25)));
    thin.line_to(point(px(20.5), px(16.25)));
    thin.line_to(point(px(20.5), px(20.25)));
    thin.line_to(point(px(20.25), px(20.25)));
    thin.close();
    let mut junction = Path::new(point(px(20.25), px(16.25)));
    for points in [
        [[0., 0.], [4., 0.], [0., 4.]],
        [[4., 0.], [4., 4.], [2., 2.]],
        [[4., 4.], [0., 4.], [2., 2.]],
    ] {
        let p = points.map(|p| point(px(p[0] + 20.25), px(p[1] + 16.25)));
        junction.push_triangle(
            (p[0], p[1], p[2]),
            (point(0., 1.), point(0., 1.), point(0., 1.)),
        );
    }
    vec![thin.build().unwrap(), junction]
}
#[cfg(feature = "wgpu")]
#[test]
fn direct_path_pixels_match_exact_polygon_area_without_internal_seams() {
    let brush = Brush::solid([0.8, 0.2, 0.4, 0.6]).unwrap();
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
        for (kind, path) in [shape(0), shape(1), shape(2)]
            .into_iter()
            .chain(edge_cases())
            .enumerate()
        {
            for scale in [1., 1.5, 2.] {
                let mut scene = Scene::default();
                let bounds = clip().bounds.scale(scale);
                scene.insert_primitive(Quad {
                    bounds,
                    content_mask: ContentMask { bounds },
                    background: gpui::black().into(),
                    ..Default::default()
                });
                let draw = brush
                    .direct_path_draw(&path, clip(), 0.7, scale)
                    .unwrap()
                    .unwrap();
                scene.insert_primitive(Primitive::Shader(draw));
                scene.finish();
                let image = renderer
                    .render_scene_to_image(
                        &scene,
                        size(
                            DevicePixels((120. * scale) as i32),
                            DevicePixels((80. * scale) as i32),
                        ),
                    )
                    .unwrap();
                for y in 0..image.height() {
                    for x in 0..image.width() {
                        let a = coverage(&path, x, y, scale as f64) * 0.6 * 0.7;
                        let expected = [
                            (0.8 * a * 255.).round() as u8,
                            (0.2 * a * 255.).round() as u8,
                            (0.4 * a * 255.).round() as u8,
                            255,
                        ];
                        let actual = image.get_pixel(x, y).0;
                        for i in 0..4 {
                            assert!(
                                actual[i].abs_diff(expected[i]) <= 2,
                                "kind={kind} scale={scale} {x},{y}: {actual:?} != {expected:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn direct_path_gallery_fill_stroke_and_many_shape_fixtures() {
    let brush = Brush::solid([1.; 4]).unwrap();
    for stroke in [false, true] {
        let mut p = if stroke {
            gpui::PathBuilder::stroke(px(12.))
        } else {
            gpui::PathBuilder::fill()
        };
        p.move_to(point(px(16.), px(26.)));
        p.cubic_bezier_to(
            point(px(188.), px(32.)),
            point(px(62.), px(-4.)),
            point(px(162.), px(4.)),
        );
        p.curve_to(point(px(166.), px(114.)), point(px(208.), px(112.)));
        p.line_to(point(px(38.), px(108.)));
        p.close();
        if !stroke {
            p.move_to(point(px(78.), px(48.)));
            p.line_to(point(px(78.), px(82.)));
            p.line_to(point(px(130.), px(82.)));
            p.line_to(point(px(130.), px(48.)));
            p.close();
        }
        assert!(
            brush
                .direct_path_draw(&p.build().unwrap(), clip(), 1., 1.)
                .unwrap()
                .is_some(),
            "gallery stroke={stroke}"
        );
    }
    let mut p = gpui::PathBuilder::fill();
    p.move_to(point(px(4.), px(4.)));
    p.cubic_bezier_to(
        point(px(40.), px(36.)),
        point(px(48.), px(0.)),
        point(px(0.), px(44.)),
    );
    p.line_to(point(px(4.), px(36.)));
    p.close();
    assert!(
        brush
            .direct_path_draw(&p.build().unwrap(), clip(), 1., 1.)
            .unwrap()
            .is_some()
    );
}

#[cfg(feature = "wgpu")]
#[test]
fn direct_path_fields_keep_original_coordinates_across_material_variants() {
    let brushes = [
        Brush::linear(75., stops()).unwrap(),
        Brush::radial([0.4, 0.3], [0.8, 0.9], stops())
            .unwrap()
            .with_noise(
                crate::Noise::simplex(0.07, 2, 0.4),
                crate::BlendMode::Overlay,
            )
            .unwrap(),
        Brush::freeform(vec![
            crate::BrushPoint::new([0.2, 0.2], [1., 0., 0., 0.6]),
            crate::BrushPoint::new([0.8, 0.8], [0., 0., 1., 0.8]),
        ])
        .unwrap(),
        Brush::hermite_mesh(3, 3, super::hermite_tests::grid_points(3, 3, true)).unwrap(),
    ];
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new().unwrap();
    let path = shape(1);
    let quad = gpui::quad(
        path.bounds,
        Corners::default(),
        gpui::transparent_black(),
        gpui::Edges::default(),
        gpui::transparent_black(),
        BorderStyle::Solid,
    );
    for brush in &brushes {
        for scale in [1., 1.5, 2.] {
            let render = |draw: ShaderDraw, renderer: &mut gpui_wgpu::WgpuHeadlessRenderer| {
                let mut scene = Scene::default();
                let bounds = clip().bounds.scale(scale);
                scene.insert_primitive(Quad {
                    bounds,
                    content_mask: ContentMask { bounds },
                    background: gpui::black().into(),
                    ..Default::default()
                });
                scene.insert_primitive(Primitive::Shader(draw));
                scene.finish();
                renderer
                    .render_scene_to_image(
                        &scene,
                        size(
                            DevicePixels((120. * scale) as i32),
                            DevicePixels((80. * scale) as i32),
                        ),
                    )
                    .unwrap()
            };
            let direct = render(
                brush
                    .direct_path_draw(&path, clip(), 0.7, scale)
                    .unwrap()
                    .unwrap(),
                &mut renderer,
            );
            let mut rectangle = super::draw(brush, &quad, false, scale);
            rectangle.opacity = 0.7;
            let field = render(rectangle, &mut renderer);
            let mut compared = 0;
            for y in (0..direct.height()).step_by(2) {
                for x in (0..direct.width()).step_by(2) {
                    if coverage(&path, x, y, scale as f64) > 0.999999 {
                        let position = point(px(x as f32 / scale), px(y as f32 / scale));
                        if !path.bounds.dilate(px(-2. / scale)).contains(&position) {
                            continue;
                        }
                        let actual = direct.get_pixel(x, y).0;
                        let expected = field.get_pixel(x, y).0;
                        assert!(
                            actual
                                .into_iter()
                                .zip(expected)
                                .all(|(a, b)| a.abs_diff(b) <= 2),
                            "scale={scale} {x},{y}: {actual:?} != {expected:?}"
                        );
                        compared += 1;
                    }
                }
            }
            assert!(compared > 100);
        }
    }
}

#[test]
fn direct_path_prepared_offsets_keep_one_program_and_batch() {
    let brush = Brush::linear(75., stops()).unwrap();
    let prepared = brush.prepare_path(&shape(1)).unwrap();
    for scale in [1., 1.5, 2.] {
        let mut scene = Scene::default();
        let mut program = None;
        for i in 0..24 {
            let draw = prepared
                .draw(point(px(i as f32 * 0.125), px(0.)), clip(), 0.7, scale)
                .unwrap();
            if let Some(id) = program {
                assert_eq!(draw.program.id(), id);
            }
            program = Some(draw.program.id());
            assert!(draw.path_mask.is_none());
            scene.insert_primitive(Primitive::Shader(draw));
        }
        scene.finish();
        assert!(
            matches!(scene.batches().next(),Some(PrimitiveBatch::Shaders {range,..}) if range.len()==24)
        );
        assert_eq!(scene.batches().count(), 1);
    }
    assert!(
        prepared
            .draw(point(px(f32::NAN), px(0.)), clip(), 1., 1.)
            .is_err()
    );
    assert!(
        prepared
            .draw(point(px(0.), px(0.)), clip(), 1., 0.)
            .is_err()
    );
}

#[cfg(feature = "wgpu")]
#[test]
fn direct_path_overlap_composes_each_path_once() {
    let brush = Brush::solid([0.8, 0.2, 0.4, 0.6]).unwrap();
    let path = shape(1);
    let prepared = brush.prepare_path(&path).unwrap();
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new().unwrap();
    let mut scene = Scene::default();
    let bounds = clip().bounds.scale(1.);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: gpui::black().into(),
        ..Default::default()
    });
    for _ in 0..2 {
        scene.insert_primitive(Primitive::Shader(
            prepared
                .draw(point(px(0.), px(0.)), clip(), 0.7, 1.)
                .unwrap(),
        ));
    }
    scene.finish();
    assert!(
        matches!(scene.batches().last(),Some(PrimitiveBatch::Shaders {range,..}) if range.len()==2)
    );
    let image = renderer
        .render_scene_to_image(&scene, size(DevicePixels(120), DevicePixels(80)))
        .unwrap();
    for y in 0..80 {
        for x in 0..120 {
            let a = coverage(&path, x, y, 1.) * 0.42;
            let alpha = a * (2. - a);
            let expected = [
                (0.8 * alpha * 255.).round() as u8,
                (0.2 * alpha * 255.).round() as u8,
                (0.4 * alpha * 255.).round() as u8,
                255,
            ];
            let actual = image.get_pixel(x, y).0;
            assert!(
                actual
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 2),
                "{x},{y}: {actual:?} != {expected:?}"
            );
        }
    }
}
