use super::*;
use crate::{BlendMode, Noise, NoiseKind};

#[test]
fn noise_validation_variants_and_zero_strength() {
    let base = Brush::solid([0.2, 0.4, 0.7, 0.3]).unwrap();
    for bad in [
        Noise::simplex(f32::NAN, 1, 1.),
        Noise::simplex(0., 1, 1.),
        Noise::simplex(0.1, 0, 1.),
        Noise::simplex(0.1, 5, 1.),
        Noise::simplex(0.1, 2, -0.1),
        Noise::dither(f32::INFINITY),
        Noise::worley_f2(f32::NAN, 0.5),
        Noise::worley_f2(0.04, 1.1),
    ] {
        assert!(base.clone().with_noise(bad, BlendMode::Overlay).is_err());
    }
    assert!(
        base.clone()
            .with_noise(
                Noise {
                    frequency: 0.5,
                    ..Noise::dither(0.1)
                },
                BlendMode::Normal
            )
            .is_err()
    );
    let a = base
        .clone()
        .with_noise(Noise::simplex(0.04, 2, 0.2), BlendMode::Overlay)
        .unwrap();
    let b = base
        .clone()
        .with_noise(Noise::simplex(0.08, 2, 0.6), BlendMode::Overlay)
        .unwrap();
    assert_eq!(
        a.program.id(),
        b.program.id(),
        "numeric edits must reuse the same program"
    );
    let clean = a
        .clone()
        .with_noise(Noise::dither(0.), BlendMode::Multiply)
        .unwrap();
    assert_eq!(clean.program.id(), base.program.id());
    assert_eq!(clean.parameters.first, base.parameters.first);
    // Compile every bounded variant through WGSL validation, Metal and HLSL
    // translation, not just the combinations used by the gallery.
    for kind in [
        NoiseKind::Simplex,
        NoiseKind::Worley,
        NoiseKind::WorleyF2,
        NoiseKind::Dither,
    ] {
        for octaves in 1..=4 {
            if kind == NoiseKind::Dither && octaves != 1 {
                continue;
            }
            for mode in [
                BlendMode::Normal,
                BlendMode::Multiply,
                BlendMode::Screen,
                BlendMode::Overlay,
                BlendMode::ColorDodge,
                BlendMode::SoftLight,
                BlendMode::LinearBurn,
            ] {
                let noise = Noise {
                    kind,
                    octaves,
                    frequency: if kind == NoiseKind::Dither { 1. } else { 0.04 },
                    strength: 0.7,
                };
                base.clone().with_noise(noise, mode).unwrap();
            }
        }
    }
    assert_eq!(std::mem::size_of::<BrushInstance>(), 128);
}

