use gpui::{DevicePixels, content::*, px, size};
use std::{
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

type Result<T> = std::result::Result<T, ContentError>;
type Describe = dyn Fn(&mut ContentBuilder, &ContentRequest) -> Result<ImageRef> + Send + Sync;
struct Source(Box<Describe>);
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TestSource")
    }
}
impl ContentSource for Source {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest) -> Result<ImageRef> {
        (self.0)(cx, request)
    }
}
fn source(
    f: impl Fn(&mut ContentBuilder, &ContentRequest) -> Result<ImageRef> + Send + Sync + 'static,
) -> Content {
    Content::new(Source(Box::new(f)))
}
fn extent() -> ImageExtent {
    ImageExtent::new(4, 4).unwrap()
}
fn request() -> ContentRequest {
    ContentRequest::new(size(px(4.), px(4.)), extent(), 1.).unwrap()
}
fn red() -> ColorValue {
    ColorValue::srgb([1., 0., 0., 1.]).unwrap()
}
fn root(content: Content) -> ContentRoot {
    ContentRoot {
        content,
        request: request(),
    }
}
fn graph(roots: &[ContentRoot]) -> Arc<ContentGraph> {
    Arc::new(
        ContentGraph::describe(
            roots,
            ContentCapabilities::default(),
            GraphLimits::default(),
        )
        .unwrap(),
    )
}
fn chain(copies: usize) -> Content {
    source(move |cx, request| {
        let mut image = cx.solid(request.extent(), red())?;
        for _ in 0..copies {
            image = cx.copy_image(image)?;
        }
        Ok(image)
    })
}
fn tracker(roots: &[ContentRoot], bytes: u64, recordings: usize) -> ExecutionTracker {
    let graph = graph(roots);
    let charges = graph.nodes().iter().map(ContentNode::byte_len).collect();
    ExecutionPlan::new(
        graph,
        charges,
        ExecutionLimits {
            max_live_bytes: bytes,
            max_recording_nodes: recordings,
        },
    )
    .unwrap()
    .start()
}

// CPU accounting fixtures have no native work. Teardown tests below separately
// retain the returned leases like a real executor's GPU completion callbacks.
fn submit(tracker: &mut ExecutionTracker, id: usize) {
    drop(tracker.submitted(id).unwrap());
}

#[test]
fn descriptors_reject_overflow_invalid_mips_and_color_association() {
    assert!(ImageExtent::new(0, 1).is_err());
    assert!(ImageExtent::try_from(size(DevicePixels(-1), DevicePixels(1))).is_err());
    assert!(ColorValue::srgb([f32::NAN, 0., 0., 1.]).is_err());
    assert!(
        ColorValue::new(
            [1., 0., 0., 0.5],
            ImageInterpretation {
                color: ColorEncoding::LinearP3,
                alpha: AlphaMode::Premultiplied
            }
        )
        .is_err()
    );
    let color = ImageInterpretation {
        color: ColorEncoding::Srgb,
        alpha: AlphaMode::Straight,
    };
    assert!(ImageDescriptor::new(extent(), PixelFormat::Rgba8, color, 4).is_err());
    assert!(ImageDescriptor::new(extent(), PixelFormat::R8, color, 1).is_err());
    assert!(
        ImageDescriptor::new(
            ImageExtent::new(i32::MAX as u32, i32::MAX as u32).unwrap(),
            PixelFormat::Rgba16Float,
            color,
            1
        )
        .is_err()
    );
    let mip = ImageDescriptor::new(
        ImageExtent::new(3, 1).unwrap(),
        PixelFormat::Rgba8,
        color,
        2,
    )
    .unwrap();
    assert_eq!(mip.byte_len(), 16);
}

#[test]
fn canonical_colors_and_inline_values_share_collision_checked_realizations() {
    let a = Content::solid(ColorValue::srgb([0., 1., 0., 1.]).unwrap());
    let b = Content::solid(ColorValue::srgb([-0., 1., 0., 1.]).unwrap());
    assert_ne!(a.id(), b.id());
    assert_eq!(a.id(), a.clone().id());
    let g = graph(&[root(a), root(b.labeled("another label"))]);
    assert_eq!(g.nodes().len(), 1);
    assert_eq!(g.roots().len(), 2);
    assert_eq!(g.roots()[0], g.roots()[1]);
    #[derive(Default)]
    struct ConstantHash;
    impl Hasher for ConstantHash {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, _: &[u8]) {}
    }
    let other = graph(&[root(Content::solid(red()))]);
    let mut map = HashMap::<_, _, BuildHasherDefault<ConstantHash>>::default();
    map.insert(g.nodes()[0].operation_key().clone(), "green");
    map.insert(other.nodes()[0].operation_key().clone(), "red");
    assert_eq!(map.len(), 2);
    assert_eq!(map[g.nodes()[0].operation_key()], "green");
}

