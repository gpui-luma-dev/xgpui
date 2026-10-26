use super::*;
use crate::{BlendMode, BrushPoint, Noise};

#[derive(Clone)]
enum Field {
    Freeform(Vec<BrushPoint>),
    Mesh([[f32; 4]; 4]),
    HermiteInset([[f32; 4]; 4]),
    HermiteGrid(Vec<crate::BrushMeshPoint>),
}
impl Field {
    fn brush(&self) -> Brush {
        match self {
            Self::Freeform(points) => Brush::freeform(points.clone()),
            Self::Mesh(colors) => Brush::mesh(*colors),
            Self::HermiteGrid(points) => Brush::hermite_mesh(3, 3, points.clone()),
            Self::HermiteInset(colors) => {
                let mut points = super::hermite_tests::points(2);
                for (point, color) in points.iter_mut().zip(colors) {
                    point.color = *color;
                }
                Brush::hermite_patch(points)
            }
        }
        .unwrap()
    }
    // Independent double-precision reference in associated encoded sRGB.
    fn associated(&self, uv: [f64; 2]) -> [f64; 4] {
        let (colors, weights): (Vec<[f32; 4]>, Vec<f64>) = match self {
            Self::HermiteGrid(points) => {
                if uv.iter().any(|v| !(0. ..=1.).contains(v)) {
                    return [0.; 4];
                }
                let x = (uv[0] * 2.).floor().min(1.) as usize;
                let y = (uv[1] * 2.).floor().min(1.) as usize;
                let i = y * 3 + x;
                return Self::Mesh([
                    points[i].color,
                    points[i + 1].color,
                    points[i + 3].color,
                    points[i + 4].color,
                ])
                .associated([uv[0] * 2. - x as f64, uv[1] * 2. - y as f64]);
            }
            Self::HermiteInset(colors) => {
                let mapped = uv.map(|v| (v - 0.1_f32 as f64) / 0.8_f32 as f64);
                if mapped.iter().any(|v| !(0. ..=1.).contains(v)) {
                    return [0.; 4];
                }
                return Self::Mesh(*colors).associated(mapped);
            }
            Self::Freeform(points) => {
                let distances: Vec<_> = points
                    .iter()
                    .map(|p| {
                        (uv[0] - p.position[0] as f64).powi(2)
                            + (uv[1] - p.position[1] as f64).powi(2)
                    })
                    .collect();
                if let Some(index) = distances.iter().position(|d| *d == 0.) {
                    let c = points[index].color;
                    return [
                        c[0] as f64 * c[3] as f64,
                        c[1] as f64 * c[3] as f64,
                        c[2] as f64 * c[3] as f64,
                        c[3] as f64,
                    ];
                }
                let weights: Vec<_> = distances.iter().map(|d| 1. / d).collect();
                let sum: f64 = weights.iter().sum();
                (
                    points.iter().map(|p| p.color).collect(),
                    weights.iter().map(|w| w / sum).collect(),
                )
            }
            Self::Mesh(colors) => {
                let u = uv[0].clamp(0., 1.);
                let v = uv[1].clamp(0., 1.);
                let h = |t: f64| 2. * t.powi(3) - 3. * t.powi(2) + 1.;
                (
                    colors.to_vec(),
                    vec![
                        h(u) * h(v),
                        (1. - h(u)) * h(v),
                        h(u) * (1. - h(v)),
                        (1. - h(u)) * (1. - h(v)),
                    ],
                )
            }
        };
        std::array::from_fn(|i| {
            colors
                .iter()
                .zip(&weights)
                .map(|(c, w)| c[i] as f64 * if i == 3 { 1. } else { c[3] as f64 } * w)
                .sum()
        })
    }
}
pub(super) fn expanded_points(count: usize) -> Vec<BrushPoint> {
    (0..count)
        .map(|i| {
            BrushPoint::new(
                [0.05 + (i % 4) as f32 * 0.3, 0.05 + (i / 4) as f32 * 0.3],
                [
                    (i % 3) as f32 * 0.5,
                    ((i + 1) % 3) as f32 * 0.5,
                    ((i + 2) % 3) as f32 * 0.5,
                    if i == 0 {
                        0.
                    } else {
                        0.2 + (i % 4) as f32 * 0.25
                    },
                ],
            )
        })
        .collect()
}

