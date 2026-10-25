use super::*;
use gpui::{ContentMask, Primitive, PrimitiveBatch, ScaledPixels, Scene, ShaderDraw, point, size};
fn stop(rgb: [f64; 3], alpha: f64) -> LinearRgba {
    LinearRgba::new(LinearRgb::new(RgbSpace::DisplayP3, rgb).unwrap(), alpha).unwrap()
}
fn gradient(interpolation: ColorInterpolation, mapping: GamutMapping) -> WideGradient {
    WideGradient::linear(
        [0.2, 0.],
        [0.8, 0.],
        [stop([0., 0., 1.], 0.25), stop([1., 1., 0.], 0.75)],
        interpolation,
        mapping,
    )
    .unwrap()
}
fn draw(painter: &WidePainter, parameters: Parameters) -> ShaderDraw {
    let mut data = [[0.; 4]; gpui::SHADER_DATA_SLOTS];
    for (i, word) in bytemuck::bytes_of(&parameters)
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
    {
        data[i / 4][i % 4] = f32::from_ne_bytes(*word);
    }
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(16.), ScaledPixels(8.)),
    );
    ShaderDraw {
        path_mask: None,
        glyph_mask: None,
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        opacity: 1.,
        scale_factor: 1.,
        program: painter.program.scene_program(),
        data,
    }
}
#[test]
fn shared_program_batches_and_layout_fits() {
    let painter = WidePainter::new().unwrap();
    let another = WidePainter::new().unwrap();
    assert_eq!(painter.program.id(), another.program.id());
    assert_eq!(std::mem::size_of::<Parameters>(), 80);
    let gradient = gradient(ColorInterpolation::Oklab, GamutMapping::ReduceChroma);
    let mut scene = Scene::default();
    for _ in 0..5000 {
        scene.insert_primitive(Primitive::Shader(draw(&painter, gradient.parameters)));
    }
    scene.finish();
    assert!(scene.custom_draws.is_empty());
    let batches: Vec<_> = scene.batches().collect();
    assert!(matches!(&batches[..],[PrimitiveBatch::Shaders{range,..}] if *range==(0..5000)));
}
#[test]
fn validates_uploads_and_degenerate_axis() {
    let colors = [stop([1., 0., 0.], 1.), stop([0., 1., 0.], 1.)];
    let make = |start, end| {
        WideGradient::linear(
            start,
            end,
            colors,
            ColorInterpolation::LinearRgb,
            GamutMapping::Clip,
        )
    };
    assert_eq!(
        make([0.; 2], [0.; 2]).unwrap().parameters.axis,
        [0., 0., 1., 0.]
    );
    assert!(make([f64::NAN, 0.], [1., 0.]).is_err());
    assert!(make([0.; 2], [f64::INFINITY, 0.]).is_err());
    assert!(make([0.; 2], [1e-20, 0.]).is_err());
    let colors = [stop([65505., 0., 0.], 1.), colors[1]];
    assert!(
        WideGradient::linear(
            [0.; 2],
            [1., 0.],
            colors,
            ColorInterpolation::Oklab,
            GamutMapping::Clip
        )
        .is_err()
    );
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn native_gradients_match_cpu_mapping_alpha_hue_clip_and_rounding() {
    use gpui::{DevicePixels, PlatformHeadlessRenderer, Quad, WindowColorMode};
    #[cfg(target_os = "macos")]
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    #[cfg(target_os = "windows")]
    let mut renderer = gpui_windows::Dx11HeadlessRenderer::new().unwrap();
    renderer
        .set_color_mode(WindowColorMode::WideGamutSdr)
        .unwrap();
    let painter = WidePainter::new().unwrap();
    let models = [
        ColorInterpolation::LinearRgb,
        ColorInterpolation::Oklab,
        ColorInterpolation::Oklch(HueInterpolation::Shorter),
        ColorInterpolation::Oklch(HueInterpolation::Longer),
        ColorInterpolation::Oklch(HueInterpolation::Increasing),
        ColorInterpolation::Oklch(HueInterpolation::Decreasing),
    ];
    for model in models {
        for policy in [GamutMapping::Clip, GamutMapping::ReduceChroma] {
            for end in [[0.8, 0.], [0.2, 0.]] {
                let start = [0.2, 0.];
                let colors = [stop([0., 0., 1.], 0.25), stop([1., 1., 0.], 0.75)];
                let gradient = WideGradient::linear(start, end, colors, model, policy).unwrap();
                let reference =
                    LinearGradient::new(start, end, colors, RgbSpace::DisplayP3, model).unwrap();
                let mut parameters = gradient.parameters;
                parameters.shape[0] = 2.;
                let mut d = draw(&painter, parameters);
                d.opacity = 0.7;
                d.content_mask.bounds.size.width = ScaledPixels(14.);
                let mut scene = Scene::default();
                scene.insert_primitive(Quad {
                    bounds: d.bounds,
                    content_mask: ContentMask { bounds: d.bounds },
                    background: gpui::black().into(),
                    ..Default::default()
                });
                scene.insert_primitive(Primitive::Shader(d));
                scene.finish();
                let image = renderer
                    .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(8)))
                    .unwrap();
                for (x, y, pixel) in image.enumerate_pixels() {
                    let qx = (x as f64 + 0.5 - 8.).abs() - 8. + 2.;
                    let qy = (y as f64 + 0.5 - 4.).abs() - 4. + 2.;
                    let distance = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - 2.;
                    let coverage = if x >= 14 {
                        0.
                    } else {
                        (0.5 - distance).clamp(0., 1.)
                    };
                    let color = reference
                        .sample([(x as f64 + 0.5) / 16., (y as f64 + 0.5) / 8.])
                        .unwrap()
                        .map_gamut(RgbSpace::DisplayP3, policy)
                        .unwrap();
                    let alpha = color.alpha() * 0.7 * coverage;
                    let rgb = color
                        .rgb()
                        .convert(RgbSpace::Srgb)
                        .unwrap()
                        .components()
                        .map(|v| (v * alpha).clamp(0., 1.));
                    let expected = LinearRgb::new(RgbSpace::Srgb, rgb)
                        .unwrap()
                        .encode()
                        .unwrap()
                        .components()
                        .map(|v| (v * 255.).round() as u8);
                    for i in 0..3 {
                        assert!(
                            pixel[i].abs_diff(expected[i]) <= 3,
                            "{model:?} {policy:?} end={end:?} ({x},{y}): {pixel:?} != {expected:?}"
                        );
                    }
                    assert_eq!(pixel[3], 255);
                }
            }
        }
    }
}