#[test]
fn structural_keys_include_extent_and_dependency_recipe_across_graphs() {
    let a = graph(&[root(chain(1))]);
    let b = graph(&[root(chain(1))]);
    assert_eq!(a.nodes()[1].operation_key(), b.nodes()[1].operation_key());
    assert_ne!(a.nodes()[0].operation_key(), a.nodes()[1].operation_key());
    let mut larger = root(chain(1));
    larger.request =
        ContentRequest::new(size(px(8.), px(4.)), ImageExtent::new(8, 4).unwrap(), 1.).unwrap();
    let c = graph(&[larger]);
    assert_ne!(a.nodes()[1].operation_key(), c.nodes()[1].operation_key());
}

#[test]
fn nested_shared_sources_describe_once_per_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let shared = source(move |cx, request| {
        count.fetch_add(1, Ordering::Relaxed);
        cx.solid(request.extent(), red())
    });
    let wrapper = source(move |cx, request| {
        let first = cx.source(&shared, request)?;
        let second = cx.source(&shared, request)?;
        assert_eq!(first, second);
        cx.copy_image(first)
    });
    let g = graph(&[root(wrapper.clone()), root(wrapper)]);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(g.nodes().len(), 2);
}

#[test]
fn foreign_references_poison_even_when_the_source_catches_the_error() {
    let saved = Arc::new(Mutex::new(None));
    let slot = saved.clone();
    graph(&[root(source(move |cx, request| {
        let image = cx.solid(request.extent(), red())?;
        *slot.lock().unwrap() = Some(image);
        Ok(image)
    }))]);
    let stale = saved.lock().unwrap().unwrap();
    let bad = source(move |cx, request| {
        assert_eq!(
            cx.copy_image(stale).unwrap_err().kind,
            ContentErrorKind::InvalidReference
        );
        cx.solid(request.extent(), red())
    });
    let error = ContentGraph::describe(
        &[root(bad)],
        ContentCapabilities::default(),
        GraphLimits::default(),
    )
    .unwrap_err();
    assert_eq!(error.kind, ContentErrorKind::InvalidReference);
    let bad_return = source(move |_, _| Ok(stale));
    assert_eq!(
        ContentGraph::describe(
            &[root(bad_return)],
            ContentCapabilities::default(),
            GraphLimits::default()
        )
        .unwrap_err()
        .kind,
        ContentErrorKind::InvalidReference
    );
}

#[test]
fn cycles_and_bounded_description_fail_without_native_work() {
    let slot = Arc::new(Mutex::new(None::<Content>));
    let recurse = slot.clone();
    let content = source(move |cx, request| {
        let content = recurse.lock().unwrap().as_ref().unwrap().clone();
        cx.source(&content, request)
    });
    *slot.lock().unwrap() = Some(content.clone());
    let result = ContentGraph::describe(
        &[root(content)],
        ContentCapabilities::default(),
        GraphLimits::default(),
    );
    slot.lock().unwrap().take(); // Break the deliberately constructed CPU ownership cycle.
    assert_eq!(result.unwrap_err().kind, ContentErrorKind::Cycle);
    let limits = GraphLimits {
        max_depth: 3,
        ..GraphLimits::default()
    };
    assert_eq!(
        ContentGraph::describe(&[root(chain(3))], ContentCapabilities::default(), limits)
            .unwrap_err()
            .kind,
        ContentErrorKind::ResourceLimit
    );
}

#[test]
fn uploads_are_source_sized_and_payloads_are_deduplicated() {
    let descriptor = ImageDescriptor::new(
        ImageExtent::new(2, 1).unwrap(),
        PixelFormat::Rgba8,
        ImageInterpretation {
            color: ColorEncoding::Srgb,
            alpha: AlphaMode::Straight,
        },
        1,
    )
    .unwrap();
    let content = source(move |cx, _| cx.upload_image(descriptor.clone(), Arc::from([255u8; 8])));
    let g = graph(&[root(content.clone()), root(content)]);
    assert_eq!(g.payload_bytes(), 8);
    let ResourceDescriptor::Image(output) = &g.nodes()[0].outputs()[0] else {
        panic!()
    };
    assert_eq!(output.extent(), ImageExtent::new(2, 1).unwrap());
    let limits = GraphLimits {
        max_payload_bytes: 7,
        ..GraphLimits::default()
    };
    let descriptor = output.clone();
    let content = source(move |cx, _| cx.upload_image(descriptor.clone(), Arc::from([0u8; 8])));
    assert_eq!(
        ContentGraph::describe(&[root(content)], ContentCapabilities::default(), limits)
            .unwrap_err()
            .kind,
        ContentErrorKind::ResourceLimit
    );
}

