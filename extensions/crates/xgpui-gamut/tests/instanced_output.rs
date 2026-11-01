//! Cross-extension parity belongs to the gamut integration, not shader internals.
#![cfg(target_os = "macos")]
use gpui::{Bounds, ContentMask, Primitive, Quad, ScaledPixels, Scene, ShaderDraw, point, size};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
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
        path_mask: None,
        glyph_mask: None,
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

#[test]
fn wide_instances_match_portable_content_and_reject_default_mode() {
    use gpui::{CustomDraw, DevicePixels, PlatformHeadlessRenderer, WindowColorMode};
    let wide = ShaderProgram::<EffectInstance>::new_linear_p3("struct Parameters { color: vec4<f32>, effect: vec4<f32>, } fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> { return p.color; }").unwrap();
    #[cfg(target_os = "macos")]
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    let (done, ready) = mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = done.send(());
    }));
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
                    content: gpui::content::ContentDraw::new(
                        xgpui_gamut::WideColor::linear_p3(color.map(|v| v.clamp(0., 1.)))
                            .unwrap()
                            .content(),
                    )
                    .into_scene_content(),
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
            let portable = make_scene(false, alpha, opacity);
            renderer.render_scene(&portable, extent).unwrap();
            if renderer.content_presentation_stats().pending {
                ready.recv_timeout(Duration::from_secs(20)).unwrap();
            }
            let expected = renderer.render_scene_to_image(&portable, extent).unwrap();
            assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
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