// Independent double-precision signal reference. Hash arithmetic intentionally
// wraps in u32, with exact integer conversion before continuous interpolation.
fn hash(x: i32, y: i32) -> u32 {
    let h = (x as u32).wrapping_mul(0x9e3779b9) ^ (y as u32).wrapping_mul(0x85ebca6b);
    let h = (h ^ (h >> 16)).wrapping_mul(0x7feb352d);
    let h = (h ^ (h >> 15)).wrapping_mul(0x846ca68b);
    h ^ (h >> 16)
}
fn unit(x: i32, y: i32) -> f64 {
    (hash(x, y) & 0xffffff) as f64 / 16777216.
}
pub(super) fn signal(kind: NoiseKind, position: [f64; 2]) -> f64 {
    let [x, y] = position;
    if kind == NoiseKind::Dither {
        let x = (x.floor() as i64).rem_euclid(16) as usize;
        let y = (y.floor() as i64).rem_euclid(16) as usize;
        return (super::super::noise::BLUE_NOISE[y * 16 + x] as f64 + 0.5) / 256.;
    }
    if matches!(kind, NoiseKind::Worley | NoiseKind::WorleyF2) {
        let cx = x.floor() as i32;
        let cy = y.floor() as i32;
        // Sort a larger 9x9 search, independently of the GPU's two-minimum
        // update and 5x5 neighborhood bound.
        let mut distances = Vec::new();
        for dy in -4..=4 {
            for dx in -4..=4 {
                let vx = dx as f64 + unit(cx + dx, cy + dy) - (x - x.floor());
                let vy = dy as f64 + unit(cx + dx + 113, cy + dy + 271) - (y - y.floor());
                distances.push(vx * vx + vy * vy);
            }
        }
        distances.sort_by(f64::total_cmp);
        let index = if kind == NoiseKind::WorleyF2 { 1 } else { 0 };
        return (distances[index].sqrt() / 2_f64.sqrt()).clamp(0., 1.);
    }
    let skew = (x + y) * (3_f64.sqrt() - 1.) / 2.;
    let cx = (x + skew).floor() as i32;
    let cy = (y + skew).floor() as i32;
    let unskew = (cx + cy) as f64 * (3. - 3_f64.sqrt()) / 6.;
    let first = [x - cx as f64 + unskew, y - cy as f64 + unskew];
    let step = if first[0] > first[1] { [1, 0] } else { [0, 1] };
    let g = (3. - 3_f64.sqrt()) / 6.;
    let offsets = [
        first,
        [first[0] - step[0] as f64 + g, first[1] - step[1] as f64 + g],
        [first[0] - 1. + 2. * g, first[1] - 1. + 2. * g],
    ];
    let cells = [[cx, cy], [cx + step[0], cy + step[1]], [cx + 1, cy + 1]];
    let d = 2_f64.sqrt().recip();
    let gradients = [
        [1., 0.],
        [-1., 0.],
        [0., 1.],
        [0., -1.],
        [d, d],
        [-d, d],
        [d, -d],
        [-d, -d],
    ];
    let value = offsets
        .iter()
        .zip(cells)
        .map(|(offset, cell)| {
            let weight = (0.5 - offset[0] * offset[0] - offset[1] * offset[1]).max(0.);
            let grad = gradients[(hash(cell[0], cell[1]) & 7) as usize];
            weight.powi(4) * (grad[0] * offset[0] + grad[1] * offset[1])
        })
        .sum::<f64>()
        * 99.2;
    (0.5 + 0.5 * value).clamp(0., 1.)
}
pub(super) fn blended(base: f64, noise: f64, mode: BlendMode) -> f64 {
    match mode {
        BlendMode::Normal => noise,
        BlendMode::Multiply => base * noise,
        BlendMode::Screen => 1. - (1. - base) * (1. - noise),
        BlendMode::Overlay => {
            if base <= 0.5 {
                2. * base * noise
            } else {
                1. - 2. * (1. - base) * (1. - noise)
            }
        }
        BlendMode::ColorDodge => {
            if base == 0. {
                0.
            } else if noise == 1. {
                1.
            } else {
                (base / (1. - noise)).min(1.)
            }
        }
        BlendMode::SoftLight => {
            if noise <= 0.5 {
                base - (1. - 2. * noise) * base * (1. - base)
            } else {
                let d = if base <= 0.25 {
                    ((16. * base - 12.) * base + 4.) * base
                } else {
                    base.sqrt()
                };
                base + (2. * noise - 1.) * (d - base)
            }
        }
        BlendMode::LinearBurn => (base + noise - 1.).max(0.),
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn noise_pixels_match_cpu_across_blends_dpi_clipping_and_backends() {
    use gpui::{DevicePixels, PlatformHeadlessRenderer, Quad};
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
    let color = [0., 0.2, 0.8, 0.4];
    for renderer in &mut renderers {
        for kind in [
            NoiseKind::Simplex,
            NoiseKind::Worley,
            NoiseKind::WorleyF2,
            NoiseKind::Dither,
        ] {
            for mode in [
                BlendMode::Normal,
                BlendMode::Multiply,
                BlendMode::Screen,
                BlendMode::Overlay,
                BlendMode::ColorDodge,
                BlendMode::SoftLight,
                BlendMode::LinearBurn,
            ] {
                let noise = Noise {
                    kind,
                    frequency: if kind == NoiseKind::Dither { 1. } else { 0.04 },
                    octaves: if kind == NoiseKind::Dither { 1 } else { 3 },
                    strength: 0.7,
                };
                let brush = Brush::solid(color)
                    .unwrap()
                    .with_noise(noise, mode)
                    .unwrap();
                for border in [false, true] {
                    for scale in [1., 1.5] {
                        let mut scene = Scene::default();
                        let mut record = draw(&brush, &quad(), border, scale);
                        record.opacity = 0.5;
                        record.content_mask.bounds.size.width = gpui::ScaledPixels(65. * scale);
                        scene.insert_primitive(Quad {
                            bounds: record.content_mask.bounds,
                            content_mask: record.content_mask,
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
                            let value = if kind == NoiseKind::Dither {
                                signal(kind, [local[0] * scale as f64, local[1] * scale as f64])
                            } else {
                                let mut total = 0.;
                                let mut normalization = 0.;
                                for octave in 0..noise.octaves {
                                    let weight = 0.5_f64.powi(octave as i32);
                                    let frequency =
                                        noise.frequency as f64 * 2_f64.powi(octave as i32);
                                    total +=
                                        signal(kind, [local[0] * frequency, local[1] * frequency])
                                            * weight;
                                    normalization += weight;
                                }
                                total / normalization
                            };
                            let pixel = image.get_pixel(ix, iy).0;
                            for i in 0..3 {
                                let base = color[i] as f64;
                                let expected = ((base
                                    + (blended(base, value, mode) - base) * noise.strength as f64)
                                    * color[3] as f64
                                    * 0.5
                                    * 255.)
                                    .round() as u8;
                                assert!(
                                    pixel[i].abs_diff(expected) <= 2,
                                    "{kind:?} {mode:?} scale={scale} {pixel:?}, channel={i} expected={expected}"
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

#[test]
fn blue_noise_has_uniform_ranks_and_suppresses_low_spatial_frequencies() {
    let mut ranks = super::super::noise::BLUE_NOISE.to_vec();
    ranks.sort_unstable();
    assert_eq!(ranks, (0..=255).collect::<Vec<u8>>());
    // Direct DFT: compare low-frequency power to the expected white-noise
    // power N*variance, independently of the generator's Gaussian objective.
    // Check the continuous signal and several thresholded coverage masks.
    for threshold in [
        None,
        Some(0.125),
        Some(0.25),
        Some(0.5),
        Some(0.75),
        Some(0.875),
    ] {
        let values: Vec<f64> = (0..256)
            .map(|i| {
                let value = signal(NoiseKind::Dither, [(i % 16) as f64, (i / 16) as f64]);
                threshold.map_or(value, |cut| if value < cut { 1. } else { 0. })
            })
            .collect();
        let mean = values.iter().sum::<f64>() / 256.;
        if threshold.is_none() {
            assert_eq!(mean, 0.5);
        }
        let white_power = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
        let mut power = 0.;
        let mut modes = 0.;
        for fy in -2_i32..=2 {
            for fx in -2_i32..=2 {
                if fx * fx + fy * fy == 0 || fx * fx + fy * fy > 4 {
                    continue;
                }
                let (mut real, mut imaginary) = (0., 0.);
                for (i, value) in values.iter().enumerate() {
                    let angle = std::f64::consts::TAU
                        * (fx as f64 * (i % 16) as f64 + fy as f64 * (i / 16) as f64)
                        / 16.;
                    real += (value - mean) * angle.cos();
                    imaginary += (value - mean) * angle.sin();
                }
                power += real * real + imaginary * imaginary;
                modes += 1.;
            }
        }
        assert!(
            power / modes < white_power * 0.15,
            "threshold={threshold:?}: relative low-frequency power {}",
            power / modes / white_power
        );
    }
    for y in -17..32 {
        for x in -17..32 {
            let value = signal(NoiseKind::Dither, [x as f64, y as f64]);
            assert_eq!(
                value,
                signal(NoiseKind::Dither, [(x + 16) as f64, (y + 16) as f64])
            );
            assert!((0. ..1.).contains(&value));
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn worley_f1_f2_dense_pixels_match_sorted_larger_neighborhood() {
    use gpui::{DevicePixels, PlatformHeadlessRenderer};
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
        let mut images = Vec::new();
        for kind in [NoiseKind::Worley, NoiseKind::WorleyF2] {
            let noise = Noise {
                kind,
                frequency: 0.73,
                octaves: 1,
                strength: 1.,
            };
            let brush = Brush::solid([0.5, 0.5, 0.5, 1.])
                .unwrap()
                .with_noise(noise, BlendMode::Normal)
                .unwrap();
            let mut scene = Scene::default();
            scene.insert_primitive(Primitive::Shader(draw(&brush, &quad(), false, 1.)));
            scene.finish();
            let image = renderer
                .render_scene_to_image(&scene, size(DevicePixels(120), DevicePixels(80)))
                .unwrap();
            for y in 16..48 {
                for x in 16..72 {
                    let expected = (signal(
                        kind,
                        [
                            (x as f64 + 0.5 - 8.) * noise.frequency as f64,
                            (y as f64 + 0.5 - 8.) * noise.frequency as f64,
                        ],
                    ) * 255.)
                        .round() as u8;
                    assert!(
                        image.get_pixel(x, y).0[..3]
                            .iter()
                            .all(|v| v.abs_diff(expected) <= 1),
                        "{kind:?} at {x},{y}: {:?} != {expected}",
                        image.get_pixel(x, y)
                    );
                }
            }
            images.push(image);
        }
        let mut different = 0;
        for y in 16..48 {
            for x in 16..72 {
                let f1 = images[0].get_pixel(x, y).0[0];
                let f2 = images[1].get_pixel(x, y).0[0];
                assert!(f2 >= f1, "F2 cannot be closer than F1");
                if f2.saturating_sub(f1) > 8 {
                    different += 1;
                }
            }
        }
        assert!(different > 1000, "F2 must visibly differ from F1");
    }
}