#[test]
fn unused_operations_are_pruned_and_copy_dependencies_are_remapped() {
    let content = source(|cx, request| {
        cx.solid(request.extent(), ColorValue::srgb([0., 1., 0., 1.])?)?;
        let used = cx.solid(request.extent(), red())?;
        cx.copy_image(used)
    });
    let g = graph(&[root(content)]);
    assert_eq!(g.nodes().len(), 2);
    assert_eq!(g.nodes()[1].dependencies(), &[0]);
    let ContentOperation::CopyImage { source } = g.nodes()[1].operation() else {
        panic!()
    };
    assert_eq!((source.node(), source.output()), (0, 0));
}

#[test]
fn capability_probe_can_choose_a_supported_path_without_poisoning() {
    let content = source(|cx, request| {
        assert!(
            !cx.capabilities()
                .image_formats
                .contains(&PixelFormat::Rgba16Float)
        );
        cx.solid(request.extent(), red())
    });
    let caps = ContentCapabilities {
        image_formats: vec![PixelFormat::Rgba8],
        ..ContentCapabilities::default()
    };
    ContentGraph::describe(&[root(content)], caps.clone(), GraphLimits::default()).unwrap();
    let unsupported = Content::solid(ColorValue::linear_p3([1., 0., 0., 1.]).unwrap());
    assert_eq!(
        ContentGraph::describe(&[root(unsupported)], caps, GraphLimits::default())
            .unwrap_err()
            .kind,
        ContentErrorKind::UnsupportedCapability
    );
}

#[test]
fn typed_buffer_upload_copy_and_overflowing_ranges_are_checked() {
    assert!(BufferLayout::new(12, 8).is_err());
    let layout = BufferLayout::new(16, 16).unwrap();
    assert!(BufferDescriptor::new(17, layout, BufferUsage::VERTEX).is_err());
    let content = source(move |cx, request| {
        let d = BufferDescriptor::new(64, layout, BufferUsage::COPY_SOURCE | BufferUsage::VERTEX)?;
        let buffer = cx.upload_buffer(d, Arc::from([0u8; 64]))?;
        let slice = BufferSlice {
            buffer,
            offset: 16,
            byte_len: 32,
        };
        cx.validate_buffer_slice(slice, BufferUsage::VERTEX)?;
        let out = BufferDescriptor::new(
            32,
            layout,
            BufferUsage::COPY_DESTINATION | BufferUsage::STORAGE,
        )?;
        let copied = cx.copy_buffer(slice, out)?;
        cx.validate_buffer_slice(
            BufferSlice {
                buffer: copied,
                offset: 0,
                byte_len: 32,
            },
            BufferUsage::STORAGE,
        )?;
        cx.solid(request.extent(), red())
    });
    graph(&[root(content)]);
    for (offset, byte_len) in [(1, 16), (0, 15), (64, 16), (u64::MAX - 15, 32)] {
        let bad = source(move |cx, request| {
            let d = BufferDescriptor::new(64, layout, BufferUsage::VERTEX)?;
            let buffer = cx.zeroed_buffer(d)?;
            let _ = cx.validate_buffer_slice(
                BufferSlice {
                    buffer,
                    offset,
                    byte_len,
                },
                BufferUsage::VERTEX,
            );
            cx.solid(request.extent(), red())
        });
        assert_eq!(
            ContentGraph::describe(
                &[root(bad)],
                ContentCapabilities::default(),
                GraphLimits::default()
            )
            .unwrap_err()
            .kind,
            ContentErrorKind::InvalidDescriptor
        );
    }
}

