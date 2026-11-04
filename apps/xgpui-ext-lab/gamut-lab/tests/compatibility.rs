use gpui::*;

#[::core::prelude::v1::test]
fn explicit_window_options_preserve_default_color_mode() {
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
        use_display_p3: false,
    };
    assert!(!options.show);
    assert!(!options.use_display_p3);
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
