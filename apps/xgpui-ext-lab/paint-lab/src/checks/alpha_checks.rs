//! Compare the unchanged compositor against CPU premultiplied source-over.
use super::*;

const WIDTH: i32 = 96;
const HEIGHT: i32 = 64;
const SOURCE_WIDTH: i32 = 66;
const SOURCE_HEIGHT: i32 = 34;

#[derive(Debug)]
struct NativeSource(Background);
impl CustomContent for NativeSource {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        Ok(Box::new(self.0))
    }
}
fn scene_background(width: i32, height: i32, background: u32) -> Scene {
    let mut scene = Scene::default();
    let bounds = rect(0., 0., width as f32, height as f32);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: rgb(background).into(),
        ..Default::default()
    });
    scene
}
fn native_source_pixels(
    renderer: &mut dyn PlatformHeadlessRenderer,
    source: Background,
    background: u32,
) -> Result<Vec<u8>> {
    let mut scene = scene_background(SOURCE_WIDTH, SOURCE_HEIGHT, background);
    let bounds = rect(0., 0., SOURCE_WIDTH as f32, SOURCE_HEIGHT as f32);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: source,
        ..Default::default()
    });
    scene.finish();
    Ok(renderer
        .render_scene_to_image(
            &scene,
            size(DevicePixels(SOURCE_WIDTH), DevicePixels(SOURCE_HEIGHT)),
        )?
        .into_raw())
}
fn pixel(bytes: &[u8], x: usize, y: usize) -> [f64; 3] {
    let index = (y * SOURCE_WIDTH as usize + x) * 4;
    [
        bytes[index] as f64 / 255.,
        bytes[index + 1] as f64 / 255.,
        bytes[index + 2] as f64 / 255.,
    ]
}
fn sample(source: &[[f64; 4]], u: f64, v: f64) -> [f64; 4] {
    // Normalized, clamp-to-edge, bilinear sampler; interpolate premultiplied texels.
    let x = u * SOURCE_WIDTH as f64 - 0.5;
    let y = v * SOURCE_HEIGHT as f64 - 0.5;
    let tx = x - x.floor();
    let ty = y - y.floor();
    let mut result = [0.; 4];
    for (dy, wy) in [(0., 1. - ty), (1., ty)] {
        for (dx, wx) in [(0., 1. - tx), (1., tx)] {
            let sx = (x.floor() + dx).clamp(0., (SOURCE_WIDTH - 1) as f64) as usize;
            let sy = (y.floor() + dy).clamp(0., (SOURCE_HEIGHT - 1) as f64) as usize;
            let value = source[sy * SOURCE_WIDTH as usize + sx];
            for c in 0..4 {
                result[c] += value[c] * wx * wy;
            }
        }
    }
    result
}
fn coverage(local: [f64; 2]) -> f64 {
    let qx = (local[0] - 65.3 / 2.).abs() - 65.3 / 2. + 8.;
    let qy = (local[1] - 33.7 / 2.).abs() - 33.7 / 2. + 8.;
    let distance = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - 8.;
    (0.5 - distance).clamp(0., 1.)
}
#[derive(Default, Debug)]
struct Evidence {
    cases: usize,
    low_alpha_samples: usize,
    antialiased_samples: usize,
    filtered_edge_samples: usize,
    max_channel_error: u8,
}
fn compare(
    renderer: &mut dyn PlatformHeadlessRenderer,
    background: Background,
    source: &[[f64; 4]],
    backdrop: u32,
    opacity: f64,
    tolerance: u8,
    evidence: &mut Evidence,
) -> Result<()> {
    // Fractional destination extent/origin forces sampling between source texels.
    let bounds = rect(10.25, 10.35, 65.3, 33.7);
    let mut scene = scene_background(WIDTH, HEIGHT, backdrop);
    let mut draw = record(bounds, Arc::new(NativeSource(background)), None);
    draw.corner_radii = Corners::all(ScaledPixels(8.));
    draw.opacity = opacity as f32;
    scene.insert_primitive(draw);
    scene.finish();
    let actual = renderer
        .render_scene_to_image(&scene, size(DevicePixels(WIDTH), DevicePixels(HEIGHT)))?
        .into_raw();
    let bg = [
        (backdrop >> 16) & 255,
        (backdrop >> 8) & 255,
        backdrop & 255,
    ]
    .map(|c| c as f64 / 255.);
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let local = [x as f64 + 0.5 - 10.25, y as f64 + 0.5 - 10.35];
            let inside = local[0] >= 0. && local[0] < 65.3 && local[1] >= 0. && local[1] < 33.7;
            let mut expected = bg;
            if inside {
                let value = sample(source, local[0] / 65.3, local[1] / 33.7);
                let shape = coverage(local);
                let k = shape * opacity;
                // Independent reference: no unpremultiplication, epsilon or RGB clamp.
                for c in 0..3 {
                    expected[c] = value[c] * k + bg[c] * (1. - value[3] * k);
                }
                if shape > 0. {
                    evidence.low_alpha_samples += usize::from(value[3] > 0. && value[3] <= 0.001);
                    evidence.antialiased_samples += usize::from(shape < 1.);
                    // Source samples inside the 8-bit minimum are necessarily filtered.
                    evidence.filtered_edge_samples +=
                        usize::from(value[3] > 0. && value[3] < 1. / 255.);
                }
            }
            let offset = (y * WIDTH as usize + x) * 4;
            for c in 0..3 {
                let expected = (expected[c].clamp(0., 1.) * 255.).round() as u8;
                let error = actual[offset + c].abs_diff(expected);
                evidence.max_channel_error = evidence.max_channel_error.max(error);
                ensure!(
                    error <= tolerance,
                    "alpha edge at ({x},{y}), channel={c}, backdrop={backdrop:06x}, opacity={opacity}: actual={}, expected={expected}, error={error}",
                    actual[offset + c]
                );
            }
            ensure!(actual[offset + 3] == 255, "opaque backdrop lost alpha");
        }
    }
    evidence.cases += 1;
    Ok(())
}
fn saturated_edges(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<Evidence> {
    let mut evidence = Evidence::default();
    for (channel, rgb) in [(0, 0xff0000), (1, 0x00ff00), (2, 0x0000ff)] {
        for alpha in [0., 0.00001, 0.0005, 1. / 255., 2. / 255., 0.02, 0.2] {
            let mut color: Hsla = gpui::rgb(rgb).into();
            color.a = alpha;
            for source in [Background::from(color), pattern_slash(color, 3., 5.)] {
                let black = native_source_pixels(renderer, source, 0)?;
                // Pure primary: its full-intensity channel is exactly the quantized
                // source alpha. RGB over opaque black equals the adapter's RGB over
                // transparent black. This avoids guessing native rasterization math.
                let texels: Vec<_> = (0..SOURCE_HEIGHT as usize)
                    .flat_map(|y| {
                        let black = &black;
                        (0..SOURCE_WIDTH as usize).map(move |x| {
                            let rgb = pixel(black, x, y);
                            [rgb[0], rgb[1], rgb[2], rgb[channel]]
                        })
                    })
                    .collect();
                for backdrop in [0x101010, 0xf0f0f0] {
                    for opacity in [1., 0.35] {
                        compare(
                            renderer,
                            source,
                            &texels,
                            backdrop,
                            opacity,
                            1,
                            &mut evidence,
                        )?;
                    }
                }
            }
        }
    }
    ensure!(
        evidence.low_alpha_samples > 100,
        "did not exercise alpha <= 0.001"
    );
    ensure!(
        evidence.filtered_edge_samples > 100,
        "did not exercise filtered low alpha"
    );
    ensure!(
        evidence.antialiased_samples > 100,
        "did not exercise antialiased edges"
    );
    Ok(evidence)
}
fn dithered_gradients(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<Evidence> {
    let mut evidence = Evidence::default();
    for color_space in [ColorSpace::Srgb, ColorSpace::Oklab] {
        for alpha in [0., 0.00001, 0.0005, 1. / 255., 0.02] {
            let source = linear_gradient(
                90.,
                linear_color_stop(hsla(0., 1., 0.5, alpha), 0.),
                linear_color_stop(hsla(2. / 3., 1., 0.5, alpha), 1.),
            )
            .color_space(color_space);
            let black = native_source_pixels(renderer, source, 0)?;
            let white = native_source_pixels(renderer, source, 0xffffff)?;
            // Native gradient dithering is backend-specific. Infer source alpha from
            // white-minus-black source-over readbacks; median rejects one clipped
            // color channel. Two 8-bit readbacks leave up to ~1 LSB alpha uncertainty.
            let texels: Vec<_> = (0..SOURCE_HEIGHT as usize)
                .flat_map(|y| {
                    let (black, white) = (&black, &white);
                    (0..SOURCE_WIDTH as usize).map(move |x| {
                        let rgb = pixel(black, x, y);
                        let light = pixel(white, x, y);
                        let mut estimates =
                            [0, 1, 2].map(|c| (1. - light[c] + rgb[c]).clamp(0., 1.));
                        estimates.sort_by(f64::total_cmp);
                        [rgb[0], rgb[1], rgb[2], estimates[1]]
                    })
                })
                .collect();
            for backdrop in [0x101010, 0xf0f0f0] {
                compare(renderer, source, &texels, backdrop, 1., 2, &mut evidence)?;
            }
        }
    }
    Ok(evidence)
}

#[::core::prelude::v1::test]
fn native_low_alpha_saturated_edges() -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?.expect("native renderer");
    eprintln!(
        "native saturated edges: {:?}",
        saturated_edges(renderer.as_mut())?
    );
    eprintln!(
        "native dithered gradients: {:?}",
        dithered_gradients(renderer.as_mut())?
    );
    Ok(())
}
#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_low_alpha_saturated_edges() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    eprintln!(
        "wgpu saturated edges: {:?}",
        saturated_edges(&mut renderer)?
    );
    eprintln!(
        "wgpu dithered gradients: {:?}",
        dithered_gradients(&mut renderer)?
    );
    Ok(())
}