#[test]
fn plan_rejects_impossible_live_sets_and_native_charge_underestimates() {
    let g = graph(&[root(chain(2))]); // Three 64-byte versions; serial peak is 128.
    let limits = ExecutionLimits {
        max_live_bytes: 128,
        max_recording_nodes: 1,
    };
    assert_eq!(
        ExecutionPlan::new(g.clone(), vec![64; 3], limits)
            .unwrap()
            .peak_bytes(),
        128
    );
    assert_eq!(
        ExecutionPlan::new(
            g.clone(),
            vec![64; 3],
            ExecutionLimits {
                max_live_bytes: 127,
                ..limits
            }
        )
        .unwrap_err()
        .kind,
        ContentErrorKind::ResourceLimit
    );
    assert!(ExecutionPlan::new(g, vec![63; 3], limits).is_err());
}

#[test]
fn bounded_recording_advances_submission_before_further_admission() {
    let roots: Vec<_> = (0..192)
        .map(|i| {
            root(Content::solid(
                ColorValue::srgb([i as f32 / 192., 0., 0., 1.]).unwrap(),
            ))
        })
        .collect();
    let mut tracker = tracker(&roots, 192 * 64, 1);
    assert_eq!(tracker.try_admit(0).unwrap(), Admission::Admitted);
    assert_eq!(
        tracker.try_admit(1).unwrap(),
        Admission::Deferred(AdmissionPressure::RecordingAllowance)
    );
    assert_eq!(tracker.live_bytes(), 64);
    submit(&mut tracker, 0);
    for id in 1..192 {
        assert_eq!(tracker.try_admit(id).unwrap(), Admission::Admitted);
        submit(&mut tracker, id);
    }
    tracker.release_roots();
    for id in 0..192 {
        tracker.completed(id, true).unwrap();
    }
    assert_eq!(tracker.live_bytes(), 0);
    assert!(tracker.is_quiescent());
}

#[test]
fn dependent_admission_and_live_pressure_do_not_partially_reserve() {
    let mut tracker = tracker(&[root(chain(2))], 128, 2);
    assert_eq!(
        tracker.try_admit(1).unwrap(),
        Admission::Deferred(AdmissionPressure::Dependencies)
    );
    assert_eq!(tracker.live_bytes(), 0);
    tracker.try_admit(0).unwrap();
    submit(&mut tracker, 0);
    tracker.try_admit(1).unwrap();
    submit(&mut tracker, 1);
    assert_eq!(
        tracker.try_admit(2).unwrap(),
        Admission::Deferred(AdmissionPressure::LiveBytes)
    );
    tracker.completed(0, true).unwrap();
    assert_eq!(tracker.live_bytes(), 128); // Input still needed by submitted consumer.
    tracker.completed(1, true).unwrap();
    assert_eq!(tracker.live_bytes(), 64);
    assert_eq!(tracker.try_admit(2).unwrap(), Admission::Admitted);
}

struct Lease(Arc<AtomicUsize>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn backing_owner_survives_until_last_submitted_reader_completes() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut tracker = tracker(&[root(chain(1))], 128, 1);
    tracker.try_admit(0).unwrap();
    tracker
        .retain_owner(0, Arc::new(Lease(drops.clone())))
        .unwrap();
    submit(&mut tracker, 0);
    tracker.try_admit(1).unwrap();
    submit(&mut tracker, 1);
    tracker.completed(0, true).unwrap();
    tracker.release_roots();
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    tracker.completed(1, true).unwrap();
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(tracker.live_bytes(), 0);
}

#[test]
fn failure_invalidates_descendants_without_releasing_submitted_reads() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut tracker = tracker(&[root(chain(2))], 192, 1);
    tracker.try_admit(0).unwrap();
    tracker
        .retain_owner(0, Arc::new(Lease(drops.clone())))
        .unwrap();
    submit(&mut tracker, 0);
    tracker.try_admit(1).unwrap();
    submit(&mut tracker, 1);
    tracker.completed(0, false).unwrap();
    assert!(!tracker.is_valid(1).unwrap());
    assert_eq!(tracker.state(2).unwrap(), ExecutionState::Abandoned);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert!(tracker.discarded(1).is_err());
    tracker.completed(1, true).unwrap();
    assert_eq!(tracker.state(1).unwrap(), ExecutionState::Failed);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert!(tracker.is_quiescent());
}

