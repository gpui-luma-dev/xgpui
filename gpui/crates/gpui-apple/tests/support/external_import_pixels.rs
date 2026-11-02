use gpui::{
    Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, ScaledPixels, Scene,
    content::*, point, px, size,
};
use gpui_apple::{
    content_executor::*, custom_draw::core_video::test_frame, metal_renderer::MetalHeadlessRenderer,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
fn request() -> ContentRequest {
    ContentRequest::new(
        size(px(64.), px(32.)),
        ImageExtent::new(64, 32).unwrap(),
        1.,
    )
    .unwrap()
}
fn graph(executor: &MetalContentExecutor, content: Content) -> Arc<ContentGraph> {
    Arc::new(
        ContentGraph::describe(
            &[ContentRoot {
                content,
                request: request(),
            }],
            executor.capabilities().clone(),
            GraphLimits::default(),
        )
        .unwrap(),
    )
}
#[test]
fn bgra_import_is_source_sized_without_upload_or_new_image_storage() {
    let native = test_frame(16, 8, 0).unwrap();
    let executor = MetalContentExecutor::new(MetalContentOptions::default()).unwrap();
    let graph = graph(
        &executor,
        Content::external(native.external_image().unwrap()),
    );
    assert_eq!(graph.nodes().len(), 1);
    assert!(matches!(
        graph.nodes()[0].operation(),
        ContentOperation::ExternalImage(_)
    ));
    assert_eq!(graph.nodes()[0].outputs()[0].byte_len(), 16 * 8 * 4);
    let frame = executor
        .submit(graph, MetalContentLimits::default())
        .unwrap()
        .wait_for_readback(Duration::from_secs(10))
        .unwrap();
    assert_eq!(frame.stats.external_imports, 1);
    assert_eq!(frame.stats.image_allocations, 0);
    assert_eq!(
        native.import_count(),
        0,
        "portable executor must not populate the native producer cache"
    );
    // Direct storage readback stays BGRA; compositor presentation swizzles it.
    assert_eq!(&frame.images[0].levels[0][0..4], &[32, 64, 224, 255]);
}
fn scene(image: ExternalImage) -> Scene {
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(4.), ScaledPixels(4.)),
        size(ScaledPixels(32.), ScaledPixels(16.)),
    );
    let draw = ContentDraw::new(Content::external(image))
        .color_output(ContentColorOutput::SrgbPreview)
        .color_transform(
            ImageColorTransform::encoded_srgb([
                [0., 0., 1., 0.],
                [0., 1., 0., 0.],
                [1., 0., 0., 0.],
            ])
            .unwrap(),
        );
    scene.insert_primitive(CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Default::default(),
        opacity: 0.5,
        coverage: None,
        content: draw.into_scene_content(),
    });
    scene.finish();
    scene
}
fn ready(renderer: &mut MetalHeadlessRenderer, scene: &Scene) -> image::RgbaImage {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match renderer.prepare_content_snapshot(scene) {
            Ok(()) => {
                return renderer
                    .render_scene_to_image(scene, size(DevicePixels(40), DevicePixels(24)))
                    .unwrap();
            }
            Err(e)
                if e.downcast_ref::<ContentError>()
                    .is_some_and(|e| e.kind == ContentErrorKind::Pending) => {}
            Err(e) => panic!("{e}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn window_presentation_and_adjustments_reuse_import_without_producer_readback() {
    let native = test_frame(16, 8, 0).unwrap();
    let descriptor = native.external_image().unwrap().descriptor().clone();
    let releases = Arc::new(AtomicUsize::new(0));
    let count = releases.clone();
    let image = ExternalImage::ready(descriptor, Arc::new(native.clone()), move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let scene = scene(image);
    let mut renderer = MetalHeadlessRenderer::new();
    let first = ready(&mut renderer, &scene);
    let p = first.get_pixel(6, 6).0;
    assert!(
        p[0].abs_diff(16) <= 1 && p[1].abs_diff(32) <= 1 && p[2].abs_diff(112) <= 1,
        "swizzled adjusted BGRA with half opacity: {p:?}"
    );
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.execution.external_imports, 1);
    assert_eq!(stats.execution.readbacks, 0);
    assert_eq!(stats.execution.image_allocations, 0);
    let replay = ready(&mut renderer, &scene);
    assert_eq!(first, replay);
    assert_eq!(
        renderer.content_presentation_stats().requests,
        stats.requests
    );
    // No hidden old-frame cache after removal; exported native uses may retire asynchronously.
    renderer
        .render_scene_to_image(&Scene::default(), size(DevicePixels(40), DevicePixels(24)))
        .unwrap();
    assert_eq!(renderer.content_presentation_stats().cached_images, 0);
    drop(scene);
    let deadline = Instant::now() + Duration::from_secs(5);
    while releases.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "removed external frame remained pinned in a hidden cache"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}
#[derive(Debug)]
struct CopyExternal(Content);
impl ContentSource for CopyExternal {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> Result<ImageRef, ContentError> {
        let source = cx.source(&self.0, request)?;
        cx.copy_image(source)
    }
}
#[test]
fn paused_dependent_copy_pins_external_lease_through_native_retirement() {
    let native = test_frame(16, 8, 0).unwrap();
    let descriptor = native.external_image().unwrap().descriptor().clone();
    let released = Arc::new(AtomicUsize::new(0));
    let count = released.clone();
    let external = ExternalImage::ready(descriptor, Arc::new(native), move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let executor = MetalContentExecutor::new(MetalContentOptions::default()).unwrap();
    let graph = graph(
        &executor,
        Content::new(CopyExternal(Content::external(external))),
    );
    struct Release(Arc<MetalContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    let gate = Release(Arc::new(MetalContentTestGate::default()));
    let job = executor
        .submit(
            graph,
            MetalContentLimits {
                pause_before_node: Some((1, gate.0.clone())),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    assert_eq!(released.load(Ordering::SeqCst), 0);
    gate.0.release();
    let frame = job.wait_for_readback(Duration::from_secs(10)).unwrap();
    assert_eq!(frame.stats.external_imports, 1);
    assert_eq!(frame.stats.image_allocations, 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while released.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(released.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_storage_and_foreign_backings_are_rejected_before_import() {
    let executor = MetalContentExecutor::new(MetalContentOptions::default()).unwrap();
    let native = test_frame(16, 8, 0).unwrap();
    let descriptor = native.external_image().unwrap().descriptor().clone();
    let wrong = ImageDescriptor::new(
        ImageExtent::new(8, 8).unwrap(),
        descriptor.format(),
        descriptor.interpretation(),
        1,
    )
    .unwrap();
    for (descriptor, backing, kind) in [
        (
            wrong,
            Arc::new(native) as Arc<dyn std::any::Any + Send + Sync>,
            ContentErrorKind::InvalidDescriptor,
        ),
        (
            descriptor,
            Arc::new(()) as Arc<dyn std::any::Any + Send + Sync>,
            ContentErrorKind::UnsupportedCapability,
        ),
    ] {
        let image = ExternalImage::ready(descriptor, backing, || {});
        let job = executor
            .submit(
                graph(&executor, Content::external(image)),
                MetalContentLimits::default(),
            )
            .unwrap();
        assert_eq!(
            job.wait_for_readback(Duration::from_secs(10))
                .unwrap_err()
                .kind,
            kind
        );
    }
}
#[test]
fn cancelling_a_paused_import_retires_once_without_sampling() {
    let native = test_frame(16, 8, 0).unwrap();
    let descriptor = native.external_image().unwrap().descriptor().clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let image = ExternalImage::ready(descriptor, Arc::new(native), move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let executor = MetalContentExecutor::new(MetalContentOptions::default()).unwrap();
    struct Release(Arc<MetalContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    let gate = Release(Arc::new(MetalContentTestGate::default()));
    let job = executor
        .submit(
            graph(&executor, Content::external(image)),
            MetalContentLimits {
                pause_before_node: Some((0, gate.0.clone())),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    job.cancel();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    gate.0.release();
    assert_eq!(
        job.wait_for_readback(Duration::from_secs(10))
            .unwrap_err()
            .kind,
        ContentErrorKind::Cancelled
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while calls.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failure_after_a_submitted_copy_retires_the_external_prefix() {
    let native = test_frame(16, 8, 0).unwrap();
    let descriptor = native.external_image().unwrap().descriptor().clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let external = ExternalImage::ready(descriptor, Arc::new(native), move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let executor = MetalContentExecutor::new(MetalContentOptions::default()).unwrap();
    let graph = graph(
        &executor,
        Content::new(CopyExternal(Content::new(CopyExternal(Content::external(
            external,
        ))))),
    );
    let job = executor
        .submit(
            graph,
            MetalContentLimits {
                fail_before_node: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        job.wait_for_readback(Duration::from_secs(10))
            .unwrap_err()
            .kind,
        ContentErrorKind::ExecutionFailure
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while calls.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn pending_acquisition_defers_import_and_retries_the_same_scene() {
    let native = test_frame(16, 8, 0).unwrap();
    let descriptor = native.external_image().unwrap().descriptor().clone();
    let (image, done) = ExternalImage::pending(descriptor, Arc::new(native), || {});
    let scene = scene(image);
    let mut renderer = MetalHeadlessRenderer::new();
    let error = renderer.prepare_content_snapshot(&scene).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ContentError>().unwrap().kind,
        ContentErrorKind::Pending
    );
    assert_eq!(
        renderer.content_presentation_stats().requests,
        0,
        "pending acquisition must not submit a GPU import"
    );
    done.resolve(Ok(()));
    let image = ready(&mut renderer, &scene);
    assert!(image.get_pixel(6, 6).0[2].abs_diff(112) <= 1);
    assert_eq!(
        renderer
            .content_presentation_stats()
            .execution
            .external_imports,
        1
    );
}
