//! Experimental desktop shell. Linux draws negotiated client decorations; other platforms retain native chrome.
use gpui::{prelude::*, *};

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
use linux::*;
use std::{cell::RefCell, collections::HashMap};

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
fn activation_key(event: &KeyDownEvent) -> bool {
    matches!(event.keystroke.key.as_str(), "enter" | "space")
        && !event.keystroke.modifiers.control
        && !event.keystroke.modifiers.alt
        && !event.keystroke.modifiers.platform
        && !event.is_held
}

thread_local! {
    // Platform title reads are macOS-only. Keep the shell chrome's title locally.
    static SHELL_TITLES: RefCell<HashMap<WindowId, String>> = RefCell::new(HashMap::new());
}
pub trait ShellWindowTitle {
    fn set_shell_title(&mut self, title: &str);
    fn shell_title(&self) -> String;
}
impl ShellWindowTitle for Window {
    fn set_shell_title(&mut self, title: &str) {
        self.set_window_title(title);
        SHELL_TITLES.with(|titles| {
            titles
                .borrow_mut()
                .insert(self.window_handle().window_id(), title.to_owned())
        });
        self.refresh();
    }
    fn shell_title(&self) -> String {
        SHELL_TITLES
            .with(|titles| {
                titles
                    .borrow()
                    .get(&self.window_handle().window_id())
                    .cloned()
            })
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "XGPUI".to_owned())
    }
}

/// Open an application with compositor-provided chrome or a shared client-side frame.
pub trait ShellApp {
    fn open_shell_window<V: Render + 'static>(
        &mut self,
        options: WindowOptions,
        shell: ShellOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<WindowHandle<ShellWindow>>;
}
impl ShellApp for App {
    fn open_shell_window<V: Render + 'static>(
        &mut self,
        options: WindowOptions,
        shell: ShellOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<WindowHandle<ShellWindow>> {
        let show_chrome = options.show;
        let initial_title = options
            .titlebar
            .as_ref()
            .and_then(|titlebar| titlebar.title.clone());
        self.open_window(options, |window, cx| {
            if let Some(title) = initial_title {
                window.set_shell_title(&title);
            }
            let content = build(window, cx).into();
            cx.new(|cx| {
                window
                    .observe_button_layout_changed(|window, _| window.refresh())
                    .detach();
                window
                    .observe_window_appearance(|window, _| window.refresh())
                    .detach();
                cx.observe_window_activation(window, |_, window, _| window.refresh())
                    .detach();
                cx.observe_window_bounds(window, |_, window, _| window.refresh())
                    .detach();
                let shell_entity = cx.entity().downgrade();
                let preferences_subscription = cx.observe_desktop_preferences(move |app| {
                    let _ = shell_entity.update(app, |_, cx| cx.notify());
                });
                ShellWindow {
                    _preferences_subscription: preferences_subscription,
                    shell,
                    content,
                    show_chrome,
                    window_id: window.window_handle().window_id(),
                }
            })
        })
    }
}