fn fields() -> [Field; 8] {
    [
        Field::Freeform(vec![BrushPoint::new([0.5, 0.5], [0.2, 0.4, 0.8, 0.6])]),
        Field::Freeform(vec![
            BrushPoint::new([0.15, 0.2], [1., 0., 0., 0.6]),
            BrushPoint::new([0.85, 0.25], [0., 0.4, 1., 0.9]),
            BrushPoint::new([0.45, 0.85], [0.1, 1., 0.1, 0.4]),
        ]),
        Field::Freeform(vec![
            BrushPoint::new([0.2, 0.5], [1., 0., 0., 0.]),
            BrushPoint::new([0.8, 0.5], [0., 0., 1., 0.8]),
        ]),
        Field::Freeform(expanded_points(4)),
        Field::Freeform(expanded_points(16)),
        Field::HermiteGrid({
            let mut p = super::hermite_tests::grid_points(3, 3, false);
            for (i, p) in p.iter_mut().enumerate() {
                p.color[3] = if i == 0 { 0. } else { 0.2 + i as f32 * 0.08 };
            }
            p
        }),
        Field::HermiteInset([
            [1., 0., 0., 0.],
            [0., 0.2, 1., 0.4],
            [0., 1., 0.3, 0.8],
            [1., 0.6, 0., 1.],
        ]),
        Field::Mesh([
            [1., 0., 0., 0.6],
            [0., 0.3, 1., 0.9],
            [0., 1., 0.1, 0.2],
            [1., 0.5, 0., 0.8],
        ]),
    ]
}

