//! CPU-only external acquisition/ownership contracts. These do not import a GPU surface.
use gpui::content::*;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};
fn descriptor() -> ImageDescriptor {
    ImageDescriptor::new(
        ImageExtent::new(16, 9).unwrap(),
        PixelFormat::Bgra8,
        ImageInterpretation {
            color: ColorEncoding::Srgb,
            alpha: AlphaMode::Straight,
        },
        1,
    )
    .unwrap()
}
#[test]
fn acquisition_is_nonblocking_and_notifies_outside_the_lock() {
    let (image, done) = ExternalImage::pending(descriptor(), Arc::new(42_u32), || {});
    let clone = image.clone();
    assert_eq!(image.id(), clone.id());
    assert_eq!(
        image.acquire().err().unwrap().kind,
        ContentErrorKind::Pending
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = image.clone();
    let count = calls.clone();
    let (_subscription, version, state) = image.subscribe(Arc::new(move || {
        assert_eq!(observed.observe(), (1, ExternalImageReadiness::Ready));
        assert_eq!(observed.acquire().unwrap().backing::<u32>(), Some(&42));
        count.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(version, 0);
    assert_eq!(state, ExternalImageReadiness::Pending);
    done.resolve(Ok(()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(clone.observe(), (1, ExternalImageReadiness::Ready));
    let lease = image.acquire().unwrap();
    assert_eq!(lease.descriptor(), &descriptor());
    assert!(lease.backing::<String>().is_none());
}
#[test]
fn snapshots_and_every_acquired_use_pin_release_and_backing() {
    let released = Arc::new(AtomicUsize::new(0));
    let count = released.clone();
    let backing = Arc::new(42_u32);
    let weak = Arc::downgrade(&backing);
    let check = weak.clone();
    let image = ExternalImage::ready(descriptor(), backing, move || {
        assert!(
            check.upgrade().is_some(),
            "backing must survive its release callback"
        );
        count.fetch_add(1, Ordering::SeqCst);
    });
    let snapshot = image.clone();
    let usage = image.acquire().unwrap();
    let submitted = usage.clone();
    drop(image);
    drop(usage);
    drop(snapshot);
    assert_eq!(released.load(Ordering::SeqCst), 0);
    assert!(weak.upgrade().is_some());
    // Model the owner transferred to a native completion callback.
    drop(submitted);
    assert_eq!(released.load(Ordering::SeqCst), 1);
    assert!(weak.upgrade().is_none());
}
#[test]
fn acquisition_endpoint_keeps_owner_after_all_consumers_retire() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let (image, completion) = ExternalImage::pending(descriptor(), Arc::new(()), move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    drop(image);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    completion.resolve(Ok(()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn failure_and_abandonment_never_allow_a_native_use() {
    for abandoned in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (image, completion) = ExternalImage::pending(descriptor(), Arc::new(()), move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        if abandoned {
            drop(completion);
        } else {
            completion.resolve(Err(ContentError::new(
                ContentErrorKind::UnsupportedCapability,
                "unsupported native synchronization",
            )));
        }
        let expected = if abandoned {
            ContentErrorKind::Cancelled
        } else {
            ContentErrorKind::UnsupportedCapability
        };
        assert_eq!(image.acquire().err().unwrap().kind, expected);
        assert_eq!(image.observe().0, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(image);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn subscription_cancellation_does_not_cancel_shared_acquisition_or_pin_backing() {
    let calls = Arc::new(AtomicUsize::new(0));
    let wakes = calls.clone();
    let backing = Arc::new(());
    let weak = Arc::downgrade(&backing);
    let (image, completion) = ExternalImage::pending(descriptor(), backing, || {});
    let (subscription, _, _) = image.subscribe(Arc::new(move || {
        wakes.fetch_add(1, Ordering::SeqCst);
    }));
    drop(subscription);
    completion.resolve(Ok(()));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let (subscription, _, state) =
        image.subscribe(Arc::new(|| panic!("terminal subscription must not wake")));
    assert_eq!(state, ExternalImageReadiness::Ready);
    drop(image);
    assert!(weak.upgrade().is_none());
    drop(subscription);
}
#[test]
fn allocation_address_is_not_frame_identity() {
    let backing = Arc::new(17_u32);
    let first = ExternalImage::ready(descriptor(), backing.clone(), || {});
    let next = ExternalImage::ready(descriptor(), backing, || {});
    assert_ne!(first.id(), next.id());
    assert_eq!(
        first.acquire().unwrap().backing::<u32>(),
        next.acquire().unwrap().backing::<u32>()
    );
}
#[test]
fn terminal_transition_cannot_race_past_subscription() {
    for _ in 0..64 {
        let (image, completion) = ExternalImage::pending(descriptor(), Arc::new(()), || {});
        let barrier = Arc::new(Barrier::new(2));
        let worker = barrier.clone();
        let thread = std::thread::spawn(move || {
            worker.wait();
            completion.resolve(Ok(()));
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        barrier.wait();
        let (_subscription, _, state) = image.subscribe(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }));
        thread.join().unwrap();
        assert_eq!(image.observe(), (1, ExternalImageReadiness::Ready));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            usize::from(state == ExternalImageReadiness::Pending)
        );
    }
}
