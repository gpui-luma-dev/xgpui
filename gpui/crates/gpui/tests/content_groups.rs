//! GPU-independent atomic group generation and policy tests.
use gpui::{
    Bounds, Corners, CustomGeometry, DevicePixels, PreparationRequest, ScaledPixels, content::*,
    point, size,
};
fn invalid(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::InvalidDescriptor, message)
}

fn request(id: ContentGroupId, x: f32) -> ContentGroupRequest {
    let mut group = ContentGroup::new(id);
    group.push(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(
                point(gpui::px(x), gpui::px(0.)),
                size(gpui::px(2.), gpui::px(2.)),
            ),
            corner_radii: Corners::default(),
        },
        Content::solid(ColorValue::srgb([1., 0., 0., 1.]).unwrap()),
    );
    ContentGroupRequest {
        group,
        scale_factor: 1.,
    }
}
fn observe(
    state: &mut ContentGroupState<u32>,
    request: &ContentGroupRequest,
) -> ContentGroupTicket {
    state.observe(&[(request.clone(), 0..1)]).unwrap().remove(0)
}
#[test]
fn admitted_generations_progress_without_mixing_or_rolling_back() {
    let mut state = ContentGroupState::default();
    let id = ContentGroupId::new();
    let first = request(id, 0.);
    let a = observe(&mut state, &first);
    assert!(state.select(&a).unwrap().is_none());
    state.publish(&a, vec![1]);
    let second = request(id, 2.);
    let b = observe(&mut state, &second);
    let third = request(id, 4.);
    let c = observe(&mut state, &third);
    state.publish(&b, vec![2]);
    let selected = state.select(&c).unwrap().unwrap();
    assert_eq!(selected.images, vec![2]);
    assert_eq!(
        selected.request.draws().unwrap()[0].bounds.origin,
        second.draws().unwrap()[0].bounds.origin
    );
    assert_eq!(
        state.require_ready().unwrap_err().kind,
        ContentErrorKind::Pending
    );
    state.publish(&c, vec![3]);
    state.publish(&a, vec![1]);
    assert_eq!(state.select(&c).unwrap().unwrap().images, vec![3]);
    state.require_ready().unwrap();
}
#[test]
fn removal_and_empty_invalidate_publication_epochs() {
    let mut state = ContentGroupState::default();
    let id = ContentGroupId::new();
    let r = request(id, 0.);
    let old = observe(&mut state, &r);
    state.publish(&old, vec![1]);
    state.observe(&[]).unwrap();
    let new = observe(&mut state, &r);
    state.publish(&old, vec![7]);
    assert!(state.select(&new).unwrap().is_none());
    state.publish(&new, vec![2]);
    let empty = ContentGroupRequest {
        group: ContentGroup::new(id),
        scale_factor: 1.,
    };
    let empty_ticket = state.observe(&[(empty, 0..0)]).unwrap().remove(0);
    assert!(state.select(&empty_ticket).unwrap().is_none());
    let again = observe(&mut state, &r);
    state.publish(&new, vec![9]);
    assert!(state.select(&again).unwrap().is_none());
}
#[test]
fn policy_applies_to_the_complete_snapshot_and_obsolete_failures_are_ignored() {
    let mut state = ContentGroupState::default();
    let id = ContentGroupId::new();
    let r = request(id, 0.);
    let a = observe(&mut state, &r);
    state.publish(&a, vec![1]);
    let mut r = request(id, 4.);
    r.group = r.group.presentation(
        PendingPresentation::Transparent,
        ContentErrorPresentation::ReportAndRetainPrevious,
    );
    let b = observe(&mut state, &r);
    assert!(state.select(&b).unwrap().is_none());
    assert!(!state.fail(&a, invalid("obsolete")));
    assert!(state.fail(&b, invalid("current")));
    assert_eq!(state.select(&b).unwrap().unwrap().images, vec![1]);
    assert!(
        !state.fail(&b, invalid("current")),
        "report once per generation"
    );
    assert_eq!(
        state.require_ready().unwrap_err().kind,
        ContentErrorKind::InvalidDescriptor
    );
    state.retain_images(|_| false);
    assert!(state.select(&b).unwrap().is_none());
}
#[test]
fn duplicate_ids_and_incomplete_results_are_rejected_atomically() {
    let mut state = ContentGroupState::default();
    let id = ContentGroupId::new();
    let r = request(id, 0.);
    let a = observe(&mut state, &r);
    state.publish(&a, vec![]);
    assert!(state.select(&a).unwrap().is_none());
    assert_eq!(
        state
            .observe(&[(r.clone(), 0..1), (r, 1..2)])
            .unwrap_err()
            .kind,
        ContentErrorKind::InvalidDescriptor
    );
    state.publish(&a, vec![1]);
    assert_eq!(state.select(&a).unwrap().unwrap().images, vec![1]);
}
#[test]
fn member_requests_preserve_logical_geometry_and_exact_raster_scale() {
    let mut r = request(ContentGroupId::new(), 0.25);
    r.scale_factor = 2.;
    let draws = r.draws().unwrap();
    let draw = &draws[0];
    assert_eq!(draw.bounds.size, size(ScaledPixels(4.), ScaledPixels(4.)));
    let prep = PreparationRequest::new(
        size(gpui::px(4.), gpui::px(4.)),
        size(DevicePixels(4), DevicePixels(4)),
        1.,
    )
    .unwrap();
    let request = draw.content.portable_content(&prep).unwrap().unwrap();
    assert_eq!(
        request.request.logical_size(),
        size(gpui::px(2.), gpui::px(2.))
    );
    assert_eq!(request.request.scale_factor(), 2.);
    assert_eq!(request.request.extent(), ImageExtent::new(4, 4).unwrap());
    r.scale_factor = f32::NAN;
    assert!(r.draws().is_err());
}
