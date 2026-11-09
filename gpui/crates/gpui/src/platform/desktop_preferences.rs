//! Optional desktop preferences for toolkit-owned window frames.

/// Desktop preferences exposed by the platform, independent of a particular toolkit.
///
/// `None` means unavailable or unset, not an instruction to override an app's defaults.
/// Linux reads Settings portal keys with optional native desktop fallbacks; other
/// platforms currently return an empty snapshot. Color scheme and button placement
/// retain their existing separate APIs.
/// These preferences do not change application styling or behavior automatically.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesktopPreferences {
    /// Preferred accent as encoded sRGB components in the inclusive range 0–1.
    pub accent_color: Option<[f32; 3]>,
    /// Whether the desktop explicitly requests high contrast.
    pub high_contrast: Option<bool>,
    /// Whether the desktop explicitly requests reduced motion.
    pub reduced_motion: Option<bool>,
    /// Positive logical text scale, independent of device/display scaling.
    pub text_scale: Option<f32>,
    /// Desktop font description, for example `Cantarell 11`; not a font family alone.
    pub font: Option<String>,
    /// Preferred title-bar double-click action, when exposed by the desktop.
    pub titlebar_double_click: Option<TitlebarAction>,
    /// Preferred title-bar middle-click action, when exposed by the desktop.
    pub titlebar_middle_click: Option<TitlebarAction>,
    /// Preferred title-bar right-click action, when exposed by the desktop.
    pub titlebar_right_click: Option<TitlebarAction>,
}

/// A supported desktop title-bar action. Unsupported actions remain unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitlebarAction {
    /// Intentionally take no action.
    None,
    /// Toggle maximization when the window supports it.
    ToggleMaximize,
    /// Minimize when the window supports it.
    Minimize,
    /// Display the platform window menu when supported.
    Menu,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn observers_read_the_new_snapshot_and_stop_when_dropped(cx: &mut crate::TestAppContext) {
        let received = Rc::new(RefCell::new(Vec::new()));
        let observations = received.clone();
        let subscription = cx.update(|app| {
            assert_eq!(app.desktop_preferences(), DesktopPreferences::default());
            app.observe_desktop_preferences(move |app| {
                observations.borrow_mut().push(app.desktop_preferences());
            })
        });
        let preferences = DesktopPreferences {
            high_contrast: Some(true),
            reduced_motion: Some(false),
            text_scale: Some(1.5),
            ..Default::default()
        };
        cx.test_platform()
            .simulate_desktop_preferences(preferences.clone());
        assert_eq!(*received.borrow(), vec![preferences]);
        drop(subscription);
        cx.test_platform()
            .simulate_desktop_preferences(DesktopPreferences::default());
        assert_eq!(received.borrow().len(), 1);
        cx.update(|app| assert_eq!(app.desktop_preferences(), DesktopPreferences::default()));
    }
}
