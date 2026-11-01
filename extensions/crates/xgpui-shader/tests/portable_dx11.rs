#![cfg(all(target_os = "windows", feature = "portable-shaders"))]
use gpui::{content::*, px, size};
use gpui_windows::content_executor::*;
use std::{sync::Arc, time::Duration};
use xgpui_shader::{
    Gradient, GradientStop, MeshGradient, MeshGrid, SolidColor, shader_paint::ShaderProgram,
};
#[test]
fn portable_producers_execute_on_dx11_with_a_one_submission_limit() {
    let executor = Dx11ContentExecutor::new(Dx11ContentOptions {
        queue_capacity: 1,
        ..Default::default()
    })
    .unwrap();
    let gradient = Gradient::linear(
        90.,
        vec![
            GradientStop::new(0., [1., 0., 0., 1.]),
            GradientStop::new(1., [1., 0., 0., 1.]),
        ],
    )
    .unwrap();
    let mesh =
        MeshGradient::new(MeshGrid::regular(3, 3, vec![[1., 0., 0.]; 9]).unwrap(), 18).unwrap();
    let shader=ShaderProgram::new("struct Parameters { color:vec4<f32> } fn paint(input:PaintInput)->vec4<f32>{return parameters.color;}").unwrap().paint(UniformValues::new().vec4("color",[1.,0.,0.,1.])).unwrap();
    let sources = [
        SolidColor::new([1., 0., 0., 1.]).unwrap().content(),
        gradient.content(),
        mesh.content(),
        shader.content(),
    ];
    for content in sources {
        let graph = ContentGraph::describe(
            &[ContentRoot {
                content,
                request: ContentRequest::new(
                    size(px(65.), px(33.)),
                    ImageExtent::new(65, 33).unwrap(),
                    1.,
                )
                .unwrap(),
            }],
            executor.capabilities().clone(),
            GraphLimits::default(),
        )
        .unwrap();
        let frame = executor
            .submit(Arc::new(graph), Default::default())
            .unwrap()
            .wait_for_readback(Duration::from_secs(20))
            .unwrap();
        assert!(
            frame.images[0].levels[0]
                .chunks_exact(4)
                .all(|p| p == [255, 0, 0, 255])
        );
        assert!(frame.stats.max_in_flight <= 1);
        assert_eq!(frame.stats.readbacks, 1);
        eprintln!(
            "dx11 source passed: operations={} pipeline_compilations={} max_in_flight={}",
            frame.stats.operations, frame.stats.pipeline_compilations, frame.stats.max_in_flight
        );
    }
}

#[test]
fn dx11_compositor_presents_completed_content_without_producer_readback() {
    use gpui::{
        Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, ScaledPixels,
        Scene, point,
    };
    let mut renderer = gpui_windows::Dx11HeadlessRenderer::new().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = tx.send(());
    }));
    let slot = PresentationSlotId::new();
    let make = |rgba| {
        let content = SolidColor::new(rgba).unwrap().content();
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(16.), ScaledPixels(8.)),
        );
        let mut scene = Scene::default();
        scene.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Default::default(),
            opacity: 1.,
            coverage: None,
            content: ContentDraw::new(content).slot(slot).into_scene_content(),
        });
        scene.finish();
        scene
    };
    let first_gate = Arc::new(Dx11ContentTestGate::default());
    renderer.set_content_validation_limits(Dx11ContentLimits {
        pause_before_node: Some((0, first_gate.clone())),
        ..Default::default()
    });
    let red = make([1., 0., 0., 1.]);
    let pending = renderer.prepare_content_snapshot(&red).unwrap_err();
    assert_eq!(
        pending.downcast_ref::<ContentError>().unwrap().kind,
        ContentErrorKind::Pending
    );
    assert!(first_gate.wait_until_entered(Duration::from_secs(20)));
    first_gate.release();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    renderer.set_content_validation_limits(Default::default());
    renderer.prepare_content_snapshot(&red).unwrap();
    let image = renderer
        .render_scene_to_image(&red, size(DevicePixels(16), DevicePixels(8)))
        .unwrap();
    assert!(image.pixels().all(|p| p.0 == [255, 0, 0, 255]));
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.execution.readbacks, 0);
    assert_eq!(stats.cached_images, 1);
    let green = make([0., 1., 0., 1.]);
    let image = renderer
        .render_scene_to_image(&green, size(DevicePixels(16), DevicePixels(8)))
        .unwrap();
    assert!(image.pixels().all(|p| p.0 == [255, 0, 0, 255]));
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    renderer.prepare_content_snapshot(&green).unwrap();
    let image = renderer
        .render_scene_to_image(&green, size(DevicePixels(16), DevicePixels(8)))
        .unwrap();
    assert!(image.pixels().all(|p| p.0 == [0, 255, 0, 255]));
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    // A completed result must advance slot history before newer input replaces it.
    let blue_gate = Arc::new(Dx11ContentTestGate::default());
    renderer.set_content_validation_limits(Dx11ContentLimits {
        pause_before_node: Some((0, blue_gate.clone())),
        ..Default::default()
    });
    let blue = make([0., 0., 1., 1.]);
    renderer
        .render_scene_to_image(&blue, size(DevicePixels(16), DevicePixels(8)))
        .unwrap();
    assert!(blue_gate.wait_until_entered(Duration::from_secs(20)));
    // Input continues while execution is paused. The admitted generation must
    // finish rather than restart at node zero for every newer frame.
    let requests = renderer.content_presentation_stats().requests;
    for i in 0..12 {
        let newer = make([i as f32 / 12., 0.2, 0.8, 1.]);
        renderer
            .render_scene_to_image(&newer, size(DevicePixels(16), DevicePixels(8)))
            .unwrap();
        assert_eq!(renderer.content_presentation_stats().requests, requests);
    }
    blue_gate.release();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    let yellow_gate = Arc::new(Dx11ContentTestGate::default());
    renderer.set_content_validation_limits(Dx11ContentLimits {
        pause_before_node: Some((0, yellow_gate.clone())),
        ..Default::default()
    });
    let yellow = make([1., 1., 0., 1.]);
    let image = renderer
        .render_scene_to_image(&yellow, size(DevicePixels(16), DevicePixels(8)))
        .unwrap();
    assert!(image.pixels().all(|p| p.0 == [0, 0, 255, 255]));
    assert!(yellow_gate.wait_until_entered(Duration::from_secs(20)));
    yellow_gate.release();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    renderer.prepare_content_snapshot(&yellow).unwrap();
    renderer.set_content_validation_limits(Default::default());
    for i in 0..60 {
        let color = [i as f32 / 59., 0., 1., 1.];
        let scene = make(color);
        // Submit while retaining a valid previous image, then require exact pixels.
        renderer
            .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(8)))
            .unwrap();
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        renderer.prepare_content_snapshot(&scene).unwrap();
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(8)))
            .unwrap();
        assert!(
            image
                .pixels()
                .all(|p| p.0[0].abs_diff((color[0] * 255.).round() as u8) <= 1
                    && p.0[2] == 255
                    && p.0[3] == 255)
        );
        assert!(renderer.content_presentation_stats().cached_images <= 2);
        assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    }
    let mut duplicate = Scene::default();
    duplicate.insert_primitive(red.custom_draws[0].clone());
    duplicate.insert_primitive(red.custom_draws[0].clone());
    duplicate.finish();
    assert_eq!(
        renderer
            .prepare_content_snapshot(&duplicate)
            .unwrap_err()
            .downcast_ref::<ContentError>()
            .unwrap()
            .kind,
        ContentErrorKind::InvalidDescriptor
    );
    renderer
        .prepare_content_snapshot(&Scene::default())
        .unwrap();
    renderer
        .render_scene_to_image(&Scene::default(), size(DevicePixels(16), DevicePixels(8)))
        .unwrap();
    let start = std::time::Instant::now();
    while renderer.content_presentation_stats().retained_gpu_bytes != 0 {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(1));
    }
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.cached_images, 0);
    assert_eq!(stats.retained_slots, 0);
    assert_eq!(stats.retained_gpu_bytes, 0);
}

