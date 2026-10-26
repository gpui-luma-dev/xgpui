use super::*;
use crate::{BlendMode, BrushMeshPoint, Noise};

pub(super) fn points(kind: usize) -> [BrushMeshPoint; 4] {
    let colors = [
        [1., 0., 0., 1.],
        [0., 0.2, 1., 1.],
        [0., 1., 0.3, 1.],
        [1., 0.6, 0., 1.],
    ];
    let mut points = std::array::from_fn(|i| {
        BrushMeshPoint::new(
            [(i % 2) as f32, (i / 2) as f32],
            [1., 0.],
            [0., 1.],
            colors[i],
        )
    });
    if kind == 1 {
        points[0].u_tangent[1] = 0.2;
        points[1].u_tangent[1] = -0.2;
    } else if kind == 3 {
        points[0].v_tangent[0] = 0.2;
        points[2].v_tangent[0] = -0.2;
    } else if kind == 2 {
        for p in &mut points {
            p.position = p.position.map(|v| 0.1 + v * 0.8);
            p.u_tangent = [0.8, 0.];
            p.v_tangent = [0., 0.8];
        }
    }
    points
}
#[test]
fn hermite_patch_validation_program_sharing_and_noise_restore() {
    for kind in 0..4 {
        let a = Brush::hermite_patch(points(kind)).unwrap();
        let b = Brush::hermite_patch(points(kind)).unwrap();
        assert_eq!(a.program.id(), b.program.id());
        #[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
        {
            let grid = crate::MeshGrid::new(
                2,
                2,
                points(kind)
                    .map(|p| crate::ControlPoint {
                        position: p.position,
                        u_tangent: p.u_tangent,
                        v_tangent: p.v_tangent,
                        color: [p.color[0], p.color[1], p.color[2]],
                    })
                    .to_vec(),
            )
            .unwrap();
            let converted = Brush::from_mesh_grid(&grid).unwrap();
            if kind == 0 {
                assert_eq!(converted.program.id(), base_program().unwrap().id());
            } else {
                assert_eq!(converted.program.id(), a.program.id());
            }
        }
        assert_eq!(std::mem::size_of::<BrushInstance>(), 128);
        let noisy = a
            .clone()
            .with_noise(Noise::simplex(0.04, 2, 0.4), BlendMode::Overlay)
            .unwrap();
        let clean = noisy
            .with_noise(Noise::dither(0.), BlendMode::Multiply)
            .unwrap();
        assert_eq!(a.program.id(), clean.program.id());
    }
    for value in [f32::NAN, f32::INFINITY, -3., 3.] {
        let mut p = points(0);
        p[0].u_tangent[0] = value;
        assert!(Brush::hermite_patch(p).is_err());
    }
    let mut folded = points(0);
    folded[0].u_tangent = [-1., 0.];
    assert!(Brush::hermite_patch(folded).is_err());
    let mut singular = points(0);
    singular[0].u_tangent = [0., 0.];
    assert!(Brush::hermite_patch(singular).is_err());
    let mut displaced = points(0);
    displaced[0].position = [-0.1, 0.];
    assert!(Brush::hermite_patch(displaced).is_err());
    let mut color = points(0);
    color[3].color[3] = f32::NAN;
    assert!(Brush::hermite_patch(color).is_err());
}
#[cfg(feature = "wgpu")]
fn renderers() -> Vec<Box<dyn gpui::PlatformHeadlessRenderer>> {
    let mut result: Vec<Box<dyn gpui::PlatformHeadlessRenderer>> =
        vec![Box::new(gpui_wgpu::WgpuHeadlessRenderer::new().unwrap())];
    #[cfg(target_os = "macos")]
    result.push(Box::new(
        gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
    ));
    #[cfg(target_os = "windows")]
    if std::env::var("XGPUI_BRUSH_NATIVE").as_deref() == Ok("1") {
        result.push(Box::new(gpui_windows::Dx11HeadlessRenderer::new().unwrap()));
    }
    result
}
#[cfg(feature = "wgpu")]
#[test]
fn hermite_patch_pixels_match_tessellated_legacy_geometry_and_color() {
    use gpui::{CustomDraw, DevicePixels, Quad};
    use std::sync::Arc;
    for mut renderer in renderers() {
        for kind in 0..4 {
            let p = points(kind);
            let brush = Brush::hermite_patch(p).unwrap();
            let grid = crate::MeshGrid::new(
                2,
                2,
                p.map(|p| crate::ControlPoint {
                    position: p.position,
                    u_tangent: p.u_tangent,
                    v_tangent: p.v_tangent,
                    color: [p.color[0], p.color[1], p.color[2]],
                })
                .to_vec(),
            )
            .unwrap();
            let legacy = Arc::new(crate::MeshGradient::new(grid, 64).unwrap());
            for scale in [1., 1.5, 2.] {
                let full = Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.))).scale(scale);
                let base = || {
                    let mut scene = Scene::default();
                    scene.insert_primitive(Quad {
                        bounds: full,
                        content_mask: ContentMask { bounds: full },
                        background: gpui::black().into(),
                        ..Default::default()
                    });
                    scene
                };
                let mut reference = base();
                reference.insert_primitive(CustomDraw {
                    order: 0,
                    bounds: bounds().scale(scale),
                    content_mask: ContentMask { bounds: full },
                    corner_radii: Default::default(),
                    opacity: 0.5,
                    content: legacy.clone(),
                    coverage: None,
                });
                reference.finish();
                let mut actual = base();
                let mut record = draw(&brush, &quad(), false, scale);
                record.opacity = 0.5;
                actual.insert_primitive(Primitive::Shader(record));
                actual.finish();
                let extent = size(
                    DevicePixels((120. * scale) as i32),
                    DevicePixels((80. * scale) as i32),
                );
                let expected = renderer.render_scene_to_image(&reference, extent).unwrap();
                let image = renderer.render_scene_to_image(&actual, extent).unwrap();
                for y in 18..39 {
                    for x in 20..76 {
                        let x = (x as f32 * scale) as u32;
                        let y = (y as f32 * scale) as u32;
                        for channel in 0..3 {
                            assert!(
                                image.get_pixel(x, y).0[channel]
                                    .abs_diff(expected.get_pixel(x, y).0[channel])
                                    <= 3,
                                "kind={kind} scale={scale} at {x},{y}: {:?} != {:?}",
                                image.get_pixel(x, y),
                                expected.get_pixel(x, y)
                            );
                        }
                    }
                }
                if kind == 1 {
                    let pixel = image.get_pixel((48. * scale) as u32, (8. * scale) as u32).0;
                    assert!(
                        pixel[..3].iter().all(|v| *v <= 1),
                        "curved top edge leaves transparent space"
                    );
                }
                if kind == 2 {
                    let pixel = image
                        .get_pixel((12. * scale) as u32, (24. * scale) as u32)
                        .0;
                    assert!(
                        pixel[..3].iter().all(|v| *v <= 1),
                        "inset patch leaves transparent space"
                    );
                }
                assert!(actual.custom_draws.is_empty());
            }
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn inset_hermite_patch_preserves_alpha_and_antialiases_its_own_boundary() {
    use gpui::DevicePixels;
    for mut renderer in renderers() {
        let mut p = points(2);
        for point in &mut p {
            point.position[0] += 0.003;
            point.position[1] += 0.017;
            point.color = [0.2, 0.4, 0.8, 0.5];
        }
        let brush = Brush::hermite_patch(p).unwrap();
        for scale in [1., 1.5, 2.] {
            let mut record = draw(&brush, &quad(), false, scale);
            record.opacity = 0.5;
            let mut scene = Scene::default();
            scene.insert_primitive(Primitive::Shader(record));
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
            for y in (8. * scale) as u32..(48. * scale) as u32 {
                for x in (8. * scale) as u32..(88. * scale) as u32 {
                    let target = [
                        ((x as f64 + 0.5) / scale as f64 - 8.) / 80.,
                        ((y as f64 + 0.5) / scale as f64 - 8.) / 40.,
                    ];
                    let uv = [
                        (target[0] - p[0].position[0] as f64) / 0.8_f32 as f64,
                        (target[1] - p[0].position[1] as f64) / 0.8_f32 as f64,
                    ];
                    let distance = (uv[0].min(1. - uv[0]) * 0.8 * 80. * scale as f64)
                        .min(uv[1].min(1. - uv[1]) * 0.8 * 40. * scale as f64);
                    let coverage = (0.5 + distance).clamp(0., 1.);
                    for (i, color) in [0.2, 0.4, 0.8].into_iter().enumerate() {
                        let expected = (color * 0.5 * 0.5 * coverage * 255_f64).round() as u8;
                        assert!(
                            image.get_pixel(x, y).0[i].abs_diff(expected) <= 1,
                            "scale={scale} at {x},{y}: {:?}, expected {expected}",
                            image.get_pixel(x, y)
                        );
                    }
                }
            }
        }
    }
}

