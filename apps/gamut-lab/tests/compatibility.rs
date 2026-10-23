use gpui::*;

#[::core::prelude::v1::test]
fn existing_complete_window_literals_need_no_new_fields() {
    let options = WindowOptions {
        window_bounds: None,
        titlebar: None,
        focus: false,
        show: false,
        kind: WindowKind::Normal,
        is_movable: true,
        app_owns_titlebar_drag: false,
        inactive_frame_interval: None,
        is_resizable: true,
        is_minimizable: true,
        display_id: None,
        window_background: WindowBackgroundAppearance::Opaque,
        app_id: None,
        window_min_size: None,
        window_decorations: None,
        icon: None,
        tabbing_identifier: None,
    };
    let params = WindowParams {
        bounds: Bounds::new(Point::default(), size(px(320.), px(240.))),
        titlebar: None,
        kind: WindowKind::Normal,
        is_movable: true,
        app_owns_titlebar_drag: false,
        is_resizable: true,
        is_minimizable: true,
        focus: false,
        show: false,
        icon: None,
        display_id: None,
        app_id: None,
        window_min_size: None,
        tabbing_identifier: None,
    };
    assert!(!options.show && !params.show);
}

#[::core::prelude::v1::test]
fn status_defaults_distinguish_unknown_display_from_unsupported_backend() {
    let status = WindowColorStatus::default();
    assert_eq!(status.requested(), WindowColorMode::Default);
    assert_eq!(status.effective(), WindowColorMode::Default);
    assert!(!status.supports_wide_gamut_sdr());
    assert_eq!(status.display_wide_gamut(), None);
    assert_eq!(status.fallback(), None);
}