pub struct ShellWindow {
    _preferences_subscription: Subscription,
    shell: ShellOptions,
    content: AnyView,
    show_chrome: bool,
    window_id: WindowId,
}
impl ShellWindow {
    /// Replace application-supplied overrides and redraw this window.
    pub fn set_options(&mut self, options: ShellOptions, cx: &mut Context<Self>) {
        self.shell = options;
        cx.notify();
    }
}
impl Drop for ShellWindow {
    fn drop(&mut self) {
        let _ = SHELL_TITLES.try_with(|titles| titles.borrow_mut().remove(&self.window_id));
    }
}
impl Render for ShellWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.content.clone();
        #[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
        if self.show_chrome
            && !window.is_fullscreen()
            && let Decorations::Client { tiling } = window.window_decorations()
        {
            let maximized = window.is_maximized();
            let preferences = self.shell.resolved_preferences(&cx.desktop_preferences());
            let palette = Palette::new(
                window.appearance(),
                window.is_window_active(),
                preferences.high_contrast,
            );
            let scale = preferences.text_scale;
            let button_size = px(33. * scale);
            let mut controls = window.window_controls();
            controls.minimize &= window.is_minimizable();
            controls.maximize &= window.is_resizable();
            let left_buttons = visible_buttons(
                cx.button_layout()
                    .unwrap_or_else(WindowButtonLayout::linux_default)
                    .left,
                controls,
            );
            let right_buttons = visible_buttons(
                cx.button_layout()
                    .unwrap_or_else(WindowButtonLayout::linux_default)
                    .right,
                controls,
            );
            let side_width =
                px((left_buttons.len().max(right_buttons.len()) as f32 * 33. + 8.) * scale);
            let inset = if maximized { px(0.) } else { px(5.) };
            window.set_client_inset(inset);
            let buttons = |side: Vec<WindowButton>, right: bool| {
                div()
                    .flex()
                    .items_center()
                    .flex_none()
                    .min_w(side_width)
                    .when(right, |s| s.justify_end())
                    .children(side.into_iter().map(|button| {
                        let label = control_label(button, maximized);
                        control_button(
                            button.id(),
                            label,
                            palette,
                            if preferences.high_contrast {
                                palette.text
                            } else {
                                preferences.accent
                            },
                            button_size,
                        )
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            activate_control(button, window);
                        })
                        .on_key_down(move |event, window, cx| {
                            if activation_key(event) {
                                cx.stop_propagation();
                                activate_control(button, window);
                            }
                        })
                        .on_a11y_action(AccessibleAction::Click, move |_, window, _| {
                            activate_control(button, window)
                        })
                        .child(if button == WindowButton::Close {
                            // Draw around the geometric center, independent of font baselines.
                            canvas(
                                |_, _, _| (),
                                move |bounds, _, window, _| {
                                    let margin = px(2. * scale);
                                    let start = bounds.origin + point(margin, margin);
                                    let end = bounds.bottom_right() - point(margin, margin);
                                    for (a, b) in [
                                        (start, end),
                                        (point(start.x, end.y), point(end.x, start.y)),
                                    ] {
                                        let mut path = PathBuilder::stroke(px(2. * scale));
                                        path.move_to(a);
                                        path.line_to(b);
                                        if let Ok(path) = path.build() {
                                            window.paint_path(path, rgb(palette.text));
                                        }
                                    }
                                },
                            )
                            .size(px(20. * scale))
                            .into_any_element()
                        } else {
                            div()
                                .child(match button {
                                    WindowButton::Minimize => "−",
                                    WindowButton::Maximize if maximized => "▣",
                                    _ => "□",
                                })
                                .into_any_element()
                        })
                    }))
            };
            let title = div()
                .id("shell-titlebar")
                .flex_none()
                .h(px(40. * scale))
                .px(px(4. * scale))
                .flex()
                .items_center()
                .bg(rgb(palette.header))
                .text_color(rgb(palette.text))
                .text_size(px(14. * scale))
                .font_weight(FontWeight::SEMIBOLD)
                .border_b_1()
                .border_color(rgb(palette.border))
                .child(buttons(left_buttons, false))
                .child(
                    div()
                        .id("shell-title-drag")
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(MouseButton::Left, {
                            let action = preferences.double_click_action;
                            move |event, window, _| {
                                if event.click_count == 2 {
                                    action.perform_at(window, event.position);
                                } else {
                                    window.start_window_move();
                                }
                            }
                        })
                        .on_mouse_down(MouseButton::Right, {
                            let action = preferences.right_click_action;
                            move |event, window, _| action.perform_at(window, event.position)
                        })
                        .on_mouse_down(MouseButton::Middle, {
                            let action = preferences.middle_click_action;
                            move |event, window, _| action.perform_at(window, event.position)
                        })
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .text_center()
                                .child(window.shell_title()),
                        ),
                )
                .child(buttons(right_buttons, true));
            // The inset handles resizing; content events must not initiate window moves.
            let mut frame = div()
                .id("shell-frame")
                .size_full()
                .relative()
                .bg(rgb(palette.border));
            let edges = [
                (ResizeEdge::Top, !tiling.top, CursorStyle::ResizeUpDown),
                (
                    ResizeEdge::Bottom,
                    !tiling.bottom,
                    CursorStyle::ResizeUpDown,
                ),
                (ResizeEdge::Left, !tiling.left, CursorStyle::ResizeLeftRight),
                (
                    ResizeEdge::Right,
                    !tiling.right,
                    CursorStyle::ResizeLeftRight,
                ),
                (
                    ResizeEdge::TopLeft,
                    !tiling.top && !tiling.left,
                    CursorStyle::ResizeUpLeftDownRight,
                ),
                (
                    ResizeEdge::TopRight,
                    !tiling.top && !tiling.right,
                    CursorStyle::ResizeUpRightDownLeft,
                ),
                (
                    ResizeEdge::BottomLeft,
                    !tiling.bottom && !tiling.left,
                    CursorStyle::ResizeUpRightDownLeft,
                ),
                (
                    ResizeEdge::BottomRight,
                    !tiling.bottom && !tiling.right,
                    CursorStyle::ResizeUpLeftDownRight,
                ),
            ];
            frame = frame
                .when(!maximized && !tiling.top, |s| s.pt(inset))
                .when(!maximized && !tiling.bottom, |s| s.pb(inset))
                .when(!maximized && !tiling.left, |s| s.pl(inset))
                .when(!maximized && !tiling.right, |s| s.pr(inset))
                .child(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .when(!maximized && !tiling.is_tiled(), |s| {
                            s.rounded(px(10.)).overflow_hidden()
                        })
                        .child(title)
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .min_w_0()
                                .overflow_hidden()
                                .child(content),
                        ),
                );
            if !maximized && window.is_resizable() {
                for (index, (edge, enabled, cursor)) in edges.into_iter().enumerate() {
                    if !enabled {
                        continue;
                    }
                    let region = div()
                        .id(("shell-resize", index))
                        .absolute()
                        .cursor(cursor)
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            window.start_window_resize(edge);
                        });
                    let region = match edge {
                        ResizeEdge::Top => region.top_0().left(inset).right(inset).h(inset),
                        ResizeEdge::Bottom => region.bottom_0().left(inset).right(inset).h(inset),
                        ResizeEdge::Left => region.left_0().top(inset).bottom(inset).w(inset),
                        ResizeEdge::Right => region.right_0().top(inset).bottom(inset).w(inset),
                        ResizeEdge::TopLeft => region.top_0().left_0().size(inset),
                        ResizeEdge::TopRight => region.top_0().right_0().size(inset),
                        ResizeEdge::BottomLeft => region.bottom_0().left_0().size(inset),
                        ResizeEdge::BottomRight => region.bottom_0().right_0().size(inset),
                    };
                    frame = frame.child(region);
                }
            }
            return frame.into_any_element();
        }
        // Clear any prior client frame when switching to server chrome/fullscreen.
        window.set_client_inset(px(0.));
        let _ = (cx, self.show_chrome, &self.shell);
        content.into_any_element()
    }
}