pub(super) fn grid_points(width: u32, height: u32, moved: bool) -> Vec<BrushMeshPoint> {
    let mut points: Vec<_> = (0..width * height)
        .map(|i| {
            BrushMeshPoint::new(
                [
                    (i % width) as f32 / (width - 1) as f32,
                    (i / width) as f32 / (height - 1) as f32,
                ],
                [1. / (width - 1) as f32, 0.],
                [0., 1. / (height - 1) as f32],
                [
                    (i % 3) as f32 * 0.5,
                    ((i / 3) % 3) as f32 * 0.5,
                    ((i + 1) % 3) as f32 * 0.5,
                    1.,
                ],
            )
        })
        .collect();
    if moved && width == 3 && height == 3 {
        points[4].position = [0.525, 0.475];
        points[4].u_tangent = [0.5, 0.02];
        points[4].v_tangent = [-0.01, 0.5];
    }
    points
}
#[test]
fn multi_patch_validates_dimensions_shared_geometry_and_restores_noise() {
    for (w, h) in [(2, 2), (2, 3), (3, 2), (3, 3)] {
        let p = grid_points(w, h, true);
        let a = Brush::hermite_mesh(w, h, p.clone()).unwrap();
        let b = Brush::hermite_mesh(w, h, p).unwrap();
        assert_eq!(a.program.id(), b.program.id());
        assert_eq!(std::mem::size_of::<BrushInstance>(), 128);
        let noisy = a
            .clone()
            .with_noise(Noise::simplex(0.04, 2, 0.5), BlendMode::Overlay)
            .unwrap();
        assert_eq!(
            noisy
                .with_noise(Noise::dither(0.), BlendMode::Normal)
                .unwrap()
                .program
                .id(),
            a.program.id()
        );
    }
    for (w, h) in [(0, 3), (1, 3), (4, 3), (3, 4), (u32::MAX, u32::MAX)] {
        assert!(Brush::hermite_mesh(w, h, Vec::new()).is_err());
    }
    assert!(Brush::hermite_mesh(3, 3, grid_points(2, 2, false)).is_err());
    let mut p = grid_points(3, 3, false);
    p[4].position = [0.8, 0.2];
    assert!(Brush::hermite_mesh(3, 3, p).is_err());
    let mut p = grid_points(3, 3, false);
    p[4].u_tangent = [-0.5, 0.];
    assert!(Brush::hermite_mesh(3, 3, p).is_err());
}
#[cfg(feature = "wgpu")]
#[test]
fn multi_patch_dense_pixels_match_legacy_across_moved_joins() {
    use gpui::{CustomDraw, DevicePixels};
    use std::sync::Arc;
    for mut renderer in renderers() {
        for (w, h, moved) in [(3, 3, false), (3, 3, true), (2, 3, false), (3, 2, false)] {
            let p = grid_points(w, h, moved);
            let brush = Brush::hermite_mesh(w, h, p.clone()).unwrap();
            let grid = crate::MeshGrid::new(
                w,
                h,
                p.iter()
                    .map(|p| crate::ControlPoint {
                        position: p.position,
                        u_tangent: p.u_tangent,
                        v_tangent: p.v_tangent,
                        color: [p.color[0], p.color[1], p.color[2]],
                    })
                    .collect(),
            )
            .unwrap();
            assert_eq!(
                Brush::from_mesh_grid(&grid).unwrap().program.id(),
                brush.program.id()
            );
            let legacy = Arc::new(crate::MeshGradient::new(grid, 64).unwrap());
            for scale in [1., 1.5, 2.] {
                let mut shape = quad();
                shape.bounds.size = size(px(81.), px(41.));
                let full = Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.))).scale(scale);
                let mut expected = Scene::default();
                expected.insert_primitive(CustomDraw {
                    order: 0,
                    bounds: shape.bounds.scale(scale),
                    content_mask: ContentMask { bounds: full },
                    corner_radii: Default::default(),
                    opacity: 0.5,
                    content: legacy.clone(),
                    coverage: None,
                });
                expected.finish();
                let mut actual = Scene::default();
                let mut record = draw(&brush, &shape, false, scale);
                record.opacity = 0.5;
                actual.insert_primitive(Primitive::Shader(record));
                actual.finish();
                let extent = size(
                    DevicePixels((120. * scale) as i32),
                    DevicePixels((80. * scale) as i32),
                );
                let reference = renderer.render_scene_to_image(&expected, extent).unwrap();
                let image = renderer.render_scene_to_image(&actual, extent).unwrap();
                for y in (16. * scale) as u32..(43. * scale) as u32 {
                    for x in (16. * scale) as u32..(81. * scale) as u32 {
                        for c in 0..3 {
                            assert!(
                                image.get_pixel(x, y).0[c].abs_diff(reference.get_pixel(x, y).0[c])
                                    <= 3,
                                "{w}x{h} moved={moved} scale={scale} at {x},{y}: {:?} != {:?}",
                                image.get_pixel(x, y),
                                reference.get_pixel(x, y)
                            );
                        }
                    }
                }
            }
        }
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn multi_patch_translucent_joins_have_no_fade_or_overlap() {
    use gpui::DevicePixels;
    for mut renderer in renderers() {
        for moved in [false, true] {
            let mut p = grid_points(3, 3, moved);
            for p in &mut p {
                p.color = [0.2, 0.4, 0.8, 0.5];
            }
            let brush = Brush::hermite_mesh(3, 3, p).unwrap();
            for scale in [1., 1.5, 2.] {
                let mut shape = quad();
                shape.bounds.size = size(px(81.), px(41.));
                let mut record = draw(&brush, &shape, false, scale);
                record.opacity = 0.5;
                let mut scene = Scene::default();
                scene.insert_primitive(Primitive::Shader(record));
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
                for y in (16. * scale) as u32..(43. * scale) as u32 {
                    for x in (16. * scale) as u32..(81. * scale) as u32 {
                        for (c, expected) in [13u8, 26, 51].into_iter().enumerate() {
                            assert!(
                                image.get_pixel(x, y).0[c].abs_diff(expected) <= 1,
                                "seam at {x},{y}: {:?}",
                                image.get_pixel(x, y)
                            );
                        }
                    }
                }
            }
        }
    }
}
