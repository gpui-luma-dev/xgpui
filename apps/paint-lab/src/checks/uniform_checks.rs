use super::*;

#[derive(Debug)]
struct FailUniformFrame;
impl CustomContent for FailUniformFrame {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        anyhow::bail!("intentional uniform-frame preparation failure")
    }
}
fn uniform_scene(source: Arc<SolidColor>, count: usize, phase: usize) -> Scene {
    let mut scene = Scene::default();
    let bounds = rect(0., 0., 256., 20.);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: black().into(),
        ..Default::default()
    });
    for index in 0..count {
        let x = if phase.is_multiple_of(2) {
            index
        } else {
            count - 1 - index
        } as f32
            * 3.;
        let mut draw = record(rect(x, 0., 2., 16.), source.clone(), None);
        draw.corner_radii = Corners::default();
        draw.content_mask.bounds = rect(x, 0., 2., (8 + index % 4) as f32);
        draw.opacity = ((index + phase) % 4 + 1) as f32 / 4.;
        scene.insert_primitive(draw);
    }
    // Overlaps earlier custom bounds, forcing an ordinary native batch afterward.
    // Its color must not inherit custom uniforms, blending, shaders or texture bindings.
    let marker = rect(0., 4., 256., 2.);
    scene.insert_primitive(Quad {
        bounds: marker,
        content_mask: ContentMask { bounds: marker },
        background: rgb(0x00ff00).into(),
        ..Default::default()
    });
    scene.finish();
    scene
}
fn verify_uniform_frames(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let source = Arc::new(SolidColor::new([1.; 4])?);
    let extent = size(DevicePixels(256), DevicePixels(20));
    for (phase, count) in [1, 17, 65, 4, 65].into_iter().enumerate() {
        let scene = uniform_scene(source.clone(), count, phase);
        if phase == 2 {
            let mut failed = Scene::default();
            for draw in &scene.custom_draws {
                failed.insert_primitive(draw.clone());
            }
            failed.insert_primitive(record(
                rect(230., 0., 2., 16.),
                Arc::new(FailUniformFrame),
                None,
            ));
            failed.finish();
            let error = renderer.render_scene_to_image(&failed, extent).unwrap_err();
            ensure!(
                error.to_string().contains("intentional uniform-frame"),
                "unexpected preparation error: {error:#}"
            );
        }
        let image = renderer.render_scene_to_image(&scene, extent)?;
        for y in 0..20usize {
            for x in 0..256usize {
                let expected = if (4..6).contains(&y) {
                    [0, 255, 0, 255]
                } else {
                    let slot = x / 3;
                    let index = if phase.is_multiple_of(2) {
                        slot
                    } else {
                        count.saturating_sub(1 + slot)
                    };
                    let level = if slot < count && x % 3 < 2 && y < 8 + index % 4 {
                        (((index + phase) % 4 + 1) as f32 / 4. * 255.).round() as u8
                    } else {
                        0
                    };
                    [level, level, level, 255]
                };
                let actual = image.get_pixel(x as u32, y as u32).0;
                ensure!(
                    actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                    "uniform frame {phase}, ({x},{y}): {actual:?} != {expected:?}"
                );
            }
        }
    }
    ensure!(
        source.preparations() == 1,
        "uniform changes regenerated cached source"
    );
    Ok(())
}
#[::core::prelude::v1::test]
fn native_compositor_uniform_frames_recover_and_preserve_native_batches() -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?.expect("native renderer");
    verify_uniform_frames(renderer.as_mut())
}
#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_compositor_uniform_frames_recover_and_preserve_native_batches() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    verify_uniform_frames(&mut renderer)
}
