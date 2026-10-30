use super::*;
use crate::instanced::{InstanceField, ShaderInstance, ShaderProgram};
use gpui::{BackdropDraw, LayerBlurQuality, Primitive, ShaderDraw, WindowColorMode};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Tint {
    color: [f32; 4],
}
impl ShaderInstance for Tint {
    const FIELDS: &'static [InstanceField] = &[InstanceField {
        name: "color",
        offset: 0,
        components: 4,
    }];
}
fn wide_program() -> ShaderProgram<Tint> {
    ShaderProgram::new_linear_p3(
        "struct Parameters { color: vec4<f32>, }
         fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> { return p.color; }",
    )
    .unwrap()
}
fn wide_rect(
    scene: &mut Scene,
    program: &ShaderProgram<Tint>,
    bounds: Bounds<ScaledPixels>,
    color: [f32; 4],
) {
    scene.insert_primitive(Primitive::Shader(ShaderDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        opacity: 1.,
        scale_factor: 1.,
        program: program.scene_program(),
        path_mask: None,
        glyph_mask: None,
        data: program.pack(&Tint { color }).unwrap(),
    }));
}
fn preview(p3: [f64; 4]) -> [u8; 4] {
    let [r, g, b, a] = p3;
    let rgb = [
        1.224940176 * r - 0.224940176 * g,
        -0.042056955 * r + 1.042056955 * g,
        -0.019637555 * r - 0.078636046 * g + 1.098273600 * b,
    ];
    let encoded = rgb.map(|v| {
        let v = v.clamp(0., 1.);
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        }
    });
    [encoded[0], encoded[1], encoded[2], a].map(|v| (v * 255.).round() as u8)
}
fn renderer() -> gpui_apple::metal_renderer::MetalHeadlessRenderer {
    let mut r = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    assert_eq!(
        r.set_color_mode(WindowColorMode::WideGamutSdr)
            .unwrap()
            .effective(),
        WindowColorMode::WideGamutSdr
    );
    r
}

// Independent full-resolution Gaussian in f64, without the SDR reference's
// intermediate 8-bit quantization. FP16 error is checked after preview encoding.
fn linear_gaussian(
    mut pixels: Vec<[f64; 4]>,
    width: usize,
    height: usize,
    radius: f64,
) -> Vec<[f64; 4]> {
    let support = radius.ceil() as isize;
    let sigma = radius / 3.;
    let weights: Vec<_> = (-support..=support)
        .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
        .collect();
    let total: f64 = weights.iter().sum();
    for (dx, dy) in [(1, 0), (0, 1)] {
        let mut output = vec![[0.; 4]; pixels.len()];
        for y in 0..height {
            for x in 0..width {
                for (i, weight) in (-support..=support).zip(&weights) {
                    let sx = x as isize + i * dx;
                    let sy = y as isize + i * dy;
                    if sx >= 0 && sy >= 0 && sx < width as isize && sy < height as isize {
                        for c in 0..4 {
                            output[y * width + x][c] +=
                                pixels[sy as usize * width + sx as usize][c] * weight / total;
                        }
                    }
                }
            }
        }
        pixels = output;
    }
    pixels
}

#[test]
fn wide_capture_blur_matches_linear_p3_reference_at_fractional_dpi() {
    let program = wide_program();
    let mut r = renderer();
    for scale in [1., 1.5, 2.] {
        let mask = ContentMask {
            bounds: viewport().bounds.map(|v| ScaledPixels(v.0 * scale)),
        };
        let width = (80. * scale) as usize;
        let height = (48. * scale) as usize;
        for radius in [4., 8., 12.] {
            let mut c = capture(0.6);
            let fields = Arc::get_mut(&mut c).unwrap();
            fields.bounds = fields.bounds.map(|v| ScaledPixels(v.0 * scale));
            fields.blur_radius = ScaledPixels(radius * scale);
            fields.blur_quality = LayerBlurQuality::FullResolution;
            let mut s = Scene::default();
            s.insert_primitive(Quad {
                bounds: mask.bounds,
                content_mask: mask,
                background: gpui::black().into(),
                ..Default::default()
            });
            s.begin_layer_capture(c.clone(), mask);
            let shape = Bounds::new(point(px(24.), px(20.)), size(px(32.), px(20.))).scale(scale);
            wide_rect(&mut s, &program, shape, [1., 0., 0., 0.5]);
            s.end_layer_capture(c, mask);
            s.finish();
            let mut raw = vec![[0.; 4]; width * height];
            for y in 0..height {
                for x in 0..width {
                    let wx = (x as f32 + 0.5) / scale + 8.;
                    let wy = (y as f32 + 0.5) / scale + 8.;
                    if (24. ..56.).contains(&wx) && (20. ..40.).contains(&wy) {
                        raw[y * width + x] = [0.5, 0., 0., 0.5];
                    }
                }
            }
            let reference = linear_gaussian(raw, width, height, (radius * scale) as f64);
            let image = r
                .render_scene_to_image(
                    &s,
                    size(
                        DevicePixels((96. * scale) as i32),
                        DevicePixels((64. * scale) as i32),
                    ),
                )
                .unwrap();
            for y in (0..height).step_by(3) {
                for x in (0..width).step_by(3) {
                    let src = reference[y * width + x];
                    pixel(
                        &image,
                        x as u32 + (8. * scale) as u32,
                        y as u32 + (8. * scale) as u32,
                        preview([src[0] * 0.6, 0., 0., 1.]),
                    );
                }
            }
        }
    }
}