/// Measurements include encoding, GPU completion and readback; not GPU timestamps.
/// Run alone with --ignored --nocapture --test-threads=1 so allocation counters are scoped.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
#[ignore = "manual native performance comparison"]
fn compare_producer_allocations_and_frame_readback() {
    use gpui::{CustomDraw, DevicePixels, PlatformHeadlessRenderer, WindowColorMode};
    use std::{
        sync::{Arc, atomic::Ordering},
        time::Instant,
    };
    #[cfg(target_os = "macos")]
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    #[cfg(target_os = "windows")]
    let mut renderer = gpui_windows::Dx11HeadlessRenderer::new().unwrap();
    #[cfg(target_os = "macos")]
    println!("GPU: {}", metal::Device::system_default().unwrap().name());
    renderer
        .set_color_mode(WindowColorMode::WideGamutSdr)
        .unwrap();
    let painter = WidePainter::new().unwrap();
    let color = [1., 0., 0., 0.5];
    let parameters = Parameters {
        first: color,
        second: color,
        axis: [0.; 4],
        options: [0.; 4],
        shape: [0.; 4],
    };
    let extent = size(DevicePixels(512), DevicePixels(512));
    for count in [1, 16, 1000] {
        for instanced in [false, true] {
            // Large legacy scenes can exhaust pending command-buffer capacity
            // before the host submits prepared producers. Keep that baseline bounded.
            if !instanced && count > 16 {
                continue;
            }
            let mut scene = Scene::default();
            for i in 0..count {
                let mut d = draw(&painter, parameters);
                d.bounds = Bounds::new(
                    point(
                        ScaledPixels((i % 64) as f32 * 8.),
                        ScaledPixels((i / 64) as f32 * 8.),
                    ),
                    size(ScaledPixels(8.), ScaledPixels(8.)),
                );
                d.content_mask.bounds = Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(0.)),
                    size(ScaledPixels(512.), ScaledPixels(512.)),
                );
                if instanced {
                    scene.insert_primitive(Primitive::Shader(d));
                } else {
                    scene.insert_primitive(CustomDraw {
                        order: 0,
                        bounds: d.bounds,
                        content_mask: d.content_mask,
                        corner_radii: Default::default(),
                        opacity: 1.,
                        content: Arc::new(WideColor::linear_p3(color).unwrap()),
                        coverage: None,
                    });
                }
            }
            scene.finish();
            renderer.render_scene_to_image(&scene, extent).unwrap(); // warm pipelines/targets
            crate::PRODUCER_ALLOCATIONS.store(0, Ordering::Relaxed);
            let start = Instant::now();
            for _ in 0..3 {
                renderer.render_scene_to_image(&scene, extent).unwrap();
            }
            let elapsed = start.elapsed().as_secs_f64() * 1000. / 3.;
            let allocations = crate::PRODUCER_ALLOCATIONS.load(Ordering::Relaxed);
            assert_eq!(allocations, if instanced { 0 } else { count * 3 });
            println!(
                "count={count} instanced={instanced} producer_textures/frame={} producer_passes/frame={} fill_draws/frame={} frame_encode_complete_readback_ms={elapsed:.3}",
                allocations / 3,
                if instanced { 0 } else { count },
                if instanced {
                    scene.batches().count()
                } else {
                    count
                }
            );
        }
    }
}