/// Optional per-window overrides. `None` follows desktop preferences, then built-in fallbacks.
///
/// Unlike the initial experimental API, these fields are optional: wrap explicit values in
/// `Some(...)`. This preserves explicit `false` and `TitlebarAction::None` overrides.
/// Light/dark appearance and button placement retain the platform's existing APIs.
#[derive(Default)]
pub struct ShellOptions {
    /// Override system high contrast.
    pub high_contrast: Option<bool>,
    /// Override the sRGB focus accent as 0xRRGGBB.
    pub accent: Option<u32>,
    /// Override logical text/target scaling, independent of device DPI.
    pub text_scale: Option<f32>,
    /// Override the desktop double-click action; fallback is maximize/restore.
    pub double_click_action: Option<TitlebarAction>,
    /// Override the desktop middle-click action; fallback is no action.
    pub middle_click_action: Option<TitlebarAction>,
    /// Override the desktop right-click action; fallback is the window menu.
    pub right_click_action: Option<TitlebarAction>,
}

fn shell_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(0.5, 4.)
    } else {
        1.
    }
}

/// The values actually used by the shared client frame after overrides and fallbacks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedShellPreferences {
    /// Whether the frame uses high-contrast colors.
    pub high_contrast: bool,
    /// Encoded sRGB focus accent, 0xRRGGBB.
    pub accent: u32,
    /// Logical text/target scale, clamped to 0.5–4 independently of device DPI.
    pub text_scale: f32,
    /// Effective title-bar double-click action.
    pub double_click_action: TitlebarAction,
    /// Effective title-bar middle-click action.
    pub middle_click_action: TitlebarAction,
    /// Effective title-bar right-click action.
    pub right_click_action: TitlebarAction,
}

