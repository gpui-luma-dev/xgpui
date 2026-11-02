use gpui::content::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn pixels() -> ImagePixels {
    ImagePixels::new(
        ImageDescriptor::new(
            ImageExtent::new(1, 1).unwrap(),
            PixelFormat::Rgba8,
            ImageInterpretation {
                color: ColorEncoding::Srgb,
                alpha: AlphaMode::Straight,
            },
            1,
        )
        .unwrap(),
        Arc::from([255, 0, 0, 255]),
        4,
    )
    .unwrap()
}
#[test]
fn completion_notifies_live_consumers_outside_lock_and_preserves_identity() {
    let (asset, done) = ImageAsset::pending();
    let clone = asset.clone();
    assert_eq!(asset.id(), clone.id());
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let observed = clone.clone();
    let (subscription, version, state) = asset.subscribe(Arc::new(move || {
        assert!(matches!(observed.observe().1, ImageReadiness::Available(_)));
        count.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(version, 0);
    assert!(matches!(state, ImageReadiness::Pending));
    done.resolve(Ok(pixels()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(asset.observe().0, 1);
    drop(subscription);
    let (_, version, state) = asset.subscribe(Arc::new(|| {
        panic!("terminal observation must not schedule another wake")
    }));
    assert_eq!(version, 1);
    assert!(matches!(state, ImageReadiness::Available(_)));
}
#[test]
fn unsubscribing_and_dropping_clones_do_not_cancel_other_consumers() {
    let (asset, done) = ImageAsset::pending();
    let (cancelled, _, _) = asset.subscribe(Arc::new(|| panic!("cancelled subscriber")));
    drop(cancelled);
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let (_live, _, _) = asset.subscribe(Arc::new(move || {
        c.fetch_add(1, Ordering::SeqCst);
    }));
    drop(asset.clone());
    done.resolve(Ok(pixels()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
#[test]
fn abandoned_endpoint_is_terminal_and_validation_rejects_bad_payloads() {
    let (asset, done) = ImageAsset::pending();
    drop(done);
    match asset.observe().1 {
        ImageReadiness::Failed(e) => assert_eq!(e.kind, ContentErrorKind::Cancelled),
        _ => panic!("expected terminal cancellation"),
    }
    let p = pixels();
    assert_eq!(
        ImagePixels::new(p.descriptor().clone(), Arc::from([0u8; 3]), 4)
            .unwrap_err()
            .kind,
        ContentErrorKind::InvalidDescriptor
    );
    assert_eq!(
        ImagePixels::new(p.descriptor().clone(), p.bytes().clone(), 3)
            .unwrap_err()
            .kind,
        ContentErrorKind::ResourceLimit
    );
}
#[test]
fn subscribe_completion_race_always_observes_or_receives_completion() {
    for _ in 0..100 {
        let (asset, done) = ImageAsset::pending();
        let worker = std::thread::spawn(move || done.resolve(Ok(pixels())));
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let (_subscription, _, state) = asset.subscribe(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        worker.join().unwrap();
        assert!(matches!(state, ImageReadiness::Available(_)) || count.load(Ordering::SeqCst) == 1);
    }
}

#[test]
fn tracked_description_survives_pending_then_uploads_natural_extent() {
    let (asset, done) = ImageAsset::pending();
    let roots = [ContentRoot {
        content: Content::image(asset),
        request: ContentRequest::new(
            gpui::size(gpui::px(512.), gpui::px(512.)),
            ImageExtent::new(512, 512).unwrap(),
            1.,
        )
        .unwrap(),
    }];
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let (result, dependencies) = ContentGraph::describe_tracked(
        &roots,
        ContentCapabilities::default(),
        GraphLimits::default(),
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
        }),
    );
    assert_eq!(result.unwrap_err().kind, ContentErrorKind::Pending);
    assert!(dependencies.is_pending());
    assert!(!dependencies.changed());
    done.resolve(Ok(pixels()));
    assert!(dependencies.changed());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let (result, ready) = ContentGraph::describe_tracked(
        &roots,
        ContentCapabilities::default(),
        GraphLimits::default(),
        Arc::new(|| panic!("ready asset must not wake")),
    );
    let graph = result.unwrap();
    assert!(!ready.is_pending());
    let root = graph.roots()[0];
    assert_eq!(
        graph.nodes()[root.node()].outputs()[root.output()],
        ResourceDescriptor::Image(pixels().descriptor().clone())
    );
    assert_eq!(graph.payload_bytes(), 4);
}

#[test]
fn dropping_failed_description_dependencies_unsubscribes_pending_assets() {
    let (asset, done) = ImageAsset::pending();
    let roots = [ContentRoot {
        content: Content::image(asset),
        request: ContentRequest::new(
            gpui::size(gpui::px(1.), gpui::px(1.)),
            ImageExtent::new(1, 1).unwrap(),
            1.,
        )
        .unwrap(),
    }];
    let (result, dependencies) = ContentGraph::describe_tracked(
        &roots,
        ContentCapabilities::default(),
        GraphLimits::default(),
        Arc::new(|| panic!("removed consumer must not wake")),
    );
    assert_eq!(result.unwrap_err().kind, ContentErrorKind::Pending);
    drop(dependencies);
    done.resolve(Ok(pixels()));
}
