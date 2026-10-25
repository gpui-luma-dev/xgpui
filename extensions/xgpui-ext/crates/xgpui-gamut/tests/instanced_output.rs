//! Cross-extension parity belongs to the gamut integration, not shader internals.
#![cfg(any(target_os = "macos", target_os = "windows"))]
use gpui::{Bounds, ContentMask, Primitive, Quad, ScaledPixels, Scene, ShaderDraw, point, size};
use std::sync::Arc;
use xgpui_shader::instanced::{
    ShaderProgram,
    presets::{EffectInstance, ROUNDED_BOX},
};

fn program() -> ShaderProgram<EffectInstance> {
    ShaderProgram::new(ROUNDED_BOX).unwrap()
}
fn draw(program: &ShaderProgram<EffectInstance>, x: f32, color: [f32; 4]) -> ShaderDraw {
    let mut data = [[0.; 4]; gpui::SHADER_DATA_SLOTS];
    // EffectInstance ABI: color at byte 0, effect at byte 16, remaining slots unused.
    data[0] = color;
    data[1] = [0., 0., 1., 0.];
    ShaderDraw {
        order: 0,
        bounds: Bounds::new(
            point(ScaledPixels(x), ScaledPixels(4.)),
            size(ScaledPixels(20.), ScaledPixels(20.)),
        ),
        content_mask: ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(0.), ScaledPixels(0.)),
                size(ScaledPixels(64.), ScaledPixels(64.)),
            ),
        },
        opacity: 1.,
        scale_factor: 1.,
        program: program.scene_program(),
        data,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn wide_instances_match_typed_producers_and_reject_default_mode() {
    use gpui::{CustomDraw, DevicePixels, PlatformHeadlessRenderer, WindowColorMode};
    let wide = ShaderProgram::<EffectInstance>::new_linear_p3("struct Parameters { color: vec4<f32>, effect: vec4<f32>, } fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> { return p.color; }").unwrap();
    #[cfg(target_os = "macos")]
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    #[cfg(target_os = "windows")]
    let mut renderer = gpui_windows::Dx11HeadlessRenderer::new().unwrap();
    let make_scene = |instanced: bool, alpha: f32, opacity: f32| {
        let mut scene = Scene::default();
        // Non-black legacy backdrop, ordinary shader, wide content, then native UI.
        scene.insert_primitive(Quad {
            bounds: draw(&wide, 0., [1.; 4]).content_mask.bounds,
            content_mask: draw(&wide, 0., [1.; 4]).content_mask,
            background: gpui::rgb(0x336699).into(),
            ..Default::default()
        });
        let ordinary = program();
        scene.insert_primitive(Primitive::Shader(draw(&ordinary, 2., [0.2, 0.4, 0.6, 0.5])));
        for (x, color) in [
            (4., [1., 0., 0., alpha]),
            (12., [0., 1., 0., alpha]),
            (32., [-0.2, 1.2, 0., alpha]),
        ] {
            let mut d = draw(&wide, x, color);
            d.opacity = opacity;
            d.content_mask.bounds.size.width = ScaledPixels(45.);
            if instanced {
                scene.insert_primitive(Primitive::Shader(d));
            } else {
                scene.insert_primitive(CustomDraw {
                    order: 0,
                    bounds: d.bounds,
                    content_mask: d.content_mask,
                    corner_radii: Default::default(),
                    opacity,
                    content: Arc::new(
                        xgpui_gamut::WideColor::linear_p3(color.map(|v| v.clamp(0., 1.))).unwrap(),
                    ),
                    coverage: None,
                });
            }
        }
        scene.insert_primitive(Quad {
            bounds: Bounds::new(
                point(ScaledPixels(8.), ScaledPixels(8.)),
                size(ScaledPixels(4.), ScaledPixels(4.)),
            ),
            content_mask: draw(&wide, 0., [1.; 4]).content_mask,
            background: gpui::rgba(0xffffff80).into(),
            ..Default::default()
        });
        scene.finish();
        scene
    };
    let extent = size(DevicePixels(64), DevicePixels(32));
    let scene = make_scene(true, 0.5, 0.35);
    assert!(renderer.render_scene_to_image(&scene, extent).is_err());
    renderer
        .set_color_mode(WindowColorMode::WideGamutSdr)
        .unwrap();
    for (alpha, opacity) in [(0., 1.), (0.5, 0.35), (1., 1.)] {
        for extent in [extent, size(DevicePixels(80), DevicePixels(40))] {
            let scene = make_scene(true, alpha, opacity);
            let mut replay = Scene::default();
            replay.replay(0..scene.len(), &scene);
            replay.finish();
            let actual = renderer.render_scene_to_image(&replay, extent).unwrap();
            let expected = renderer
                .render_scene_to_image(&make_scene(false, alpha, opacity), extent)
                .unwrap();
            for (a, b) in actual.pixels().zip(expected.pixels()) {
                for i in 0..4 {
                    assert!(
                        a[i].abs_diff(b[i]) <= 2,
                        "alpha={alpha} opacity={opacity}: {a:?} != {b:?}"
                    );
                }
            }
        }
    }
    renderer.render_scene_to_image(&scene, extent).unwrap();
    renderer.set_color_mode(WindowColorMode::Default).unwrap();
    assert!(renderer.render_scene_to_image(&scene, extent).is_err());
    renderer
        .set_color_mode(WindowColorMode::WideGamutSdr)
        .unwrap();
    renderer.render_scene_to_image(&scene, extent).unwrap();
}
