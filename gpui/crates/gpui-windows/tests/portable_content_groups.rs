#![cfg(all(target_os = "windows", feature = "test-support"))]
use gpui_windows::{
    Dx11HeadlessRenderer as Renderer,
    content_executor::{Dx11ContentLimits as ContentLimits, Dx11ContentTestGate as ContentGate},
};
fn new_renderer() -> Renderer {
    Renderer::new().unwrap()
}
include!("../../gpui/tests/support/atomic_content_groups.rs");

#[test]
fn wide_group_retries_after_color_policy_change_and_discards_incompatible_history() {
    if isolated("wide_group_retries_after_color_policy_change_and_discards_incompatible_history") {
        return;
    }
    let (mut renderer, rx) = setup();
    let mut g = ContentGroup::new(ContentGroupId::new());
    g.push(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(64.), px(32.))),
            corner_radii: Corners::default(),
        },
        Content::solid(
            ColorValue::new(
                [0.25, 0.125, 0., 0.5],
                ImageInterpretation {
                    color: ColorEncoding::LinearP3,
                    alpha: AlphaMode::Premultiplied,
                },
            )
            .unwrap(),
        ),
    );
    let s = scene(g, false);
    canvas_solid(&read(&mut renderer, &s), [0, 0, 0, 255]);
    assert_eq!(
        renderer
            .content_presentation_stats()
            .last_error
            .unwrap()
            .kind,
        ContentErrorKind::UnsupportedCapability
    );
    renderer
        .set_color_mode(gpui::WindowColorMode::WideGamutSdr)
        .unwrap();
    read(&mut renderer, &s);
    completed(&rx);
    let image = read(&mut renderer, &s);
    let actual = image.get_pixel(4, 4).0;
    // Associated linear P3 (0.25,0.125,0) over opaque black, converted by the
    // compositor's SDR sRGB preview. This also verifies portable alpha metadata.
    for (a, e) in actual.into_iter().zip([144u8, 97, 0, 255]) {
        assert!(a.abs_diff(e) <= 2, "wide group preview: {actual:?}");
    }
    renderer.prepare_content_snapshot(&s).unwrap();
    renderer
        .set_color_mode(gpui::WindowColorMode::Default)
        .unwrap();
    canvas_solid(&read(&mut renderer, &s), [0, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().committed_groups, 0);
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
