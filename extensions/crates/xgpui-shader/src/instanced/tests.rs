use super::*;
use gpui::{
    ContentMask, Primitive, PrimitiveBatch, Quad, ScaledPixels, Scene, ShaderDraw, point, size,
};
use presets::*;

fn program() -> ShaderProgram<EffectInstance> {
    ShaderProgram::new(ROUNDED_BOX).unwrap()
}
fn draw(program: &ShaderProgram<EffectInstance>, x: f32, color: [f32; 4]) -> ShaderDraw {
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
        program: program.program.clone(),
        data: program
            .pack(&EffectInstance {
                color,
                effect: [0., 0., 1., 0.],
            })
            .unwrap(),
    }
}
#[test]
fn presets_translate_and_reject_bad_abi_and_resources() {
    for source in [ROUNDED_BOX, PULSE_GLOW, PARTICLE] {
        ShaderProgram::<EffectInstance>::new(source).unwrap();
    }
    assert!(
        ShaderProgram::<EffectInstance>::new(
            &ROUNDED_BOX.replace("color: vec4<f32>", "color: vec3<f32>")
        )
        .is_err()
    );
    assert!(
        ShaderProgram::<EffectInstance>::new(&ROUNDED_BOX.replace("color:", "other:")).is_err()
    );
    assert!(
        ShaderProgram::<EffectInstance>::new(&format!(
            "{ROUNDED_BOX}\n@group(1) @binding(0) var tex: texture_2d<f32>;"
        ))
        .is_err()
    );
    assert!(
        ShaderProgram::<EffectInstance>::new(
            &ROUNDED_BOX.replace("effect: vec4<f32>", "effect: mat4x4<f32>")
        )
        .is_err()
    );
    assert!(
        program()
            .pack(&EffectInstance {
                color: [f32::NAN; 4],
                effect: [0.; 4]
            })
            .is_err()
    );
}
#[test]
fn batching_preserves_program_and_native_order_and_replay() {
    let a = program();
    let b = program();
    let mut scene = Scene::default();
    scene.insert_primitive(Primitive::Shader(draw(&a, 4., [1.; 4])));
    scene.insert_primitive(Primitive::Shader(draw(&a, 5., [1.; 4])));
    scene.insert_primitive(Primitive::Shader(draw(&b, 6., [1.; 4])));
    scene.insert_primitive(Quad {
        bounds: draw(&a, 7., [1.; 4]).bounds,
        content_mask: draw(&a, 7., [1.; 4]).content_mask,
        ..Default::default()
    });
    scene.insert_primitive(Primitive::Shader(draw(&a, 8., [1.; 4])));
    scene.finish();
    let batches: Vec<_> = scene.batches().collect();
    assert!(matches!(&batches[0], PrimitiveBatch::Shaders { range, .. } if *range == (0..2)));
    assert!(
        matches!(&batches[1], PrimitiveBatch::Shaders { program_id, .. } if *program_id == b.id())
    );
    assert!(matches!(&batches[2], PrimitiveBatch::Quads(_)));
    assert!(
        matches!(&batches[3], PrimitiveBatch::Shaders { program_id, .. } if *program_id == a.id())
    );
    let mut replay = Scene::default();
    replay.replay(0..scene.len(), &scene);
    replay.finish();
    assert_eq!(
        replay.batches().map(|b| b.label()).collect::<Vec<_>>(),
        batches.iter().map(|b| b.label()).collect::<Vec<_>>()
    );
    assert_eq!(replay.shader_draws[1].data, scene.shader_draws[1].data);
    replay.clear();
    assert!(replay.shader_draws.is_empty());
    assert_eq!(replay.batches().count(), 0);
}

