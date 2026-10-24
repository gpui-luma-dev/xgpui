//! Producer regressions independent of the interactive paint editor.
use anyhow::{Result, ensure};
use gpui::*;
use std::sync::Arc;
use xgpui_shader::SolidColor;
mod mesh_checks;

fn producer(color: [f32; 4]) -> Arc<SolidColor> {
    Arc::new(SolidColor::new(color).unwrap())
}
fn draw(
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
    opacity: f32,
    content: Arc<dyn gpui::CustomContent>,
) -> CustomDraw {
    CustomDraw {
        order: 0,
        bounds: bounds.scale(1.),
        content_mask: ContentMask {
            bounds: mask.scale(1.),
        },
        corner_radii: Corners::all(px(12.)).scale(1.),
        opacity,
        content,
        coverage: None,
    }
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

fn quad(bounds: Bounds<Pixels>, color: u32) -> gpui::Quad {
    gpui::Quad {
        bounds: bounds.scale(1.),
        content_mask: ContentMask {
            bounds: bounds.scale(1.),
        },
        background: rgb(color).into(),
        ..Default::default()
    }
}
fn verify(renderer: &mut dyn gpui::PlatformHeadlessRenderer) -> Result<()> {
    let bounds = rect(0., 0., 128., 96.);
    let red = producer([1., 0., 0., 1.]);
    let mut scene = Scene::default();
    scene.insert_primitive(quad(bounds, 0x000000));
    scene.insert_primitive(draw(
        rect(16., 16., 80., 64.),
        rect(32., 16., 64., 64.),
        0.5,
        red.clone(),
    ));
    scene.insert_primitive(quad(rect(60., 40., 12., 12.), 0xffffff));
    let length = scene.len();
    scene.finish();
    let extent = size(DevicePixels(128), DevicePixels(96));
    let image = renderer.render_scene_to_image(&scene, extent)?;
    let sample = |x, y| image.get_pixel(x, y).0;
    ensure!(sample(20, 40) == [0, 0, 0, 255], "inherited clip failed");
    ensure!(sample(94, 17) == [0, 0, 0, 255], "rounded corner failed");
    ensure!(
        sample(50, 40)[0].abs_diff(128) <= 2 && sample(50, 40)[1] == 0,
        "opacity/color failed: {:?}",
        sample(50, 40)
    );
    ensure!(
        sample(64, 44) == [255, 255, 255, 255],
        "overlay ordering failed"
    );
    let mut replay = Scene::default();
    replay.replay(0..length, &scene);
    replay.finish();
    let replayed = renderer.render_scene_to_image(&replay, extent)?;
    ensure!(image == replayed, "scene replay changed output");
    ensure!(
        red.preparations() == 1,
        "static producer did not reuse its texture"
    );
    drop(scene);
    drop(replay);
    let mut empty = Scene::default();
    empty.insert_primitive(quad(bounds, 0x000000));
    empty.finish();
    ensure!(
        renderer
            .render_scene_to_image(&empty, extent)?
            .get_pixel(50, 40)
            .0
            == [0, 0, 0, 255],
        "removed content persisted"
    );
    // Two first-use records share an unsubmitted producer command. The host
    // must submit it exactly once, and both outputs must be initialized.
    let shared = producer([0., 1., 0., 0.5]);
    let mut multiple = Scene::default();
    multiple.insert_primitive(quad(bounds, 0x000000));
    multiple.insert_primitive(draw(rect(0., 0., 48., 48.), bounds, 1., shared.clone()));
    multiple.insert_primitive(draw(rect(64., 0., 48., 48.), bounds, 1., shared.clone()));
    multiple.finish();
    let image = renderer.render_scene_to_image(&multiple, extent)?;
    ensure!(
        image.get_pixel(24, 24).0[1].abs_diff(128) <= 2
            && image.get_pixel(88, 24).0[1].abs_diff(128) <= 2,
        "shared first-use/straight alpha failed"
    );
    ensure!(shared.preparations() == 1, "shared output was regenerated");
    // Resize the producer and destination; this creates a new immutable output.
    let mut resized = Scene::default();
    resized.insert_primitive(quad(bounds, 0x000000));
    resized.insert_primitive(draw(rect(0., 0., 65., 33.), bounds, 1., shared.clone()));
    resized.finish();
    ensure!(
        renderer
            .render_scene_to_image(&resized, extent)?
            .get_pixel(30, 16)
            .0[1]
            .abs_diff(128)
            <= 2,
        "resize output failed"
    );
    ensure!(
        shared.preparations() == 2,
        "resize did not regenerate output"
    );
    // A later bad producer aborts preparation before any commands are submitted.
    // Retrying the earlier cached pending output must still initialize it.
    let pending = producer([0., 0., 1., 1.]);
    let mut failed = Scene::default();
    failed.insert_primitive(draw(rect(0., 0., 48., 48.), bounds, 1., pending.clone()));
    failed.insert_primitive(draw(
        rect(64., 0., 48., 48.),
        bounds,
        1.,
        Arc::new(InvalidOutput),
    ));
    failed.finish();
    ensure!(
        renderer.render_scene_to_image(&failed, extent).is_err(),
        "invalid output was accepted"
    );
    let mut retry = Scene::default();
    retry.insert_primitive(draw(rect(0., 0., 48., 48.), bounds, 1., pending.clone()));
    retry.finish();
    ensure!(
        renderer
            .render_scene_to_image(&retry, extent)?
            .get_pixel(24, 24)
            .0[2]
            == 255,
        "aborted preparation poisoned cache"
    );
    ensure!(
        pending.preparations() == 1,
        "pending output was unnecessarily rebuilt"
    );
    let mut clipped = Scene::default();
    clipped.insert_primitive(draw(
        rect(200., 200., 48., 48.),
        bounds,
        1.,
        pending.clone(),
    ));
    ensure!(
        clipped.custom_draws.is_empty(),
        "fully clipped draw was retained"
    );
    Ok(())
}
#[derive(Debug)]
struct InvalidOutput;
impl gpui::CustomContent for InvalidOutput {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: gpui::Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        Ok(Box::new(()))
    }
}

#[::core::prelude::v1::test]
fn native_producer_pixels_replay_cache_and_failures() -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?.expect("native renderer");
    verify(renderer.as_mut())
}
#[::core::prelude::v1::test]
fn native_mesh_matches_cpu_and_preserves_snapshots() -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?.expect("native renderer");
    mesh_checks::verify_mesh(renderer.as_mut())
}
#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_mesh_matches_cpu_and_preserves_snapshots() -> Result<()> {
    mesh_checks::verify_mesh(&mut gpui_wgpu::WgpuHeadlessRenderer::new()?)
}