#[test]
fn wide_capture_nested_cache_clip_resize_and_mode_switch_preserve_color() {
    let mut r = renderer();
    let p = wide_program();
    let mut s = Scene::default();
    rect(&mut s, 0., 0., 96., 64., gpui::black());
    let outer = capture(0.5);
    let mut inner = capture(0.5);
    Arc::get_mut(&mut inner).unwrap().clip =
        Some(Bounds::new(point(px(16.), px(16.)), size(px(64.), px(32.))).scale(1.));
    s.begin_layer_capture(outer.clone(), viewport());
    wide_rect(&mut s, &p, outer.bounds, [1., 0., 0., 1.]);
    s.begin_layer_capture(inner.clone(), viewport());
    wide_rect(&mut s, &p, inner.bounds, [0., 1., 0., 1.]);
    s.end_layer_capture(inner, viewport());
    s.end_layer_capture(outer, viewport());
    // A following UI run must be drawn after restoration/composition.
    rect(&mut s, 60., 24., 8., 8., gpui::white());
    s.finish();
    for extent in [
        size(DevicePixels(96), DevicePixels(64)),
        size(DevicePixels(112), DevicePixels(80)),
    ] {
        for _ in 0..2 {
            let image = r.render_scene_to_image(&s, extent).unwrap();
            pixel(&image, 32, 24, preview([0.25, 0.25, 0., 1.]));
            pixel(&image, 12, 12, preview([0.5, 0., 0., 1.]));
            pixel(&image, 64, 28, [255; 4]);
        }
    }
    assert!(r.layer_cache_stats().hits > 0);
    assert_eq!(r.layer_texture_bytes(), 2 * 80 * 48 * 2 * 8);
    // Switching domains must rebuild textures and invalidate retained captures.
    let ordinary = scene(true, false);
    r.set_color_mode(WindowColorMode::Default).unwrap();
    let legacy = r
        .render_scene_to_image(&ordinary, size(DevicePixels(96), DevicePixels(64)))
        .unwrap();
    pixel(&legacy, 48, 24, [191, 128, 191, 255]);
    assert_eq!(r.layer_texture_bytes(), 2 * 80 * 48 * 2 * 4);
    r.set_color_mode(WindowColorMode::WideGamutSdr).unwrap();
    let image = r
        .render_scene_to_image(&s, size(DevicePixels(96), DevicePixels(64)))
        .unwrap();
    pixel(&image, 32, 24, preview([0.25, 0.25, 0., 1.]));
    r.render_scene_to_image(&Scene::default(), size(DevicePixels(96), DevicePixels(64)))
        .unwrap();
    assert_eq!(r.effect_texture_count(), 0);
    assert_eq!(r.layer_texture_bytes(), 0);
}

#[test]
fn wide_capture_grayscale_uses_linear_p3_luminance() {
    let mut r = renderer();
    let p = wide_program();
    let mut s = Scene::default();
    rect(&mut s, 0., 0., 96., 64., gpui::black());
    let mut c = capture(1.);
    Arc::get_mut(&mut c).unwrap().saturation = 0.;
    s.begin_layer_capture(c.clone(), viewport());
    wide_rect(&mut s, &p, c.bounds, [1., 0., 0., 1.]);
    s.end_layer_capture(c, viewport());
    s.finish();
    let image = r
        .render_scene_to_image(&s, size(DevicePixels(96), DevicePixels(64)))
        .unwrap();
    pixel(
        &image,
        32,
        24,
        preview([0.2289745641, 0.2289745641, 0.2289745641, 1.]),
    );
}

#[test]
fn wide_backdrop_filters_composed_p3_and_keeps_following_path_mask() {
    let mut r = renderer();
    let p = wide_program();
    let mut s = Scene::default();
    wide_rect(&mut s, &p, viewport().bounds, [1., 0., 0., 1.]);
    s.insert_primitive(Primitive::Backdrop(BackdropDraw {
        order: 0,
        bounds: viewport().bounds,
        content_mask: viewport(),
        surface_bounds: Bounds::new(point(px(16.), px(12.)), size(px(64.), px(40.))).scale(1.),
        clip: viewport(),
        corner_radii: Corners::all(ScaledPixels(0.)),
        radius: 8.,
        opacity: 1.,
        program: backdrop::shared_program().unwrap(),
    }));
    // Curved path forces the existing coverage-mask fallback.
    let mut path = gpui::PathBuilder::fill();
    path.move_to(point(px(24.), px(20.)));
    path.curve_to(point(px(56.), px(20.)), point(px(40.), px(4.)));
    path.line_to(point(px(56.), px(40.)));
    path.line_to(point(px(24.), px(40.)));
    path.close();
    let brush = Brush::solid([0., 0., 1., 1.]).unwrap();
    s.insert_primitive(Primitive::Shader(
        brush
            .path_draw(
                path.build().unwrap(),
                ContentMask {
                    bounds: viewport().bounds.map(|v| px(v.0)),
                },
                1.,
                1.,
            )
            .unwrap(),
    ));
    s.finish();
    for _ in 0..2 {
        let image = r
            .render_scene_to_image(&s, size(DevicePixels(96), DevicePixels(64)))
            .unwrap();
        pixel(&image, 18, 32, [255, 0, 0, 255]);
        pixel(&image, 40, 32, [0, 0, 255, 255]);
    }
    assert!(r.backdrop_cache_stats().hits > 0);
}
