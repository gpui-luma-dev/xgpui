#![cfg(all(target_os = "macos", feature = "test-support"))]
//! Exercise the window renderer's producer/compositor path. Only the final
//! validation target is read back; producer texture transfers stay on the GPU.
use gpui::{
    Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, Quad, ScaledPixels,
    Scene, WindowColorMode, black, content::*, point, size,
};
use gpui_apple::{
    content_executor::*, metal::MTLCommandBuffer, metal_renderer::MetalHeadlessRenderer,
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
fn bounds(width: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(width), ScaledPixels(4.)),
    )
}
fn scene(draw: ContentDraw) -> Scene {
    let mut scene = Scene::default();
    let bounds = bounds(4.);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: black().into(),
        ..Default::default()
    });
    scene.insert_primitive(CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Default::default(),
        opacity: 1.,
        content: draw.into_scene_content(),
        coverage: None,
    });
    scene.finish();
    scene
}
fn solid(rgba: [f32; 4]) -> Content {
    Content::solid(ColorValue::srgb(rgba).unwrap())
}
fn read(renderer: &mut MetalHeadlessRenderer, scene: &Scene) -> image::RgbaImage {
    renderer
        .render_scene_to_image(scene, size(DevicePixels(4), DevicePixels(4)))
        .unwrap()
}
fn pixels(image: &image::RgbaImage, expected: [u8; 4]) {
    for pixel in image.pixels() {
        assert_eq!(pixel.0, expected);
    }
}
fn wake(renderer: &mut MetalHeadlessRenderer) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = tx.send(());
    }));
    rx
}
fn completed(rx: &mpsc::Receiver<()>) {
    rx.recv_timeout(Duration::from_secs(20))
        .expect("producer completion must request redraw");
}
struct Release(Arc<MetalContentTestGate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
fn pause(renderer: &mut MetalHeadlessRenderer) -> Release {
    let gate = Arc::new(MetalContentTestGate::default());
    renderer.set_content_validation_limits(MetalContentLimits {
        pause_before_node: Some((0, gate.clone())),
        ..Default::default()
    });
    Release(gate)
}
#[test]
fn pending_replacement_replay_and_slot_removal_have_no_producer_readback() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slot = PresentationSlotId::new();
    let red = scene(ContentDraw::new(solid([1., 0., 0., 1.])).slot(slot));
    let gate = pause(&mut renderer);
    pixels(&read(&mut renderer, &red), [0, 0, 0, 255]);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    pixels(&read(&mut renderer, &red), [0, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
    gate.0.release();
    completed(&rx);
    pixels(&read(&mut renderer, &red), [255, 0, 0, 255]);
    pixels(&read(&mut renderer, &red), [255, 0, 0, 255]);
    let gate = pause(&mut renderer);
    let green = scene(ContentDraw::new(solid([0., 1., 0., 1.])).slot(slot));
    pixels(&read(&mut renderer, &green), [255, 0, 0, 255]);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    gate.0.release();
    completed(&rx);
    pixels(&read(&mut renderer, &green), [0, 255, 0, 255]);
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.requests, 2);
    assert_eq!(stats.completions, 2);
    assert_eq!(stats.execution.readbacks, 0);
    assert_eq!(stats.execution.operations, 1);
    read(&mut renderer, &Scene::default());
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.retained_slots, 0);
    assert_eq!(stats.cached_images, 0);
    let gate = pause(&mut renderer);
    pixels(&read(&mut renderer, &red), [0, 0, 0, 255]); // Removed slot cannot inherit history.
    gate.0.release();
    completed(&rx);
}
#[test]
fn collapsing_512_tiles_stretches_retained_texture_before_replacement_completes() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots: Vec<_> = (0..512).map(|_| PresentationSlotId::new()).collect();
    let source = solid([1., 0., 0., 1.]);
    let extent = size(DevicePixels(64), DevicePixels(32));
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(64.), ScaledPixels(32.)),
    );
    let make_scene = |dense: bool| {
        let mut scene = Scene::default();
        scene.insert_primitive(Quad {
            bounds: full,
            content_mask: ContentMask { bounds: full },
            background: black().into(),
            ..Default::default()
        });
        for (index, slot) in slots.iter().enumerate().take(if dense { 512 } else { 1 }) {
            let bounds = if dense {
                Bounds::new(
                    point(
                        ScaledPixels((index % 32) as f32 * 2.),
                        ScaledPixels((index / 32) as f32 * 2.),
                    ),
                    size(ScaledPixels(1.), ScaledPixels(1.)),
                )
            } else {
                full
            };
            scene.insert_primitive(CustomDraw {
                order: 0,
                bounds,
                content_mask: ContentMask { bounds: full },
                corner_radii: Default::default(),
                opacity: 1.,
                content: ContentDraw::new(source.clone())
                    .slot(*slot)
                    .into_scene_content(),
                coverage: None,
            });
        }
        scene.finish();
        scene
    };
    let dense = make_scene(true);
    renderer.render_scene(&dense, extent).unwrap();
    completed(&rx);
    let image = renderer.render_scene_to_image(&dense, extent).unwrap();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(1, 1).0, [0, 0, 0, 255]);

    let gate = pause(&mut renderer);
    let single = make_scene(false);
    // Hold the full-sized replacement indefinitely. Every composed pixel must
    // already use the new bounds while sampling the retained 1x1 texture.
    let image = renderer.render_scene_to_image(&single, extent).unwrap();
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    pixels(&image, [255, 0, 0, 255]);
    pixels(
        &renderer.render_scene_to_image(&single, extent).unwrap(),
        [255, 0, 0, 255],
    );
    assert_eq!(renderer.content_presentation_stats().retained_slots, 1);
    gate.0.release();
    completed(&rx);
    pixels(
        &renderer.render_scene_to_image(&single, extent).unwrap(),
        [255, 0, 0, 255],
    );
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
#[test]
fn admitted_generation_progresses_before_latest_slot_replacement() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slot = PresentationSlotId::new();
    let gate = pause(&mut renderer);
    let red = scene(ContentDraw::new(solid([1., 0., 0., 1.])).slot(slot));
    pixels(&read(&mut renderer, &red), [0, 0, 0, 255]);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    renderer.set_content_validation_limits(Default::default());
    let blue = scene(ContentDraw::new(solid([0., 0., 1., 1.])).slot(slot));
    pixels(&read(&mut renderer, &blue), [0, 0, 0, 255]);
    gate.0.release();
    completed(&rx);
    // The progress policy preserves the admitted generation while newer inputs
    // coalesce. Harvest its red pixels, then admit and complete the latest blue.
    pixels(&read(&mut renderer, &blue), [255, 0, 0, 255]);
    completed(&rx);
    pixels(&read(&mut renderer, &blue), [0, 0, 255, 255]);
    assert_eq!(renderer.content_presentation_stats().completions, 2);
}
#[test]
fn failed_generation_retains_or_clears_and_duplicate_slots_are_rejected() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slot = PresentationSlotId::new();
    let red = scene(ContentDraw::new(solid([1., 0., 0., 1.])).slot(slot));
    read(&mut renderer, &red);
    completed(&rx);
    pixels(&read(&mut renderer, &red), [255, 0, 0, 255]);
    renderer.set_content_validation_limits(MetalContentLimits {
        fail_after_admit: Some(0),
        ..Default::default()
    });
    let green = solid([0., 1., 0., 1.]);
    let failed = scene(ContentDraw::new(green.clone()).slot(slot));
    pixels(&read(&mut renderer, &failed), [255, 0, 0, 255]);
    completed(&rx);
    pixels(&read(&mut renderer, &failed), [255, 0, 0, 255]);
    assert_eq!(
        renderer
            .content_presentation_stats()
            .last_error
            .unwrap()
            .kind,
        ContentErrorKind::ExecutionFailure
    );
    let clear = scene(
        ContentDraw::new(green.clone()).presentation(ContentPresentation {
            slot: Some(slot),
            on_error: ContentErrorPresentation::ReportAndTransparent,
            ..Default::default()
        }),
    );
    pixels(&read(&mut renderer, &clear), [0, 0, 0, 255]);
    let strict = scene(ContentDraw::new(green).presentation(ContentPresentation {
        slot: Some(slot),
        on_error: ContentErrorPresentation::FailFrame,
        ..Default::default()
    }));
    assert!(
        renderer
            .render_scene_to_image(&strict, size(DevicePixels(4), DevicePixels(4)))
            .is_err()
    );
    let mut duplicate = Scene::default();
    duplicate.insert_primitive(failed.custom_draws[0].clone());
    duplicate.insert_primitive(failed.custom_draws[0].clone());
    duplicate.finish();
    assert!(
        renderer
            .render_scene_to_image(&duplicate, size(DevicePixels(4), DevicePixels(4)))
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    renderer.set_content_validation_limits(Default::default());
    let blue = scene(ContentDraw::new(solid([0., 0., 1., 1.])).slot(slot));
    read(&mut renderer, &blue);
    completed(&rx);
    pixels(&read(&mut renderer, &blue), [0, 0, 255, 255]);
}
#[test]
fn descriptor_alpha_and_linear_color_are_applied_once_by_the_compositor() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    for (color, expected) in [
        (
            ColorValue::srgb([1., 0., 0., 0.5]).unwrap(),
            [128, 0, 0, 255],
        ),
        (
            ColorValue::new(
                [0.5, 0., 0., 0.5],
                ImageInterpretation {
                    color: ColorEncoding::Srgb,
                    alpha: AlphaMode::Premultiplied,
                },
            )
            .unwrap(),
            [128, 0, 0, 255],
        ),
        (
            ColorValue::new(
                [0.25, 0., 0., 0.5],
                ImageInterpretation {
                    color: ColorEncoding::LinearSrgb,
                    alpha: AlphaMode::Straight,
                },
            )
            .unwrap(),
            [69, 0, 0, 255],
        ),
        (
            ColorValue::new(
                [0.125, 0., 0., 0.5],
                ImageInterpretation {
                    color: ColorEncoding::LinearSrgb,
                    alpha: AlphaMode::Premultiplied,
                },
            )
            .unwrap(),
            [69, 0, 0, 255],
        ),
    ] {
        let scene = scene(ContentDraw::new(Content::solid(color)));
        read(&mut renderer, &scene);
        completed(&rx);
        let image = read(&mut renderer, &scene);
        for pixel in image.pixels() {
            for (actual, expected) in pixel.0.into_iter().zip(expected) {
                assert!(actual.abs_diff(expected) <= 1, "{pixel:?} vs {expected}");
            }
        }
        assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    }
}
#[test]
fn wide_presenter_preserves_linear_p3_and_rejects_implicit_srgb_downgrade() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let color = ColorValue::new(
        [0.25, 0.125, 0., 0.5],
        ImageInterpretation {
            color: ColorEncoding::LinearP3,
            alpha: AlphaMode::Premultiplied,
        },
    )
    .unwrap();
    let scene = scene(ContentDraw::new(Content::solid(color)));
    read(&mut renderer, &scene);
    assert_eq!(
        renderer
            .content_presentation_stats()
            .last_error
            .unwrap()
            .kind,
        ContentErrorKind::UnsupportedCapability
    );
    renderer
        .set_color_mode(WindowColorMode::WideGamutSdr)
        .unwrap();
    // A presentation-policy change must invalidate an earlier capability error.
    renderer
        .render_scene_to_linear_p3(&scene, size(DevicePixels(4), DevicePixels(4)))
        .unwrap();
    completed(&rx);
    let image = renderer
        .render_scene_to_linear_p3(&scene, size(DevicePixels(4), DevicePixels(4)))
        .unwrap();
    for pixel in image.pixels {
        assert_eq!(pixel, [0x3400, 0x3000, 0, 0x3c00]);
    }
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
#[derive(Debug)]
struct ComputeImage(PortableProgram);
impl ContentSource for ComputeImage {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> Result<ImageRef, ContentError> {
        let descriptor = ImageDescriptor::new(
            request.extent(),
            PixelFormat::Rgba8,
            ImageInterpretation {
                color: ColorEncoding::Srgb,
                alpha: AlphaMode::Straight,
            },
            1,
        )?;
        let result =
            cx.compute(ComputePass::new(&self.0, [4, 4, 1]).write_image("output", descriptor))?;
        result.image("output")
    }
}
#[test]
fn compute_texture_flows_to_compositor_with_clip_opacity_and_scene_order() {
    let interpretation = ImageInterpretation {
        color: ColorEncoding::Srgb,
        alpha: AlphaMode::Straight,
    };
    let program = PortableProgram::compute("@group(0) @binding(0) var output: texture_storage_2d<rgba8unorm, write>; @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) { textureStore(output, vec2<i32>(id.xy), vec4(1.0,0.0,0.0,1.0)); }", "main", ImageContracts::new().image("output",interpretation)).unwrap();
    let mut scene = scene(ContentDraw::new(Content::new(ComputeImage(program))));
    scene.custom_draws[0].opacity = 0.5;
    scene.custom_draws[0].content_mask.bounds.origin.x = ScaledPixels(2.);
    scene.custom_draws[0].content_mask.bounds.size.width = ScaledPixels(2.);
    let white = Bounds::new(
        point(ScaledPixels(3.), ScaledPixels(0.)),
        size(ScaledPixels(1.), ScaledPixels(4.)),
    );
    scene.insert_primitive(Quad {
        bounds: white,
        content_mask: ContentMask { bounds: white },
        background: gpui::white().into(),
        ..Default::default()
    });
    scene.finish();
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    read(&mut renderer, &scene);
    completed(&rx);
    let image = read(&mut renderer, &scene);
    assert_eq!(image.get_pixel(0, 2).0, [0, 0, 0, 255]);
    assert_eq!(image.get_pixel(2, 2).0, [128, 0, 0, 255]);
    assert_eq!(image.get_pixel(3, 2).0, [255, 255, 255, 255]);
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
#[test]
fn dense_compositor_path_finishes_in_a_bounded_subprocess() {
    use std::{process::Command, time::Instant};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "dense_presentation_worker",
            "--nocapture",
        ])
        .env("XGPUI_DENSE_PRESENTATION_CHILD", "1")
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(45) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("dense GPU presentation timed out")
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "subprocess helper; the dense compositor test invokes it"]
fn dense_presentation_worker() {
    assert_eq!(
        std::env::var("XGPUI_DENSE_PRESENTATION_CHILD").as_deref(),
        Ok("1")
    );
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots: Vec<_> = (0..512).map(|_| PresentationSlotId::new()).collect();
    for cycle in 0..3 {
        for count in [512, 1, 512] {
            let mut scene = Scene::default();
            for (index, slot) in slots.iter().enumerate().take(count) {
                let bounds = Bounds::new(
                    point(ScaledPixels(index as f32), ScaledPixels(0.)),
                    size(ScaledPixels(1.), ScaledPixels(4.)),
                );
                let value = (index + 1) as f32 / count as f32;
                let color = if cycle % 2 == 0 {
                    [value, 0., 0., 1.]
                } else {
                    [0., value, 0., 1.]
                };
                scene.insert_primitive(CustomDraw {
                    order: 0,
                    bounds,
                    content_mask: ContentMask { bounds },
                    corner_radii: Default::default(),
                    opacity: 1.,
                    content: ContentDraw::new(solid(color))
                        .slot(*slot)
                        .into_scene_content(),
                    coverage: None,
                });
            }
            scene.finish();
            let extent = size(DevicePixels(count as i32), DevicePixels(4));
            let before = renderer.content_presentation_stats().requests;
            renderer.render_scene(&scene, extent).unwrap();
            if renderer.content_presentation_stats().requests > before {
                completed(&rx);
            }
            for _ in 0..2 {
                let image = renderer.render_scene_to_image(&scene, extent).unwrap();
                for (x, _, pixel) in image.enumerate_pixels() {
                    let value = (((x + 1) as f32 / count as f32) * 255.).round() as u8;
                    let expected = if cycle % 2 == 0 {
                        [value, 0, 0, 255]
                    } else {
                        [0, value, 0, 255]
                    };
                    assert_eq!(pixel.0, expected);
                }
                let stats = renderer.content_presentation_stats();
                assert_eq!(stats.execution.readbacks, 0);
                assert!(stats.execution.max_in_flight <= 2);
                eprintln!(
                    "GPU presentation cycle={cycle} count={count}: pixels verified, producer_readbacks=0, max_in_flight={}",
                    stats.execution.max_in_flight
                );
            }
        }
    }
}
#[test]
fn shared_device_budget_and_sampling_lease_survive_renderer_teardown() {
    let mut first = MetalHeadlessRenderer::new();
    let rx = wake(&mut first);
    let mut second = MetalHeadlessRenderer::new();
    let second_rx = wake(&mut second);
    assert_eq!(
        first.content_presentation_stats().device_service,
        second.content_presentation_stats().device_service
    );
    let red = scene(ContentDraw::new(solid([1., 0., 0., 1.])));
    read(&mut first, &red);
    completed(&rx);
    pixels(&read(&mut first, &red), [255, 0, 0, 255]);
    let held = second.content_presentation_stats().retained_gpu_bytes;
    assert!(held > 0);
    let frame = first
        .prepare_validation_frame(&red, size(DevicePixels(4), DevicePixels(4)))
        .unwrap();
    drop(first);
    assert_eq!(
        second.content_presentation_stats().retained_gpu_bytes,
        held,
        "unsubmitted compositor must retain its source reservation"
    );
    second.set_content_validation_limits(MetalContentLimits {
        max_gpu_bytes: held + 1,
        ..Default::default()
    });
    let blue = scene(ContentDraw::new(solid([0., 0., 1., 1.])));
    read(&mut second, &blue);
    completed(&second_rx);
    read(&mut second, &blue);
    assert_eq!(
        second.content_presentation_stats().last_error.unwrap().kind,
        ContentErrorKind::ResourceLimit
    );
    frame.command.commit();
    frame.command.waitUntilCompleted();
    pixels(&frame.read_completed_image().unwrap(), [255, 0, 0, 255]);
    drop(frame);
    assert_eq!(second.content_presentation_stats().retained_gpu_bytes, 0);
    second.set_content_validation_limits(Default::default());
    let green = scene(ContentDraw::new(solid([0., 1., 0., 1.])));
    read(&mut second, &green);
    completed(&second_rx);
    pixels(&read(&mut second, &green), [0, 255, 0, 255]);
}
#[derive(Debug)]
struct NaturalExtent(ImageExtent);
impl ContentSource for NaturalExtent {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        _: &ContentRequest,
    ) -> Result<ImageRef, ContentError> {
        let image = cx.solid(self.0, ColorValue::srgb([1., 0., 0., 1.])?)?;
        cx.copy_image(image)
    }
}
#[test]
fn natural_odd_sized_sources_reserve_whole_texture_pages_and_stretch_to_destination() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    for (width, height) in [(67, 77), (3, 65), (257, 129), (66, 74)] {
        let scene = scene(ContentDraw::new(Content::new(NaturalExtent(
            ImageExtent::new(width, height).unwrap(),
        ))));
        read(&mut renderer, &scene);
        completed(&rx);
        pixels(&read(&mut renderer, &scene), [255, 0, 0, 255]);
        assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    }
}
#[test]
fn equivalent_reconstructed_sources_share_pending_work_and_transparent_policy_is_explicit() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slot = PresentationSlotId::new();
    let gate = pause(&mut renderer);
    let red = scene(ContentDraw::new(solid([1., 0., 0., 1.])).slot(slot));
    read(&mut renderer, &red);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    let equivalent = scene(ContentDraw::new(solid([1., 0., 0., 1.])).slot(slot));
    read(&mut renderer, &equivalent);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
    gate.0.release();
    completed(&rx);
    pixels(&read(&mut renderer, &equivalent), [255, 0, 0, 255]);
    let gate = pause(&mut renderer);
    let green = scene(ContentDraw::new(solid([0., 1., 0., 1.])).presentation(
        ContentPresentation {
            slot: Some(slot),
            pending: PendingPresentation::Transparent,
            ..Default::default()
        },
    ));
    pixels(&read(&mut renderer, &green), [0, 0, 0, 255]);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    gate.0.release();
    completed(&rx);
    pixels(&read(&mut renderer, &green), [0, 255, 0, 255]);
    let gate = pause(&mut renderer);
    let anonymous = scene(ContentDraw::new(solid([0., 0., 1., 1.])));
    pixels(&read(&mut renderer, &anonymous), [0, 0, 0, 255]);
    gate.0.release();
    completed(&rx);
}
#[test]
fn coverage_subscenes_preserve_parent_subscriptions_and_slot_history() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slot = PresentationSlotId::new();
    let mut border = scene(ContentDraw::new(solid([1., 0., 0., 1.])).slot(slot));
    border.custom_draws[0].coverage = Some(gpui::CustomCoverage::Border {
        widths: gpui::Edges::all(ScaledPixels(1.)),
        style: gpui::BorderStyle::Solid,
    });
    read(&mut renderer, &border);
    completed(&rx);
    let image = read(&mut renderer, &border);
    assert_eq!(image.get_pixel(0, 2).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(2, 2).0, [0, 0, 0, 255]);
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.requests, 1);
    assert_eq!(stats.retained_slots, 1);
    assert_eq!(stats.cached_images, 1);
    read(&mut renderer, &border);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
}
#[test]
fn strict_snapshot_preparation_reports_pending_instead_of_accepting_a_fallback() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let gate = pause(&mut renderer);
    let scene = scene(ContentDraw::new(solid([1., 0., 0., 1.])));
    let error = renderer.prepare_content_snapshot(&scene).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ContentError>().unwrap().kind,
        ContentErrorKind::Pending
    );
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    gate.0.release();
    completed(&rx);
    renderer.prepare_content_snapshot(&scene).unwrap();
    pixels(&read(&mut renderer, &scene), [255, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
}

#[test]
fn pending_image_asset_wakes_idle_presenter_and_uploads_without_failure_cache() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let (asset, done) = ImageAsset::pending();
    let scene = scene(ContentDraw::new(Content::image(asset)));
    pixels(&read(&mut renderer, &scene), [0, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().requests, 0);
    let descriptor = ImageDescriptor::new(
        ImageExtent::new(1, 1).unwrap(),
        PixelFormat::Rgba8,
        ImageInterpretation {
            color: ColorEncoding::Srgb,
            alpha: AlphaMode::Straight,
        },
        1,
    )
    .unwrap();
    done.resolve(Ok(ImagePixels::new(
        descriptor,
        Arc::from([255, 0, 0, 255]),
        4,
    )
    .unwrap()));
    completed(&rx); // CPU asset completion must wake even with no GPU job.
    let _ = read(&mut renderer, &scene);
    completed(&rx); // GPU upload completion.
    pixels(&read(&mut renderer, &scene), [255, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
}

#[test]
fn removed_image_asset_does_not_wake_presenter_on_late_completion() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let (asset, done) = ImageAsset::pending();
    let pending = scene(ContentDraw::new(Content::image(asset)));
    let _ = read(&mut renderer, &pending);
    let mut empty = Scene::default();
    empty.finish();
    let _ = read(&mut renderer, &empty);
    drop(done);
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    assert_eq!(renderer.content_presentation_stats().requests, 0);
}