#[test]
fn failed_recording_needs_discard_and_does_not_invalidate_independent_results() {
    let roots = [
        root(chain(1)),
        root(Content::solid(ColorValue::srgb([0., 1., 0., 1.]).unwrap())),
    ];
    let mut tracker = tracker(&roots, 192, 2);
    tracker.try_admit(0).unwrap();
    submit(&mut tracker, 0);
    tracker.try_admit(1).unwrap();
    tracker.try_admit(2).unwrap();
    submit(&mut tracker, 2);
    tracker.completed(2, true).unwrap();
    tracker.completed(0, false).unwrap();
    assert_eq!(tracker.state(1).unwrap(), ExecutionState::Encoding);
    assert!(tracker.submitted(1).is_err());
    assert!(tracker.is_valid(2).unwrap());
    tracker.discarded(1).unwrap();
    assert_eq!(tracker.live_bytes(), 64);
    tracker.release_roots();
    assert_eq!(tracker.live_bytes(), 0);
}

#[test]
fn cancellation_releases_queued_consumers_but_not_active_gpu_work() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut tracker = tracker(&[root(chain(1))], 128, 1);
    tracker.try_admit(0).unwrap();
    tracker
        .retain_owner(0, Arc::new(Lease(drops.clone())))
        .unwrap();
    submit(&mut tracker, 0);
    tracker.cancel_unstarted();
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    tracker.completed(0, true).unwrap();
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn presentation_generation_rejects_late_and_cross_window_results() {
    let mut epoch = PresentationEpoch::default();
    let first = epoch.request();
    let second = epoch.request();
    let mut other = PresentationEpoch::default();
    other.request();
    other.request();
    assert!(!epoch.is_current(first));
    assert!(epoch.is_current(second));
    assert!(!other.is_current(second));
}

#[test]
fn completion_lease_protects_input_owners_after_renderer_tracker_teardown() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut tracker = tracker(&[root(chain(1))], 128, 1);
    tracker.try_admit(0).unwrap();
    tracker
        .retain_owner(0, Arc::new(Lease(drops.clone())))
        .unwrap();
    let producer_completion = tracker.submitted(0).unwrap();
    tracker.try_admit(1).unwrap();
    let consumer_completion = tracker.submitted(1).unwrap();
    assert_eq!(consumer_completion.owner_count(), 1);
    drop(tracker);
    drop(producer_completion);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(consumer_completion); // Native consumer completion would release this.
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn thread_affine_owners_use_the_same_lifecycle_without_unsafe_send() {
    let g = graph(&[root(chain(0))]);
    let mut tracker = ExecutionPlan::new(
        g,
        vec![64],
        ExecutionLimits {
            max_live_bytes: 64,
            max_recording_nodes: 1,
        },
    )
    .unwrap()
    .start_with_owner::<std::rc::Rc<Lease>>();
    let drops = Arc::new(AtomicUsize::new(0));
    tracker.try_admit(0).unwrap();
    tracker
        .retain_owner(0, std::rc::Rc::new(Lease(drops.clone())))
        .unwrap();
    let completion = tracker.submitted(0).unwrap();
    drop(tracker);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(completion);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn independent_root_cannot_reserve_memory_needed_by_an_earlier_working_set() {
    let roots = [
        root(chain(1)),
        root(Content::solid(ColorValue::srgb([0., 1., 0., 1.]).unwrap())),
    ];
    // The serial plan needs 128 bytes. Admitting the independent root first
    // would pin 64 bytes, leaving no room for both the copy's input and output.
    let mut tracker = tracker(&roots, 128, 2);
    assert_eq!(
        tracker.try_admit(2).unwrap(),
        Admission::Deferred(AdmissionPressure::ExecutionOrder)
    );
    assert_eq!(tracker.live_bytes(), 0);
    tracker.try_admit(0).unwrap();
    submit(&mut tracker, 0);
    tracker.completed(0, true).unwrap();
    tracker.try_admit(1).unwrap();
    submit(&mut tracker, 1);
    tracker.completed(1, true).unwrap();
    assert_eq!(tracker.try_admit(2).unwrap(), Admission::Admitted);
}

#[test]
fn late_failure_notification_invalidates_an_already_completed_consumer() {
    let mut tracker = tracker(&[root(chain(1))], 128, 1);
    tracker.try_admit(0).unwrap();
    submit(&mut tracker, 0);
    tracker.try_admit(1).unwrap();
    submit(&mut tracker, 1);
    // Completion notifications may be delivered in a different order from GPU work.
    tracker.completed(1, true).unwrap();
    assert!(tracker.is_valid(1).unwrap());
    tracker.completed(0, false).unwrap();
    assert_eq!(tracker.state(1).unwrap(), ExecutionState::Failed);
    assert!(!tracker.is_valid(1).unwrap());
    assert_eq!(tracker.live_bytes(), 0);
}