#[test]
fn portable_compositor_obeys_color_alpha_and_coverage_metadata() {
    use gpui::{
        Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, ScaledPixels,
        Scene, point,
    };
    #[derive(Debug)]
    struct Pixels(PixelFormat, ImageInterpretation, Vec<u8>);
    impl ContentSource for Pixels {
        fn describe(
            &self,
            cx: &mut ContentBuilder,
            r: &ContentRequest,
        ) -> Result<ImageRef, ContentError> {
            cx.upload_image(
                ImageDescriptor::new(r.extent(), self.0, self.1, 1)?,
                self.2
                    .repeat((r.extent().width() * r.extent().height()) as usize)
                    .into(),
            )
        }
    }
    let mut renderer = gpui_windows::Dx11HeadlessRenderer::new().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = tx.send(());
    }));
    let half_bytes = |r: u16| {
        [r, 0, 0, 0x3800]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    for (format, color, alpha, bytes, expected) in [
        (
            PixelFormat::Rgba8,
            ColorEncoding::Srgb,
            AlphaMode::Straight,
            vec![255, 0, 0, 128],
            [64, 0, 0, 255],
        ),
        (
            PixelFormat::Rgba8,
            ColorEncoding::Srgb,
            AlphaMode::Premultiplied,
            vec![128, 0, 0, 128],
            [64, 0, 0, 255],
        ),
        (
            PixelFormat::Rgba8,
            ColorEncoding::Srgb,
            AlphaMode::Opaque,
            vec![255, 0, 0, 0],
            [128, 0, 0, 255],
        ),
        (
            PixelFormat::Rgba16Float,
            ColorEncoding::LinearSrgb,
            AlphaMode::Straight,
            half_bytes(0x3400),
            [34, 0, 0, 255],
        ),
        (
            PixelFormat::Rgba16Float,
            ColorEncoding::LinearSrgb,
            AlphaMode::Premultiplied,
            half_bytes(0x3000),
            [34, 0, 0, 255],
        ),
        (
            PixelFormat::R8,
            ColorEncoding::Coverage,
            AlphaMode::Opaque,
            vec![128],
            [64, 64, 64, 255],
        ),
    ] {
        let content = Content::new(Pixels(format, ImageInterpretation { color, alpha }, bytes));
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(16.), ScaledPixels(8.)),
        );
        let mut scene = Scene::default();
        scene.insert_primitive(gpui::Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: gpui::black().into(),
            ..Default::default()
        });
        scene.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Default::default(),
            opacity: 0.5,
            coverage: None,
            content: ContentDraw::new(content).into_scene_content(),
        });
        scene.finish();
        match renderer.prepare_content_snapshot(&scene) {
            Ok(()) => {}
            Err(e)
                if e.downcast_ref::<ContentError>()
                    .is_some_and(|e| e.kind == ContentErrorKind::Pending) =>
            {
                rx.recv_timeout(Duration::from_secs(20)).unwrap();
            }
            Err(e) => panic!("{format:?} {color:?} {alpha:?}: {e:#}"),
        }
        renderer.prepare_content_snapshot(&scene).unwrap();
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(8)))
            .unwrap();
        assert!(
            image.pixels().all(|p| p
                .0
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 1)),
            "{format:?} {color:?} {alpha:?}: {:?}",
            image.get_pixel(8, 4)
        );
        assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    }
}
