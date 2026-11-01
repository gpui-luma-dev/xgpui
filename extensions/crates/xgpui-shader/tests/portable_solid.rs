#![cfg(target_os = "macos")]
//! Native acceptance for the real extension producer. Only the compositor
//! validation target is read back; producer images remain GPU-owned.
use gpui::{
    Bounds, ContentMask, Corners, CustomDraw, DevicePixels, PlatformHeadlessRenderer, Quad,
    ScaledPixels, Scene, content::*, point, size,
};
use gpui_apple::{
    content_executor::{MetalContentLimits, MetalContentTestGate},
    metal_renderer::MetalHeadlessRenderer,
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
use xgpui_shader::SolidColor;
fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(w), ScaledPixels(h)),
    )
}
fn draw(solid: &SolidColor, bounds: Bounds<ScaledPixels>) -> CustomDraw {
    CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::default(),
        opacity: 1.,
        coverage: None,
        content: ContentDraw::new(solid.content()).into_scene_content(),
    }
}
fn background(scene: &mut Scene, bounds: Bounds<ScaledPixels>, gray: f32) {
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: gpui::Rgba {
            r: gray,
            g: gray,
            b: gray,
            a: 1.,
        }
        .into(),
        ..Default::default()
    });
}
fn wake(renderer: &mut MetalHeadlessRenderer) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = tx.send(());
    }));
    rx
}
fn ready(renderer: &mut dyn PlatformHeadlessRenderer, scene: &Scene, rx: &mpsc::Receiver<()>) {
    match renderer.prepare_content_snapshot(scene) {
        Ok(()) => {}
        Err(error) => {
            assert_eq!(
                error.downcast_ref::<ContentError>().unwrap().kind,
                ContentErrorKind::Pending
            );
            rx.recv_timeout(Duration::from_secs(20)).unwrap();
            renderer.prepare_content_snapshot(scene).unwrap();
        }
    }
}
#[test]
fn straight_srgb_alpha_rounding_clip_opacity_and_order_match_reference() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let extent = size(DevicePixels(16), DevicePixels(12));
    let viewport = rect(0., 0., 16., 12.);
    for gray in [0., 1.] {
        for alpha in [0., 0.5, 1.] {
            for opacity in [0.5, 1.] {
                let solid = SolidColor::new([1., 0.25, 0., alpha]).unwrap();
                let mut scene = Scene::default();
                background(&mut scene, viewport, gray);
                let mut d = draw(&solid, rect(2., 2., 12., 8.));
                d.corner_radii = Corners::all(ScaledPixels(3.));
                d.content_mask.bounds = rect(4., 0., 12., 12.);
                d.opacity = opacity;
                scene.insert_primitive(d);
                let marker = rect(8., 5., 2., 2.);
                background(&mut scene, marker, 1.);
                scene.finish();
                ready(&mut renderer, &scene, &rx);
                let image = renderer.render_scene_to_image(&scene, extent).unwrap();
                for (x, y, pixel) in image.enumerate_pixels() {
                    let expected = if (8..10).contains(&x) && (5..7).contains(&y) {
                        [255; 4]
                    } else {
                        let qx = ((x as f32 + 0.5) - 8.).abs() - 6. + 3.;
                        let qy = ((y as f32 + 0.5) - 6.).abs() - 4. + 3.;
                        let distance = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - 3.;
                        let coverage = if x >= 4 && (2..14).contains(&x) && (2..10).contains(&y) {
                            (0.5 - distance).clamp(0., 1.)
                        } else {
                            0.
                        };
                        let a = (alpha * 255.).round() / 255. * opacity * coverage;
                        [1., 64. / 255., 0.]
                            .map(|v| ((v * a + gray * (1. - a)) * 255.).round() as u8)
                            .into_iter()
                            .chain([255])
                            .collect::<Vec<_>>()
                            .try_into()
                            .unwrap()
                    };
                    for (actual, expected) in pixel.0.into_iter().zip(expected) {
                        assert!(
                            actual.abs_diff(expected) <= 2,
                            "({x},{y}) {pixel:?} != {expected}, alpha={alpha} opacity={opacity}"
                        );
                    }
                }
                assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
            }
        }
    }
}
#[test]
fn host_owns_shared_realizations_replay_and_resize() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let solid = SolidColor::new([0., 1., 0., 0.5]).unwrap();
    let extent = size(DevicePixels(32), DevicePixels(16));
    let mut scene = Scene::default();
    background(&mut scene, rect(0., 0., 32., 16.), 0.);
    scene.insert_primitive(draw(&solid, rect(0., 0., 8., 8.)));
    scene.insert_primitive(draw(&solid.clone(), rect(16., 0., 8., 8.)));
    scene.finish();
    ready(&mut renderer, &scene, &rx);
    let image = renderer.render_scene_to_image(&scene, extent).unwrap();
    assert_eq!(image.get_pixel(4, 4).0, [0, 128, 0, 255]);
    assert_eq!(image.get_pixel(20, 4).0, [0, 128, 0, 255]);
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.execution.operations, 1);
    assert_eq!(stats.cached_images, 1);
    assert_eq!(stats.execution.readbacks, 0);
    let mut replay = Scene::default();
    replay.replay(0..scene.len(), &scene);
    replay.finish();
    assert_eq!(
        renderer.render_scene_to_image(&replay, extent).unwrap(),
        image
    );
    assert_eq!(
        renderer.content_presentation_stats().requests,
        stats.requests
    );
    let mut resized = Scene::default();
    background(&mut resized, rect(0., 0., 32., 16.), 0.);
    resized.insert_primitive(draw(&solid, rect(0., 0., 17., 11.)));
    resized.finish();
    ready(&mut renderer, &resized, &rx);
    assert_eq!(
        renderer
            .render_scene_to_image(&resized, extent)
            .unwrap()
            .get_pixel(8, 5)
            .0,
        [0, 128, 0, 255]
    );
    assert_eq!(
        renderer.content_presentation_stats().requests,
        stats.requests + 1
    );
}
struct Release(Arc<MetalContentTestGate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
#[test]
fn generic_headless_readiness_never_reports_a_pending_fallback_as_success() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let gate = Release(Arc::new(MetalContentTestGate::default()));
    renderer.set_content_validation_limits(MetalContentLimits {
        pause_before_node: Some((0, gate.0.clone())),
        ..Default::default()
    });
    let mut scene = Scene::default();
    scene.insert_primitive(draw(
        &SolidColor::new([1.; 4]).unwrap(),
        rect(0., 0., 4., 4.),
    ));
    scene.finish();
    let error =
        PlatformHeadlessRenderer::prepare_content_snapshot(&mut renderer, &scene).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ContentError>().unwrap().kind,
        ContentErrorKind::Pending
    );
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    gate.0.release();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    PlatformHeadlessRenderer::prepare_content_snapshot(&mut renderer, &scene).unwrap();
    assert_eq!(renderer.content_presentation_stats().requests, 1);
}
#[test]
fn unsupported_headless_executor_rejects_portable_content_before_recording() {
    struct NoExecutor;
    impl PlatformHeadlessRenderer for NoExecutor {
        fn render_scene_to_image(
            &mut self,
            _: &Scene,
            _: gpui::Size<DevicePixels>,
        ) -> anyhow::Result<image::RgbaImage> {
            panic!("must not record/read back")
        }
        fn render_scene(&mut self, _: &Scene, _: gpui::Size<DevicePixels>) -> anyhow::Result<()> {
            panic!("must not record")
        }
        fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
            panic!("unused")
        }
    }
    let mut scene = Scene::default();
    scene.insert_primitive(draw(
        &SolidColor::new([1.; 4]).unwrap(),
        rect(0., 0., 4., 4.),
    ));
    scene.finish();
    let error = NoExecutor.prepare_content_snapshot(&scene).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ContentError>().unwrap().kind,
        ContentErrorKind::UnsupportedCapability
    );
    NoExecutor
        .prepare_content_snapshot(&Scene::default())
        .unwrap();
}
