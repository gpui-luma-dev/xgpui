#![cfg(all(not(target_family = "wasm"), feature = "test-support"))]
use gpui_wgpu::{
    WgpuHeadlessRenderer as Renderer,
    content_executor::{WgpuContentLimits as ContentLimits, WgpuContentTestGate as ContentGate},
};
fn new_renderer() -> Renderer {
    Renderer::new().unwrap()
}
include!("../../gpui/tests/support/atomic_content_groups.rs");

#[test]
fn unsupported_wide_group_uses_whole_group_error_policy() {
    if isolated("unsupported_wide_group_uses_whole_group_error_policy") {
        return;
    }
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let initial = scene(group(id, false, false), false);
    read(&mut renderer, &initial);
    completed(&rx);
    let previous = read(&mut renderer, &initial);
    let mut wide = ContentGroup::new(id);
    wide.push(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(64.), px(32.))),
            corner_radii: Corners::default(),
        },
        Content::solid(ColorValue::linear_p3([0.5, 0., 0., 1.]).unwrap()),
    );
    let keep = scene(wide.clone(), false);
    canvas_eq(&read(&mut renderer, &keep), &previous);
    assert_eq!(
        renderer
            .content_presentation_stats()
            .last_error
            .unwrap()
            .kind,
        ContentErrorKind::UnsupportedCapability
    );
    assert!(renderer.prepare_content_snapshot(&keep).is_err());
    let clear = scene(
        wide.presentation(
            PendingPresentation::RetainPrevious,
            ContentErrorPresentation::ReportAndTransparent,
        ),
        false,
    );
    canvas_solid(&read(&mut renderer, &clear), [0, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
}