#[test]
fn spatial_validation_rejects_unrepresentable_inputs() {
    assert!(Brush::freeform(vec![]).is_err());
    let mut too_many = expanded_points(16);
    too_many.push(BrushPoint::new([0.5, 0.5], [1.; 4]));
    assert!(Brush::freeform(too_many).is_err());
    let mut invalid_color = expanded_points(16);
    invalid_color[15].color[3] = f32::NAN;
    assert!(Brush::freeform(invalid_color).is_err());
    assert!(Brush::freeform(vec![BrushPoint::new([0.; 2], [1.; 4]); 4]).is_err());
    assert!(Brush::freeform(vec![BrushPoint::new([f32::NAN, 0.], [1.; 4])]).is_err());
    assert!(Brush::freeform(vec![BrushPoint::new([-0.1, 0.], [1.; 4])]).is_err());
    assert!(Brush::freeform(vec![BrushPoint::new([0.; 2], [f32::NAN; 4])]).is_err());
    assert!(Brush::freeform(vec![BrushPoint::new([0.; 2], [1.; 4]); 2]).is_err());
    assert!(
        Brush::freeform(vec![
            BrushPoint::new([0.; 2], [1.; 4]),
            BrushPoint::new([0.00001, 0.], [1.; 4])
        ])
        .is_err()
    );
    assert!(Brush::mesh([[f32::NAN; 4]; 4]).is_err());
    assert!(Brush::mesh([[2.; 4]; 4]).is_err());
    let constant = Brush::freeform(vec![BrushPoint::new([0.5; 2], [0.2, 0.4, 0.8, 0.7])]).unwrap();
    assert_eq!(constant.parameters.options[0], -1.);
    #[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
    {
        let grid = crate::MeshGrid::regular(2, 2, vec![[0.1, 0.4, 0.8]; 4]).unwrap();
        assert!(Brush::from_mesh_grid(&grid).is_ok());
        assert!(
            Brush::from_mesh_grid(&crate::MeshGrid::regular(4, 2, vec![[1.; 3]; 8]).unwrap())
                .is_err()
        );
        let mut points = grid.points().to_vec();
        points[0].u_tangent = [-1., 0.];
        assert!(Brush::from_mesh_grid(&crate::MeshGrid::new(2, 2, points).unwrap()).is_err());
    }
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
fn spatial_pixels_match_reference_for_fills_borders_noise_and_fractional_dpi() {
    use gpui::{DevicePixels, Quad};
    for renderer in &mut renderers() {
        for field in fields() {
            for noisy in [false, true] {
                let brush = field.brush();
                let brush = if noisy {
                    brush
                        .with_noise(Noise::simplex(0.04, 2, 0.35), BlendMode::Overlay)
                        .unwrap()
                } else {
                    brush
                };
                for border in [false, true] {
                    for scale in [1., 1.5, 2.] {
                        let mut scene = Scene::default();
                        let mut record = draw(&brush, &quad(), border, scale);
                        record.opacity = 0.5;
                        record.content_mask.bounds.size.width = gpui::ScaledPixels(70. * scale);
                        let full = Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.)))
                            .scale(scale);
                        scene.insert_primitive(Quad {
                            bounds: full,
                            content_mask: ContentMask { bounds: full },
                            background: gpui::black().into(),
                            ..Default::default()
                        });
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
                        let samples = if border {
                            [(10., 24.), (40., 10.), (60., 10.)]
                        } else {
                            [(24., 24.), (40., 30.), (60., 36.)]
                        };
                        for (x, y) in samples {
                            let ix = (x * scale) as u32;
                            let iy = (y * scale) as u32;
                            let local = [
                                (ix as f64 + 0.5) / scale as f64 - 8.,
                                (iy as f64 + 0.5) / scale as f64 - 8.,
                            ];
                            let associated = field.associated([local[0] / 80., local[1] / 40.]);
                            let pixel = image.get_pixel(ix, iy).0;
                            for i in 0..3 {
                                let mut channel = associated[i];
                                if noisy && associated[3] > 0. {
                                    let frequency = 0.04_f32 as f64;
                                    let signal = (super::noise_tests::signal(
                                        crate::NoiseKind::Simplex,
                                        [local[0] * frequency, local[1] * frequency],
                                    ) + 0.5
                                        * super::noise_tests::signal(
                                            crate::NoiseKind::Simplex,
                                            [local[0] * frequency * 2., local[1] * frequency * 2.],
                                        ))
                                        / 1.5;
                                    let base = channel / associated[3];
                                    channel = (base
                                        + (super::noise_tests::blended(
                                            base,
                                            signal,
                                            BlendMode::Overlay,
                                        ) - base)
                                            * 0.35_f32 as f64)
                                        * associated[3];
                                }
                                let expected = (channel * 0.5 * 255.).round() as u8;
                                assert!(
                                    pixel[i].abs_diff(expected) <= 2,
                                    "spatial border={border} noisy={noisy} scale={scale} pixel={pixel:?} channel={i} expected={expected}"
                                );
                            }
                        }
                        assert_eq!(
                            &image
                                .get_pixel((76. * scale) as u32, (30. * scale) as u32)
                                .0[..3],
                            &[0, 0, 0]
                        );
                        assert!(scene.custom_draws.is_empty());
                    }
                }
            }
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn spatial_text_uses_the_same_color_field_and_existing_atlas() {
    use crate::brush::text::TextBrush;
    use gpui::{AtlasKey, DevicePixels, FontId, GlyphId, Quad, RenderGlyphParams};
    use std::borrow::Cow;
    for renderer in &mut renderers() {
        let tile = renderer
            .sprite_atlas()
            .get_or_insert_with(
                AtlasKey::Glyph(RenderGlyphParams {
                    font_id: FontId(0),
                    glyph_id: GlyphId(991),
                    font_size: px(16.),
                    subpixel_variant: point(0, 0),
                    scale_factor: 1.,
                    is_emoji: false,
                    subpixel_rendering: false,
                    dilation: 0,
                }),
                &mut || {
                    Ok(Some((
                        size(DevicePixels(16), DevicePixels(16)),
                        Cow::Owned(vec![255; 256]),
                    )))
                },
            )
            .unwrap()
            .unwrap();
        for field in fields() {
            let brush = TextBrush::new(field.brush()).unwrap();
            let mut scene = Scene::default();
            let full = Bounds::new(point(px(0.), px(0.)), size(px(96.), px(32.))).scale(1.);
            scene.insert_primitive(Quad {
                bounds: full,
                content_mask: ContentMask { bounds: full },
                background: gpui::black().into(),
                ..Default::default()
            });
            for x in [8., 48.] {
                scene.insert_primitive(Primitive::Shader(super::text_tests::masked_draw(
                    &brush,
                    super::text_tests::glyph(tile, x, 1.),
                    1.,
                )));
            }
            scene.finish();
            let image = renderer
                .render_scene_to_image(&scene, size(DevicePixels(96), DevicePixels(32)))
                .unwrap();
            for x in [12, 52] {
                let expected = field.associated([(x as f64 + 0.5 - 8.) / 64., (12.5 - 8.) / 16.]);
                let pixel = image.get_pixel(x, 12).0;
                for i in 0..3 {
                    assert!(
                        pixel[i].abs_diff((expected[i] * 0.5 * 255.).round() as u8) <= 2,
                        "spatial text {pixel:?}, expected={expected:?}"
                    );
                }
            }
            assert!(scene.custom_draws.is_empty());
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn regular_mesh_matches_the_existing_hermite_producer() {
    use gpui::{CustomDraw, DevicePixels, Quad};
    use std::sync::Arc;
    let grid = crate::MeshGrid::regular(
        2,
        2,
        vec![[1., 0., 0.], [0., 0.3, 1.], [0., 1., 0.1], [1., 0.5, 0.]],
    )
    .unwrap();
    let brush = Brush::from_mesh_grid(&grid).unwrap();
    let producer = Arc::new(crate::MeshGradient::new(grid, 64).unwrap());
    for renderer in &mut renderers() {
        let mut legacy = Scene::default();
        let full = Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.))).scale(1.);
        legacy.insert_primitive(Quad {
            bounds: full,
            content_mask: ContentMask { bounds: full },
            background: gpui::black().into(),
            ..Default::default()
        });
        legacy.insert_primitive(CustomDraw {
            order: 0,
            bounds: bounds().scale(1.),
            content_mask: ContentMask { bounds: full },
            corner_radii: Default::default(),
            opacity: 1.,
            content: producer.clone(),
            coverage: None,
        });
        legacy.finish();
        let mut direct = Scene::default();
        direct.insert_primitive(Quad {
            bounds: full,
            content_mask: ContentMask { bounds: full },
            background: gpui::black().into(),
            ..Default::default()
        });
        direct.insert_primitive(Primitive::Shader(draw(&brush, &quad(), false, 1.)));
        direct.finish();
        let extent = size(DevicePixels(120), DevicePixels(80));
        let expected = renderer.render_scene_to_image(&legacy, extent).unwrap();
        let actual = renderer.render_scene_to_image(&direct, extent).unwrap();
        for (x, y) in [(24, 24), (40, 30), (60, 36)] {
            for i in 0..3 {
                assert!(
                    actual.get_pixel(x, y).0[i].abs_diff(expected.get_pixel(x, y).0[i]) <= 2,
                    "legacy Hermite color mismatch at {x},{y}"
                );
            }
        }
        assert!(direct.custom_draws.is_empty());
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn freeform_control_point_samples_are_exact_and_finite() {
    use gpui::{DevicePixels, Quad};
    for renderer in &mut renderers() {
        for alpha in [0., 0.4, 1.] {
            let mut points = expanded_points(16);
            points[0] = BrushPoint::new([0.5, 0.5], [1., 0., 0., alpha]);
            let brush = Brush::freeform(points).unwrap();
            let mut shape = quad();
            shape.bounds.size = size(px(81.), px(39.));
            let mut record = draw(&brush, &shape, false, 1.);
            record.opacity = 0.5;
            let mut scene = Scene::default();
            let full = Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.))).scale(1.);
            scene.insert_primitive(Quad {
                bounds: full,
                content_mask: ContentMask { bounds: full },
                background: gpui::black().into(),
                ..Default::default()
            });
            scene.insert_primitive(Primitive::Shader(record));
            scene.finish();
            let image = renderer
                .render_scene_to_image(&scene, size(DevicePixels(120), DevicePixels(80)))
                .unwrap();
            let pixel = image.get_pixel(48, 27).0;
            assert!(pixel[0].abs_diff((alpha * 0.5 * 255.).round() as u8) <= 1);
            assert_eq!(
                &pixel[1..3],
                &[0, 0],
                "neighbor color must not contaminate an exact control point"
            );
        }
    }
}