impl ShellOptions {
    /// Resolve the same values used by the client frame. This does not apply them to
    /// native/server decorations or alter desktop settings. Useful for diagnostics.
    pub fn resolved_preferences(&self, desktop: &DesktopPreferences) -> ResolvedShellPreferences {
        let accent = desktop
            .accent_color
            .filter(|c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
            .map(|[r, g, b]| {
                ((r * 255.).round() as u32) << 16
                    | ((g * 255.).round() as u32) << 8
                    | (b * 255.).round() as u32
            });
        ResolvedShellPreferences {
            high_contrast: self
                .high_contrast
                .or(desktop.high_contrast)
                .unwrap_or(false),
            accent: self.accent.or(accent).unwrap_or(0x3584e4),
            text_scale: shell_scale(self.text_scale.or(desktop.text_scale).unwrap_or(1.)),
            double_click_action: self
                .double_click_action
                .or_else(|| {
                    desktop
                        .titlebar_double_click
                        .map(TitlebarAction::from_platform)
                })
                .unwrap_or(TitlebarAction::ToggleMaximize),
            middle_click_action: self
                .middle_click_action
                .or_else(|| {
                    desktop
                        .titlebar_middle_click
                        .map(TitlebarAction::from_platform)
                })
                .unwrap_or(TitlebarAction::None),
            right_click_action: self
                .right_click_action
                .or_else(|| {
                    desktop
                        .titlebar_right_click
                        .map(TitlebarAction::from_platform)
                })
                .unwrap_or(TitlebarAction::Menu),
        }
    }
}

/// Supported title-bar interaction, resolved from desktop preferences or an explicit override.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitlebarAction {
    #[default]
    ToggleMaximize,
    Minimize,
    None,
    /// Display the platform window menu when supported.
    Menu,
}
impl TitlebarAction {
    fn from_platform(action: gpui::TitlebarAction) -> Self {
        match action {
            gpui::TitlebarAction::None => Self::None,
            gpui::TitlebarAction::ToggleMaximize => Self::ToggleMaximize,
            gpui::TitlebarAction::Minimize => Self::Minimize,
            gpui::TitlebarAction::Menu => Self::Menu,
        }
    }