#[cfg(all(target_os = "macos", feature = "wgpu"))]
#[test]
fn metal_and_wgpu_instances_clip_opacity_and_native_interleaving() {
    use gpui::{DevicePixels, PlatformHeadlessRenderer};
    let a = program();
    let mut scene = Scene::default();
    let mut first = draw(&a, 4., [1., 0., 0., 1.]);
    first.opacity = 0.5;
    first.content_mask.bounds.size.width = ScaledPixels(16.);
    scene.insert_primitive(Primitive::Shader(first));
    scene.insert_primitive(Primitive::Shader(draw(&a, 32., [0., 1., 0., 1.])));
    scene.insert_primitive(Quad {
        bounds: Bounds::new(
            point(ScaledPixels(8.), ScaledPixels(8.)),
            size(ScaledPixels(4.), ScaledPixels(4.)),
        ),
        content_mask: draw(&a, 0., [1.; 4]).content_mask,
        background: gpui::rgb(0x0000ff).into(),
        ..Default::default()
    });
    scene.finish();
    let extent = size(DevicePixels(64), DevicePixels(64));
    let mut metal = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    let mut wgpu = gpui_wgpu::WgpuHeadlessRenderer::new().unwrap();
    for image in [
        metal.render_scene_to_image(&scene, extent).unwrap(),
        wgpu.render_scene_to_image(&scene, extent).unwrap(),
    ] {
        let red = image.get_pixel(6, 12).0;
        assert!(
            (red[0] as i32 - 128).abs() <= 2 && red[1] < 2,
            "opacity: {red:?}"
        );
        assert_eq!(&image.get_pixel(20, 12).0[..3], &[0, 0, 0], "clip");
        assert_eq!(
            &image.get_pixel(36, 12).0[..3],
            &[0, 255, 0],
            "second instance"
        );
        assert_eq!(
            &image.get_pixel(10, 10).0[..3],
            &[0, 0, 255],
            "native order"
        );
    }
}

#[test]
fn five_thousand_particle_instances_form_one_ordered_run() {
    let particles = ShaderProgram::<EffectInstance>::new(PARTICLE).unwrap();
    let mut scene = Scene::default();
    for i in 0..5000 {
        scene.insert_primitive(Primitive::Shader(draw(
            &particles,
            (i % 40) as f32,
            [0.3, 0.6, 1., 0.5],
        )));
    }
    scene.finish();
    let batches: Vec<_> = scene.batches().collect();
    assert_eq!(batches.len(), 1);
    assert!(matches!(&batches[0], PrimitiveBatch::Shaders { range, .. } if *range == (0..5000)));
    assert_eq!(
        scene.shader_draws.len() * std::mem::size_of_val(&scene.shader_draws[0].instance_data()),
        880_000
    );
}

#[test]
fn scalar_after_vec3_uses_reflected_offset_without_rust_alignment_casts() {
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    #[repr(C)]
    struct Parameters {
        tint: [f32; 3],
        alpha: f32,
    }
    impl ShaderInstance for Parameters {
        const FIELDS: &'static [InstanceField] = &[
            InstanceField {
                name: "tint",
                offset: std::mem::offset_of!(Self, tint),
                components: 3,
            },
            InstanceField {
                name: "alpha",
                offset: std::mem::offset_of!(Self, alpha),
                components: 1,
            },
        ];
    }
    let program = ShaderProgram::<Parameters>::new("struct Parameters { tint: vec3<f32>, alpha: f32, } fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> { return vec4(p.tint, p.alpha); }").unwrap();
    assert_eq!(
        program
            .pack(&Parameters {
                tint: [0.1, 0.2, 0.3],
                alpha: 0.4
            })
            .unwrap()[0],
        [0.1, 0.2, 0.3, 0.4]
    );
}

#[test]
fn explicit_encoding_is_immutable_and_defaults_to_srgb() {
    let ordinary = program();
    let wide = ShaderProgram::<EffectInstance>::new_linear_p3(ROUNDED_BOX).unwrap();
    assert_eq!(
        ordinary.program.output_encoding(),
        ShaderOutputEncoding::EncodedSrgb
    );
    assert_eq!(
        wide.program.output_encoding(),
        ShaderOutputEncoding::LinearDisplayP3
    );
    assert_ne!(ordinary.id(), wide.id());
    assert_eq!(wide.clone().id(), wide.id());
    assert!(wide.program.wgsl().contains("if host.pad != 0u"));
    assert!(!ordinary.program.wgsl().contains("if host.pad != 0u"));
}

#[cfg(feature = "wgpu")]
#[test]
fn wgpu_rejects_wide_encoding() {
    use gpui::{DevicePixels, PlatformHeadlessRenderer};
    let wide = ShaderProgram::<EffectInstance>::new_linear_p3(ROUNDED_BOX).unwrap();
    let mut scene = Scene::default();
    scene.insert_primitive(Primitive::Shader(draw(&wide, 4., [1.; 4])));
    scene.finish();
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new().unwrap();
    assert!(
        renderer
            .render_scene_to_image(&scene, size(DevicePixels(64), DevicePixels(32)))
            .is_err()
    );
}