#[test]
fn expanded_freeform_shares_exact_sets_and_preserves_field_across_noise_changes() {
    use crate::brush::cache::ProgramCache;
    Brush::freeform(expanded_points(8)).unwrap();
    let points = expanded_points(16);
    let brush = Brush::freeform(points.clone()).unwrap();
    let same = Brush::freeform(points.clone()).unwrap();
    assert_eq!(brush.program.id(), same.program.id());
    assert_ne!(brush.program.id(), base_program().unwrap().id());
    assert_eq!(std::mem::size_of::<BrushInstance>(), 128);
    let mut changed = points.clone();
    changed[15].color[0] = 0.12345679;
    let changed = Brush::freeform(changed).unwrap();
    assert_ne!(brush.program.id(), changed.program.id());
    let noisy = brush
        .clone()
        .with_noise(Noise::simplex(0.04, 2, 0.4), BlendMode::Overlay)
        .unwrap();
    let same_noisy = same
        .with_noise(Noise::simplex(0.08, 2, 0.9), BlendMode::Overlay)
        .unwrap();
    assert_eq!(noisy.program.id(), same_noisy.program.id());
    let clean = noisy
        .with_noise(Noise::worley(0.04, 0.), BlendMode::Screen)
        .unwrap();
    assert_eq!(
        clean.program.id(),
        brush.program.id(),
        "zero noise restores the specialized field"
    );
    let mut scene = Scene::default();
    for brush in [&brush, &brush, &changed] {
        scene.insert_primitive(Primitive::Shader(draw(brush, &quad(), false, 1.)));
    }
    scene.finish();
    assert_eq!(
        scene.batches().count(),
        2,
        "identical fields batch; different point constants split batches"
    );
    let glyph = crate::brush::text::TextBrush::new(brush.clone()).unwrap();
    let same_glyph = crate::brush::text::TextBrush::new(brush.clone()).unwrap();
    assert_eq!(glyph.program.id(), same_glyph.program.id());
    let mut cache: ProgramCache<u8, 2> = ProgramCache::default();
    cache.insert(1, brush.program.clone());
    cache.insert(2, changed.program.clone());
    let retained = cache.get(&1).unwrap();
    cache.insert(3, base_program().unwrap());
    assert!(
        cache.get(&2).is_none(),
        "least recently used entry is evicted"
    );
    assert!(cache.get(&1).is_some());
    assert!(
        retained.pack(&brush.parameters).is_ok(),
        "eviction leaves retained programs valid"
    );
}