    #[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
    fn perform_at(self, window: &mut Window, position: Point<Pixels>) {
        match self {
            Self::ToggleMaximize if window.window_controls().maximize && window.is_resizable() => {
                window.zoom_window()
            }
            Self::Minimize if window.window_controls().minimize && window.is_minimizable() => {
                window.minimize_window()
            }
            Self::Menu if window.window_controls().window_menu => window.show_window_menu(position),
            _ => (),
        }
    }
}

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
mod linux {
    use super::*;
    pub(super) fn visible_buttons(
        side: [Option<WindowButton>; MAX_BUTTONS_PER_SIDE],
        controls: WindowControls,
    ) -> Vec<WindowButton> {
        side.into_iter()
            .flatten()
            .filter(|button| match button {
                WindowButton::Minimize => controls.minimize,
                WindowButton::Maximize => controls.maximize,
                WindowButton::Close => true,
            })
            .collect()
    }
    pub(super) fn control_label(button: WindowButton, maximized: bool) -> &'static str {
        match button {
            WindowButton::Minimize => "Minimize window",
            WindowButton::Maximize if maximized => "Restore window",
            WindowButton::Maximize => "Maximize window",
            WindowButton::Close => "Close window",
        }
    }
    pub(super) fn activate_control(button: WindowButton, window: &mut Window) {
        // Capabilities may have changed between rendering and activation.
        match button {
            WindowButton::Minimize
                if window.window_controls().minimize && window.is_minimizable() =>
            {
                window.minimize_window()
            }
            WindowButton::Maximize
                if window.window_controls().maximize && window.is_resizable() =>
            {
                window.zoom_window()
            }
            WindowButton::Close => window.remove_window(),
            _ => (),
        }
    }
    #[derive(Clone, Copy)]
    pub(super) struct Palette {
        pub(super) header: u32,
        pub(super) text: u32,
        pub(super) border: u32,
        pub(super) hover: u32,
    }
    impl Palette {
        pub(super) fn new(appearance: WindowAppearance, active: bool, contrast: bool) -> Self {
            let dark = matches!(
                appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            );
            match (dark, active, contrast) {
                (true, _, true) => Self {
                    header: 0x000000,
                    text: 0xffffff,
                    border: 0xffffff,
                    hover: 0x333333,
                },
                (false, _, true) => Self {
                    header: 0xffffff,
                    text: 0x000000,
                    border: 0x000000,
                    hover: 0xdddddd,
                },
                (true, true, false) => Self {
                    header: 0x303030,
                    text: 0xf6f5f4,
                    border: 0x141414,
                    hover: 0x484848,
                },
                (true, false, false) => Self {
                    header: 0x282828,
                    text: 0xaaaaaa,
                    border: 0x202020,
                    hover: 0x404040,
                },
                (false, true, false) => Self {
                    header: 0xebebeb,
                    text: 0x202020,
                    border: 0xb8b8b8,
                    hover: 0xd8d8d8,
                },
                (false, false, false) => Self {
                    header: 0xf5f5f5,
                    text: 0x707070,
                    border: 0xd5d5d5,
                    hover: 0xe3e3e3,
                },
            }
        }
    }
    struct ControlTooltip(&'static str);
    impl Render for ControlTooltip {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let palette = Palette::new(window.appearance(), true, false);
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(palette.header))
                .text_color(rgb(palette.text))
                .child(self.0)
        }
    }
    pub(super) fn control_button(
        id: &'static str,
        label: &'static str,
        palette: Palette,
        accent: u32,
        size: Pixels,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_stop(true)
            .size(size)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_2()
            .border_color(transparent_black())
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(palette.hover)))
            .focus_visible(move |s| s.border_color(rgb(accent)))
            .tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[::core::prelude::v1::test]
        fn omitted_and_unavailable_buttons_are_not_reintroduced() {
            let close_only = WindowButtonLayout::parse(":close").unwrap();
            assert_eq!(
                visible_buttons(close_only.right, WindowControls::default()),
                vec![WindowButton::Close]
            );
            let none = WindowButtonLayout::parse(":").unwrap();
            assert!(visible_buttons(none.left, WindowControls::default()).is_empty());
            assert!(visible_buttons(none.right, WindowControls::default()).is_empty());
            let controls = WindowControls {
                minimize: false,
                maximize: false,
                ..Default::default()
            };
            assert_eq!(
                visible_buttons(WindowButtonLayout::linux_default().right, controls),
                vec![WindowButton::Close]
            );
        }
        #[::core::prelude::v1::test]
        fn light_dark_inactive_and_contrast_have_distinct_tokens() {
            let light = Palette::new(WindowAppearance::Light, true, false);
            let dark = Palette::new(WindowAppearance::Dark, true, false);
            assert_ne!(light.header, dark.header);
            assert_ne!(
                light.text,
                Palette::new(WindowAppearance::Light, false, false).text
            );
            assert_eq!(
                Palette::new(WindowAppearance::Dark, true, true).text,
                0xffffff
            );
            assert_eq!(
                Palette::new(WindowAppearance::Light, true, true).text,
                0x000000
            );
        }
        #[::core::prelude::v1::test]
        fn invalid_text_scales_cannot_break_layout() {
            assert_eq!(shell_scale(f32::NAN), 1.);
            assert_eq!(shell_scale(f32::INFINITY), 1.);
            assert_eq!(shell_scale(-1.), 0.5);
            assert_eq!(shell_scale(1.5), 1.5);
            assert_eq!(shell_scale(100.), 4.);
        }
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn desktop_preferences_follow_live_values_and_explicit_overrides_win() {
        let desktop = DesktopPreferences {
            high_contrast: Some(true),
            accent_color: Some([1., 0., 0.]),
            text_scale: Some(1.5),
            titlebar_double_click: Some(gpui::TitlebarAction::Minimize),
            titlebar_middle_click: Some(gpui::TitlebarAction::Menu),
            ..Default::default()
        };
        let inherited = ShellOptions::default().resolved_preferences(&desktop);
        assert!(inherited.high_contrast);
        assert_eq!(inherited.accent, 0xff0000);
        assert_eq!(inherited.text_scale, 1.5);
        assert_eq!(inherited.double_click_action, TitlebarAction::Minimize);
        assert_eq!(inherited.middle_click_action, TitlebarAction::Menu);
        assert_eq!(inherited.right_click_action, TitlebarAction::Menu);
        let overridden = ShellOptions {
            high_contrast: Some(false),
            accent: Some(0),
            text_scale: Some(1.),
            double_click_action: Some(TitlebarAction::None),
            middle_click_action: Some(TitlebarAction::None),
            right_click_action: Some(TitlebarAction::None),
        }
        .resolved_preferences(&desktop);
        assert!(!overridden.high_contrast);
        assert_eq!(overridden.accent, 0);
        assert_eq!(overridden.text_scale, 1.);
        assert_eq!(overridden.double_click_action, TitlebarAction::None);
        assert_eq!(overridden.middle_click_action, TitlebarAction::None);
        assert_eq!(overridden.right_click_action, TitlebarAction::None);
        let reset = ShellOptions::default().resolved_preferences(&DesktopPreferences::default());
        assert!(!reset.high_contrast);
        assert_eq!(reset.text_scale, 1.);
        assert_eq!(reset.double_click_action, TitlebarAction::ToggleMaximize);
    }

    #[::core::prelude::v1::test]
    fn activation_keys_do_not_consume_modified_shortcuts_or_repeats() {
        let key = |text: &'static str, held| KeyDownEvent {
            keystroke: Keystroke::parse(text).unwrap(),
            is_held: held,
            prefer_character_input: false,
        };
        assert!(activation_key(&key("enter", false)));
        assert!(activation_key(&key("space", false)));
        assert!(!activation_key(&key("ctrl-enter", false)));
        assert!(!activation_key(&key("alt-space", false)));
        assert!(!activation_key(&key("enter", true)));
        assert!(!activation_key(&key("a", false)));
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use ::core::prelude::v1::test;
    struct Content;
    impl Render for Content {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    #[gpui::test]
    fn independent_titles_are_released_on_close(cx: &mut TestAppContext) {
        let (one, two) = cx.update(|cx| {
            let mut open = |title: &str| {
                cx.open_shell_window(
                    WindowOptions {
                        titlebar: Some(TitlebarOptions {
                            title: Some(title.to_owned().into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    ShellOptions::default(),
                    |_, cx| cx.new(|_| Content),
                )
                .unwrap()
            };
            (open("First"), open("Second"))
        });
        let one_id = one.window_id();
        let two_id = two.window_id();
        cx.update_window(*one, |_, window, _| {
            assert_eq!(window.shell_title(), "First");
            window.set_shell_title("Updated");
        })
        .unwrap();
        cx.update_window(*two, |_, window, _| {
            assert_eq!(window.shell_title(), "Second")
        })
        .unwrap();
        cx.update_window(*one, |_, window, _| {
            assert_eq!(window.shell_title(), "Updated");
            window.remove_window();
        })
        .unwrap();
        cx.run_until_parked();
        SHELL_TITLES.with(|titles| {
            assert!(!titles.borrow().contains_key(&one_id));
            assert!(titles.borrow().contains_key(&two_id));
        });
        cx.update_window(*two, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        SHELL_TITLES.with(|titles| assert!(!titles.borrow().contains_key(&two_id)));
    }
}
