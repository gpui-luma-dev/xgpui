//! Interactive diagnostics for the shared shell; no desktop settings are written.
mod titlebar;
use anyhow::{Context as _, Result};
use clap::Parser;
use gpui::{prelude::*, *};
use std::{
    collections::VecDeque,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};
use titlebar::{HeaderAssets, LabTitleBar};
use xgpui_shell::{
    ClientFramePreview, ShellApp, ShellControl, ShellControlPresenter, ShellControlState,
    ShellOptions, ShellTypography, ShellWindow, ShellWindowTitle, TitlebarAction,
};

actions!(shell_lab, [FocusNext, FocusPrevious]);

/// Tab and Shift-Tab move keyboard focus through the whole window, title-bar
/// controls included.
fn bind_focus_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
    ]);
    cx.on_action(|_: &FocusNext, cx| move_focus(cx, true));
    cx.on_action(|_: &FocusPrevious, cx| move_focus(cx, false));
}

fn move_focus(cx: &mut App, forward: bool) {
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, |_, window, cx| {
            if forward {
                window.focus_next(cx)
            } else {
                window.focus_prev(cx)
            }
        });
    }
}

#[derive(Parser)]
#[command(about = "Inspect desktop preferences and test per-window xgpui-shell overrides")]
struct Options {
    /// Request client decorations; the compositor still decides the actual mode.
    #[arg(long)]
    client_decorations: bool,
    /// Open a non-resizable window to test capability filtering.
    #[arg(long)]
    fixed_size: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Overrides {
    contrast: Option<bool>,
    accent: Option<u32>,
    scale: Option<f32>,
    typography: Option<TypographyPreset>,
    reduced_motion: Option<bool>,
    double_click: Option<TitlebarAction>,
    middle_click: Option<TitlebarAction>,
    right_click: Option<TitlebarAction>,
}
impl Overrides {
    fn shell_options(self) -> ShellOptions {
        ShellOptions {
            high_contrast: self.contrast,
            accent: self.accent,
            text_scale: self.scale,
            typography: self.typography.map(TypographyPreset::typography),
            reduced_motion: self.reduced_motion,
            double_click_action: self.double_click,
            middle_click_action: self.middle_click,
            right_click_action: self.right_click,
        }
    }

    fn apply(&mut self, action: Action) -> bool {
        match action {
            Action::Contrast(value) => self.contrast = value,
            Action::Accent(value) => self.accent = value,
            Action::Scale(value) => self.scale = value,
            Action::Typography(value) => self.typography = value,
            Action::ReducedMotion(value) => self.reduced_motion = value,
            Action::Titlebar(Click::Double, value) => self.double_click = value,
            Action::Titlebar(Click::Middle, value) => self.middle_click = value,
            Action::Titlebar(Click::Right, value) => self.right_click = value,
            Action::Reset => *self = Self::default(),
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Copy, Debug)]
enum Click {
    Double,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Contrast(Option<bool>),
    Accent(Option<u32>),
    Scale(Option<f32>),
    Typography(Option<TypographyPreset>),
    ReducedMotion(Option<bool>),
    Titlebar(Click, Option<TitlebarAction>),
    Reset,
    Refresh,
    TogglePlatformReadings,
    LongTitle,
    Maximize,
    Minimize,
    Fullscreen,
    WindowMenu,
    CompactSize,
    NormalSize,
    RequestDecorations(WindowDecorations),
    NewWindow { fixed: bool },
    PreviewMode(PreviewMode),
    PreviewAppearance(Option<WindowAppearance>),
    PreviewActive(bool),
    PreviewFocus(bool),
    PreviewHover(bool),
    PreviewLayout(PreviewLayout),
    ControlArtwork(ControlArtwork),
}

/// Window-control artwork: the shell's built-in look, or an app presenter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ControlArtwork {
    #[default]
    Builtin,
    /// App SVG icons on a filled circle.
    FilledCircle,
    /// The same SVG icons with the circle treatment removed.
    PlainIcons,
}
impl ControlArtwork {
    fn presenter(self) -> Option<ShellControlPresenter> {
        match self {
            Self::Builtin => None,
            Self::FilledCircle => Some(ShellControlPresenter::new(|state, _, _| {
                control_artwork(state, true)
            })),
            Self::PlainIcons => Some(ShellControlPresenter::new(|state, _, _| {
                control_artwork(state, false)
            })),
        }
    }
}

/// Lab artwork: a small SVG icon, optionally on a filled circle. The circle and
/// icon are deliberately smaller than the shell's clickable target.
fn control_artwork(state: &ShellControlState, filled: bool) -> AnyElement {
    let icon = match state.control {
        ShellControl::Minimize => "shell/minimize.svg",
        ShellControl::Maximize => "shell/maximize.svg",
        ShellControl::Restore => "shell/restore.svg",
        ShellControl::Close => "shell/close.svg",
    };
    let colors = state.colors;
    let dark = matches!(
        state.appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    );
    let active = state.hovered || state.pressed;
    // High contrast: invert on hover/press using the shell's contrast tokens.
    let (circle, icon_color) = if state.high_contrast {
        if active {
            (Some(colors.text), colors.header)
        } else {
            (filled.then_some(colors.header), colors.text)
        }
    } else {
        let rest = if dark { 0x4a4a4a } else { 0xdadada };
        let hover = if dark { 0x5e5e5e } else { 0xc8c8c8 };
        let pressed = if dark { 0x8a8a8a } else { 0x9a9a9a };
        let circle = match (filled, state.hovered, state.pressed) {
            (_, _, true) => Some(pressed),
            (_, true, _) => Some(hover),
            // Inactive windows keep only the icon, like native caption buttons.
            (true, _, _) if state.window_active => Some(rest),
            _ => None,
        };
        (circle, colors.text)
    };
    let diameter = px(24. * state.scale);
    div()
        .size(diameter)
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when_some(circle, |s, color| s.bg(rgb(color)))
        .when(state.high_contrast, |s| {
            s.border_1().border_color(rgb(colors.text))
        })
        .when(!state.window_active && !active, |s| s.opacity(0.6))
        .child(
            svg()
                .path(icon)
                .size(px(12. * state.scale))
                .text_color(rgb(icon_color)),
        )
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypographyPreset {
    SystemUi,
    Large,
    Serif,
}
impl TypographyPreset {
    fn typography(self) -> ShellTypography {
        match self {
            Self::SystemUi => ShellTypography::default(),
            Self::Large => ShellTypography {
                font: font(".SystemUIFont").bold(),
                size: 24.0,
            },
            Self::Serif => ShellTypography {
                font: font("Georgia"),
                size: 18.0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PreviewMode {
    #[default]
    Floating,
    Maximized,
    Tiled,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PreviewLayout {
    #[default]
    Platform,
    CloseOnly,
    Left,
}

fn linux_window_operations(platform: PlatformInfo) -> bool {
    platform.target == PlatformTarget::Native
        && matches!(
            platform.os,
            OperatingSystem::Linux | OperatingSystem::FreeBsd
        )
}

static NEXT_WINDOW: AtomicUsize = AtomicUsize::new(1);

struct Lab {
    id: usize,
    overrides: Overrides,
    requested: WindowDecorations,
    long_title: bool,
    show_platform_readings: bool,
    preview_mode: PreviewMode,
    preview_appearance: Option<WindowAppearance>,
    preview_active: bool,
    preview_focus: bool,
    preview_hover: bool,
    preview_layout: PreviewLayout,
    artwork: ControlArtwork,
    events: VecDeque<String>,
    _subscriptions: Vec<Subscription>,
    session: String,
}

impl Lab {
    fn new(
        id: usize,
        requested: WindowDecorations,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![
            cx.observe_window_bounds(window, |this, _, cx| {
                this.record("Window bounds/state changed");
                cx.notify();
            }),
            cx.observe_window_activation(window, |this, _, cx| {
                this.record("Window activation changed");
                cx.notify();
            }),
        ];
        let weak = cx.entity().downgrade();
        subscriptions.push(cx.observe_desktop_preferences(move |app| {
            let _ = weak.update(app, |this, cx| {
                this.record("Detected desktop preferences changed");
                cx.notify();
            });
        }));
        let weak = cx.entity().downgrade();
        subscriptions.push(window.observe_window_appearance(move |_, app| {
            let _ = weak.update(app, |this, cx| {
                this.record("Window appearance/decorations changed");
                cx.notify();
            });
        }));
        let weak = cx.entity().downgrade();
        subscriptions.push(window.observe_button_layout_changed(move |_, app| {
            let _ = weak.update(app, |this, cx| {
                this.record("Window button layout changed");
                cx.notify();
            });
        }));
        let weak = cx.entity().downgrade();
        subscriptions.push(window.observe_window_display(move |_, app| {
            let _ = weak.update(app, |this, cx| {
                this.record("Display/scaling changed");
                cx.notify();
            });
        }));
        Self {
            id,
            overrides: Overrides::default(),
            requested,
            long_title: false,
            show_platform_readings: true,
            preview_mode: PreviewMode::default(),
            preview_appearance: None,
            preview_active: true,
            preview_focus: true,
            preview_hover: false,
            preview_layout: PreviewLayout::default(),
            artwork: ControlArtwork::default(),
            events: VecDeque::from(["Lab opened; following desktop preferences".into()]),
            _subscriptions: subscriptions,
            session: format!(
                "Desktop: {} · Session: {} · Platform: {:?}",
                std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "not reported".into()),
                std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "not reported".into()),
                cx.platform_info()
            ),
        }
    }

    /// Return the simulated preview and the control artwork to their defaults.
    fn reset_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview_mode = PreviewMode::default();
        self.preview_appearance = None;
        self.preview_active = true;
        self.preview_focus = true;
        self.preview_hover = false;
        self.preview_layout = PreviewLayout::default();
        self.artwork = ControlArtwork::default();
        if let Some(root) = window.root::<ShellWindow>().flatten() {
            root.update(cx, |shell, cx| shell.set_control_presenter(None, cx));
        }
    }

    fn record(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.events.back() != Some(&message) {
            self.events.push_back(message);
            while self.events.len() > 12 {
                self.events.pop_front();
            }
        }
    }

    fn dispatch(
        &mut self,
        action: Action,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, Action::Reset) {
            self.reset_preview(window, cx);
        }
        if self.overrides.apply(action) {
            if let Some(root) = window.root::<ShellWindow>().flatten() {
                root.update(cx, |shell, cx| {
                    shell.set_options(self.overrides.shell_options(), cx)
                });
                self.record(format!("Applied {action:?} to window {}", self.id));
            } else {
                self.record("Cannot apply overrides: this window does not use xgpui-shell");
            }
            cx.notify();
            return;
        }
        let controls = window.window_controls();
        match action {
            Action::TogglePlatformReadings => {
                self.show_platform_readings = !self.show_platform_readings;
                self.record("Toggled platform readings using an app-owned title-bar control");
            }
            Action::Refresh => {
                self.record("Refreshed displayed snapshot (does not force a portal read)")
            }
            Action::LongTitle => {
                self.long_title = !self.long_title;
                let title = if self.long_title {
                    format!(
                        "Shell Lab {} — A deliberately long document title to test centered titles, truncation and access to window controls",
                        self.id
                    )
                } else {
                    format!("XGPUI Shell Lab — Window {}", self.id)
                };
                window.set_shell_title(&title);
                self.record("Changed this window's title");
            }
            Action::Maximize if controls.maximize && window.is_resizable() => {
                window.zoom_window();
                self.record("Requested maximize/restore");
            }
            Action::Minimize if controls.minimize && window.is_minimizable() => {
                window.minimize_window();
                self.record("Requested minimize; restore through the desktop");
            }
            Action::Fullscreen if controls.fullscreen => {
                window.toggle_fullscreen();
                self.record("Requested fullscreen toggle");
            }
            Action::WindowMenu
                if linux_window_operations(cx.platform_info()) && controls.window_menu =>
            {
                window.show_window_menu(position)
            }
            Action::CompactSize | Action::NormalSize
                if window.is_resizable() && !window.is_fullscreen() && !window.is_maximized() =>
            {
                let dimensions = if matches!(action, Action::CompactSize) {
                    size(px(580.), px(500.))
                } else {
                    size(px(1060.), px(820.))
                };
                window.resize(dimensions);
                self.record("Requested windowed size; compositor may constrain it");
            }
            Action::RequestDecorations(mode) if linux_window_operations(cx.platform_info()) => {
                self.requested = mode;
                window.request_decorations(mode);
                self.record(format!(
                    "Requested {mode:?} decorations; inspect the platform-reported mode"
                ));
            }
            Action::NewWindow { fixed } => match open_lab(cx, self.requested, fixed) {
                Ok(_) => self.record("Opened an independent window with system defaults"),
                Err(error) => self.record(format!("Could not open window: {error:#}")),
            },
            Action::PreviewMode(mode) => {
                self.preview_mode = mode;
                self.record("Changed simulated preview state only");
            }
            Action::PreviewAppearance(appearance) => {
                self.preview_appearance = appearance;
                self.record("Changed simulated preview appearance only");
            }
            Action::PreviewActive(active) => {
                self.preview_active = active;
                self.record("Changed simulated preview activation only");
            }
            Action::PreviewHover(hover) => {
                self.preview_hover = hover;
                self.record("Changed simulated preview hover only");
            }
            Action::PreviewFocus(focus) => {
                self.preview_focus = focus;
                self.record("Changed simulated preview focus only");
            }
            Action::PreviewLayout(layout) => {
                self.preview_layout = layout;
                self.record("Changed simulated preview button layout only");
            }
            Action::ControlArtwork(artwork) => {
                self.artwork = artwork;
                if let Some(root) = window.root::<ShellWindow>().flatten() {
                    root.update(cx, |shell, cx| {
                        shell.set_control_presenter(artwork.presenter(), cx)
                    });
                    self.record(format!(
                        "Window-control artwork: {artwork:?} (this window and the preview)"
                    ));
                } else {
                    self.record("Cannot set artwork: this window does not use xgpui-shell");
                }
            }
            _ => self.record("Operation unavailable in this window state"),
        }
        cx.notify();
    }

    fn button(
        &self,
        id: impl Into<ElementId>,
        label: &str,
        selected: bool,
        enabled: bool,
        action: Action,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity();
        let handler = Rc::new(move |position, window: &mut Window, app: &mut App| {
            entity.update(app, |this, cx| this.dispatch(action, position, window, cx));
        });
        let click = handler.clone();
        let keyboard = handler.clone();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(if enabled {
                label.to_owned()
            } else {
                format!("{label} (unavailable)")
            })
            .focusable()
            .tab_stop(enabled)
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(if selected { 0x60a5fa } else { 0x475569 }))
            .bg(rgb(if selected { 0x1e40af } else { 0x1e293b }))
            .when(enabled, |s| {
                s.cursor_pointer().hover(|s| s.bg(rgb(0x334155)))
            })
            .when(!enabled, |s| s.opacity(0.45))
            .focus_visible(|s| s.border_color(rgb(0xfbbf24)))
            .on_click(move |event, window, cx| {
                if enabled {
                    click(event.position(), window, cx);
                }
            })
            .on_key_down(move |event, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if enabled
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    && !event.is_held
                    && !modifiers.control
                    && !modifiers.alt
                    && !modifiers.platform
                {
                    cx.stop_propagation();
                    keyboard(point(px(20.), px(20.)), window, cx);
                }
            })
            .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                if enabled {
                    handler(point(px(20.), px(20.)), window, cx);
                }
            })
            .child(label.to_owned())
            .into_any_element()
    }

    fn preference(
        &self,
        name: &str,
        detected: String,
        effective: String,
        choices: Vec<AnyElement>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .py_3()
            .border_b_1()
            .border_color(rgb(0x334155))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(name.to_owned()),
            )
            .child(format!(
                "Detected: {detected}  ·  Resolved client frame: {effective}"
            ))
            .child(div().flex().flex_wrap().gap_2().children(choices))
            .into_any_element()
    }
    fn platform_preferences(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let desktop = cx.desktop_preferences();
        let coverage = if linux_window_operations(cx.platform_info()) {
            "Linux portal preferences plus optional GNOME/KDE fallbacks. Not reported means unavailable, unsupported or still pending; it does not mean the OS has no preference."
        } else {
            "Desktop preference detection is currently implemented only for Linux. On this platform, accent/font/contrast/motion/title-bar action detection is not implemented. Appearance and actual window state are available separately. The app bar and preview use fallback values or your overrides."
        };
        section(
            "Platform preferences — actual readings",
            [
                div().child(coverage).into_any_element(),
                property("Platform", format!("{:?}", cx.platform_info())),
                property("Appearance", format!("{:?}", window.appearance())),
                property("Platform button layout", layout(cx.button_layout())),
                property("Accent", detected(desktop.accent_color)),
                property("High contrast", detected(desktop.high_contrast)),
                property("Reduced motion", detected(desktop.reduced_motion)),
                property("Font description", detected(desktop.font)),
                property("Text scale", detected(desktop.text_scale)),
                property(
                    "Title-bar actions",
                    format!(
                        "double {}; middle {}; right {}",
                        detected(desktop.titlebar_double_click),
                        detected(desktop.titlebar_middle_click),
                        detected(desktop.titlebar_right_click)
                    ),
                ),
                property(
                    "Native decoration theme",
                    "Not exposed by this API. The OS/window manager draws native/server decorations; the client frame uses xgpui-shell's look.",
                ),
            ],
        )
    }

    fn look_inspector(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let desktop = cx.desktop_preferences();
        let resolved = self
            .overrides
            .shell_options()
            .resolved_preferences(&desktop);
        let look = resolved.resolved_look(
            self.preview_appearance
                .unwrap_or_else(|| window.appearance()),
            self.preview_active,
        );
        let font_source = if self.overrides.typography.is_some() {
            "explicit override"
        } else if desktop
            .font
            .as_deref()
            .and_then(ShellTypography::from_desktop_description)
            .is_some()
        {
            "desktop description"
        } else {
            "built-in fallback (missing or unsupported description)"
        };
        let colors = [
            ("Header", look.colors.header),
            ("Text", look.colors.text),
            ("Border", look.colors.border),
            ("Hover", look.colors.hover),
            ("Focus", look.focus),
        ];
        let swatches = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(colors.into_iter().map(|(name, color)| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(18.))
                            .bg(rgb(color))
                            .border_1()
                            .border_color(rgb(0x94a3b8)),
                    )
                    .child(format!("{name} #{color:06X}"))
            }))
            .into_any_element();
        section("How the look is applied", [
            div().child("Detected preferences → this window's overrides → resolved tokens → client-frame elements. These tokens match the simulated appearance/activation selected above.").into_any_element(),
            swatches,
            property("Title typography requested", format!("{} · {:.1} logical px · weight {} · {:?}; source: {font_source}", look.typography.font.family, look.typography.size, look.typography.font.weight, look.typography.font.style)),
            property("Font availability", "Requested family; the text system may substitute an installed font. This inspector does not claim an exact matched face."),
            property("Title and controls", format!("Header {:.1}px; button target {:.1}px. Logical scale {:.0}% is applied once; device DPI is separate. Icons keep a stable system font or vector shape.", look.titlebar_height, look.button_size, resolved.text_scale * 100.)),
            property("Focus and hover", "Header/text/border follow appearance, activation and contrast. Accent colors keyboard focus; high contrast uses foreground instead. Hover changes the button background instantly. Use the simulated close icon above."),
            property("Title-bar behavior", format!("Double {:?}; middle {:?}; right {:?}. Real dispatch checks window capabilities; preview controls never dispatch.", resolved.double_click_action, resolved.middle_click_action, resolved.right_click_action)),
            property("Reduced motion", format!("{} · resolved policy only. This frame has no animation; hover/focus changes are already instantaneous.", look.reduced_motion)),
            property("Application content", "Keeps its own fonts, colors and behavior. The sample body and lab controls are styled independently; shell preferences style the shell header and client-frame chrome. Native/server decorations remain controlled by the OS."),
        ])
    }

    fn preview(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let appearance = self
            .preview_appearance
            .unwrap_or_else(|| window.appearance());
        let dark = matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let sample = div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(if dark { 0x17202a } else { 0xfafafa }))
            .text_color(rgb(if dark { 0xe2e8f0 } else { 0x1e293b }))
            .child("Application content — preview only");
        let mut preview =
            ClientFramePreview::new(window.shell_title(), self.overrides.shell_options(), sample);
        preview.appearance = appearance;
        preview.active = self.preview_active;
        preview.maximized = self.preview_mode == PreviewMode::Maximized;
        preview.fullscreen = self.preview_mode == PreviewMode::Fullscreen;
        preview.show_focus = self.preview_focus;
        preview.show_hover = self.preview_hover;
        preview.control_presenter = self.artwork.presenter();
        preview.tiling = if self.preview_mode == PreviewMode::Tiled {
            Tiling {
                top: true,
                left: true,
                bottom: true,
                right: false,
            }
        } else {
            Tiling::default()
        };
        preview.button_layout = match self.preview_layout {
            PreviewLayout::Platform => None,
            PreviewLayout::CloseOnly => Some(WindowButtonLayout {
                left: [None; 3],
                right: [Some(WindowButton::Close), None, None],
            }),
            PreviewLayout::Left => Some(WindowButtonLayout {
                left: [
                    Some(WindowButton::Close),
                    Some(WindowButton::Minimize),
                    Some(WindowButton::Maximize),
                ],
                right: [None; 3],
            }),
        };
        let mut controls = Vec::new();
        for (i, mode) in [
            PreviewMode::Floating,
            PreviewMode::Maximized,
            PreviewMode::Tiled,
            PreviewMode::Fullscreen,
        ]
        .into_iter()
        .enumerate()
        {
            controls.push(self.button(
                ("preview-mode", i),
                &format!("{mode:?}"),
                self.preview_mode == mode,
                true,
                Action::PreviewMode(if self.preview_mode == mode {
                    PreviewMode::default()
                } else {
                    mode
                }),
                cx,
            ));
        }
        for (i, (name, appearance)) in [
            ("Native appearance", None),
            ("Light", Some(WindowAppearance::Light)),
            ("Dark", Some(WindowAppearance::Dark)),
        ]
        .into_iter()
        .enumerate()
        {
            controls.push(self.button(
                ("preview-appearance", i),
                name,
                self.preview_appearance == appearance,
                true,
                Action::PreviewAppearance(if self.preview_appearance == appearance {
                    None
                } else {
                    appearance
                }),
                cx,
            ));
        }
        controls.push(self.button(
            "preview-active",
            if self.preview_active {
                "Active preview"
            } else {
                "Inactive preview"
            },
            self.preview_active,
            true,
            Action::PreviewActive(!self.preview_active),
            cx,
        ));
        controls.push(self.button(
            "preview-focus",
            "Simulate Close focus",
            self.preview_focus,
            self.preview_mode != PreviewMode::Fullscreen,
            Action::PreviewFocus(!self.preview_focus),
            cx,
        ));
        controls.push(self.button(
            "preview-hover",
            "Simulate Close hover",
            self.preview_hover,
            self.preview_mode != PreviewMode::Fullscreen,
            Action::PreviewHover(!self.preview_hover),
            cx,
        ));
        for (i, (name, layout)) in [
            ("Platform / fallback buttons", PreviewLayout::Platform),
            ("Close only", PreviewLayout::CloseOnly),
            ("Buttons on left", PreviewLayout::Left),
        ]
        .into_iter()
        .enumerate()
        {
            controls.push(self.button(
                ("preview-layout", i),
                name,
                self.preview_layout == layout,
                true,
                Action::PreviewLayout(if self.preview_layout == layout {
                    PreviewLayout::default()
                } else {
                    layout
                }),
                cx,
            ));
        }
        for (i, (name, artwork)) in [
            ("Built-in controls", ControlArtwork::Builtin),
            ("SVG on filled circle", ControlArtwork::FilledCircle),
            ("SVG, no circle", ControlArtwork::PlainIcons),
        ]
        .into_iter()
        .enumerate()
        {
            controls.push(self.button(
                ("control-artwork", i),
                name,
                self.artwork == artwork,
                true,
                Action::ControlArtwork(if self.artwork == artwork {
                    ControlArtwork::default()
                } else {
                    artwork
                }),
                cx,
            ));
        }
        let look = self
            .overrides
            .shell_options()
            .resolved_preferences(&cx.desktop_preferences())
            .resolved_look(appearance, self.preview_active);
        section("Client-frame preview — simulation", [
            div().child("Visual preview of the shared client-frame renderer. State, appearance and button-layout choices here are simulated; detected preferences and your native window are unchanged. Frame buttons, dragging and resizing are non-interactive.").into_any_element(),
            div().flex().flex_wrap().gap_2().children(controls).into_any_element(),
            property("Close hover simulation", if self.preview_mode == PreviewMode::Fullscreen { "No title bar in Fullscreen. Choose Floating, Maximized or Tiled.".to_owned() } else { format!("{} — only the Close icon background changes: header #{:06X} → hover #{:06X}. This is a simulated state, not an enable/disable switch for mouse input.", if self.preview_hover { "ON" } else { "OFF" }, look.colors.header, look.colors.hover) }),
            property("Title-bar icons", match self.artwork {
                ControlArtwork::Builtin => "xgpui-shell artwork: vector X, fixed minimize/maximize/restore symbols. Platform preferences supply button placement and availability, not icon artwork. These are not macOS traffic lights or the desktop's native decoration theme.",
                ControlArtwork::FilledCircle => "App presenter (ShellControlPresenter): lab SVG icons on a filled circle, applied to this window's shell-drawn controls and the preview. The shell still owns order, availability, activation and the focus ring.",
                ControlArtwork::PlainIcons => "App presenter with the circle treatment removed: the same SVG icons, with a circle only on hover/press.",
            }),
            div().id("client-frame-preview").w_full().h(px(look.titlebar_height + 140.)).min_h(px(180.)).flex_shrink_0().mt_2().child(preview).into_any_element(),
        ])
    }
}

fn property(label: &str, value: impl Into<String>) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap_2()
        .py_1()
        .child(
            div()
                .w(px(170.))
                .text_color(rgb(0x94a3b8))
                .child(label.to_owned()),
        )
        .child(value.into())
        .into_any_element()
}

fn section(title: &str, rows: impl IntoIterator<Item = AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap_1()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(0x334155))
        .child(
            div()
                .text_size(px(19.))
                .font_weight(FontWeight::SEMIBOLD)
                .mb_2()
                .child(title.to_owned()),
        )
        .children(rows)
        .into_any_element()
}

fn detected<T: std::fmt::Debug>(value: Option<T>) -> String {
    value
        .map(|v| format!("{v:?}"))
        .unwrap_or_else(|| "not reported".into())
}

fn layout(value: Option<WindowButtonLayout>) -> String {
    value
        .map(|v| format!("left {:?} · right {:?}", v.left, v.right))
        .unwrap_or_else(|| "not reported".into())
}

impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let desktop = cx.desktop_preferences();
        let resolved = self
            .overrides
            .shell_options()
            .resolved_preferences(&desktop);
        let controls = window.window_controls();
        let mut preferences = Vec::new();
        let contrast = [
            ("System", None),
            ("Normal", Some(false)),
            ("High contrast", Some(true)),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (label, value))| {
            self.button(
                ("contrast", i),
                label,
                self.overrides.contrast == value,
                true,
                Action::Contrast(if self.overrides.contrast == value {
                    None
                } else {
                    value
                }),
                cx,
            )
        })
        .collect();
        preferences.push(self.preference(
            "Contrast",
            detected(desktop.high_contrast),
            resolved.high_contrast.to_string(),
            contrast,
        ));
        let accent = [
            ("System", None),
            ("Blue", Some(0x3584e4)),
            ("Orange", Some(0xf97316)),
            ("Black", Some(0)),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (label, value))| {
            self.button(
                ("accent", i),
                label,
                self.overrides.accent == value,
                true,
                Action::Accent(if self.overrides.accent == value {
                    None
                } else {
                    value
                }),
                cx,
            )
        })
        .collect();
        preferences.push(self.preference(
            "Focus accent",
            detected(desktop.accent_color),
            format!("#{:06X}", resolved.accent),
            accent,
        ));
        let scale = [
            ("System", None),
            ("100%", Some(1.)),
            ("125%", Some(1.25)),
            ("150%", Some(1.5)),
            ("200%", Some(2.)),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (label, value))| {
            self.button(
                ("scale", i),
                label,
                self.overrides.scale == value,
                true,
                Action::Scale(if self.overrides.scale == value {
                    None
                } else {
                    value
                }),
                cx,
            )
        })
        .collect();
        preferences.push(self.preference(
            "Logical text/target scale",
            detected(desktop.text_scale),
            format!("{:.0}%", resolved.text_scale * 100.),
            scale,
        ));
        for (i, kind, name, raw, current, effective) in [
            (
                0,
                Click::Double,
                "Title-bar double-click",
                desktop.titlebar_double_click,
                self.overrides.double_click,
                resolved.double_click_action,
            ),
            (
                1,
                Click::Middle,
                "Title-bar middle-click",
                desktop.titlebar_middle_click,
                self.overrides.middle_click,
                resolved.middle_click_action,
            ),
            (
                2,
                Click::Right,
                "Title-bar right-click",
                desktop.titlebar_right_click,
                self.overrides.right_click,
                resolved.right_click_action,
            ),
        ] {
            let choices = [
                ("System", None),
                ("Maximize/restore", Some(TitlebarAction::ToggleMaximize)),
                ("Minimize", Some(TitlebarAction::Minimize)),
                ("Menu", Some(TitlebarAction::Menu)),
                ("None", Some(TitlebarAction::None)),
            ]
            .into_iter()
            .enumerate()
            .map(|(j, (label, value))| {
                self.button(
                    ("titlebar", i * 10 + j),
                    label,
                    current == value,
                    true,
                    Action::Titlebar(kind, if current == value { None } else { value }),
                    cx,
                )
            })
            .collect();
            preferences.push(self.preference(
                name,
                detected(raw),
                format!("{effective:?}"),
                choices,
            ));
        }
        let typography = [
            ("System", None),
            ("System UI 14px", Some(TypographyPreset::SystemUi)),
            ("Large bold 24px", Some(TypographyPreset::Large)),
            ("Georgia 18px", Some(TypographyPreset::Serif)),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, value))| {
            self.button(
                ("typography", i),
                name,
                self.overrides.typography == value,
                true,
                Action::Typography(if self.overrides.typography == value {
                    None
                } else {
                    value
                }),
                cx,
            )
        })
        .collect();
        preferences.push(self.preference(
            "Frame typography",
            detected(desktop.font.clone()),
            format!(
                "{} · {:.1}px before logical scaling · {:?} · weight {}",
                resolved.typography.font.family,
                resolved.typography.size,
                resolved.typography.font.style,
                resolved.typography.font.weight
            ),
            typography,
        ));
        let motion = [
            ("System", None),
            ("Normal", Some(false)),
            ("Reduce", Some(true)),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, value))| {
            self.button(
                ("motion", i),
                name,
                self.overrides.reduced_motion == value,
                true,
                Action::ReducedMotion(if self.overrides.reduced_motion == value {
                    None
                } else {
                    value
                }),
                cx,
            )
        })
        .collect();
        preferences.push(self.preference(
            "Reduced motion policy (frame has no animation)",
            detected(desktop.reduced_motion),
            resolved.reduced_motion.to_string(),
            motion,
        ));
        let native = !linux_window_operations(cx.platform_info());
        let frame_note = if native {
            "The real title/app bar above is composed by this app. macOS retains native traffic lights; Windows uses shell window controls. The panel below previews Linux frame defaults. Desktop preference detection, Window Menu and decoration requests remain unsupported here."
        } else if window.is_fullscreen() {
            "Fullscreen: the client frame is hidden. Exit fullscreen to inspect its styling."
        } else if matches!(window.window_decorations(), Decorations::Server) {
            "Server decorations: the window manager draws the frame. Request client decorations to inspect xgpui-shell (the compositor may decline)."
        } else {
            "Client decorations: per-window overrides apply to the xgpui-shell frame. Desktop settings are never modified."
        };
        let state = section(
            "Window state — reported by the platform",
            [
                property("Decoration request", format!("{:?}", self.requested)),
                property(
                    "Decoration mode",
                    format!("{:?}", window.window_decorations()),
                ),
                property("Appearance", format!("{:?}", window.appearance())),
                property("Button layout", layout(cx.button_layout())),
                property(
                    "Active / maximized",
                    format!("{} / {}", window.is_window_active(), window.is_maximized()),
                ),
                property(
                    "Fullscreen / resizable",
                    format!("{} / {}", window.is_fullscreen(), window.is_resizable()),
                ),
                property("Backend capabilities", format!("{controls:?}")),
                property(
                    "Window menu in this lab",
                    if controls.window_menu && !native {
                        "available"
                    } else {
                        "unsupported on this platform"
                    },
                ),
                property("Window bounds", format!("{:?}", window.window_bounds())),
                property("Content viewport", format!("{:?}", window.viewport_size())),
                property(
                    "Device scale",
                    format!(
                        "{:.2}× (separate from logical text scale)",
                        window.scale_factor()
                    ),
                ),
            ],
        );
        let actions = [
            (
                "Maximize/restore",
                Action::Maximize,
                controls.maximize && window.is_resizable(),
            ),
            (
                "Minimize",
                Action::Minimize,
                controls.minimize && window.is_minimizable(),
            ),
            (
                if window.is_fullscreen() {
                    "Exit fullscreen"
                } else {
                    "Fullscreen"
                },
                Action::Fullscreen,
                controls.fullscreen,
            ),
            (
                "Window menu",
                Action::WindowMenu,
                controls.window_menu && !native,
            ),
            (
                "Compact size",
                Action::CompactSize,
                window.is_resizable() && !window.is_maximized() && !window.is_fullscreen(),
            ),
            (
                "Normal size",
                Action::NormalSize,
                window.is_resizable() && !window.is_maximized() && !window.is_fullscreen(),
            ),
            ("Toggle long title", Action::LongTitle, true),
            ("Refresh view", Action::Refresh, true),
            ("New window", Action::NewWindow { fixed: false }, true),
            (
                "New fixed-size window",
                Action::NewWindow { fixed: true },
                true,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, action, enabled))| {
            self.button(("action", i), name, false, enabled, action, cx)
        })
        .collect::<Vec<_>>();
        let decoration_buttons = [WindowDecorations::Server, WindowDecorations::Client]
            .into_iter()
            .enumerate()
            .map(|(i, mode)| {
                self.button(
                    ("decoration", i),
                    &format!("Request {mode:?}"),
                    self.requested == mode,
                    !native,
                    Action::RequestDecorations(mode),
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let operations = section("Window operations", [
            div().flex().flex_wrap().gap_2().children(actions).into_any_element(),
            div().flex().flex_wrap().gap_2().mt_2().children(decoration_buttons).into_any_element(),
            div().mt_2().child("Tiling is controlled by your desktop. Restore minimized windows through the desktop. New windows start with independent system defaults.").into_any_element(),
        ]);
        // Keep the preview visible while independently scrolling the override controls.
        div()
            .id("shell-lab")
            .size_full()
            .bg(rgb(0x0f172a))
            .text_color(rgb(0xe2e8f0))
            .text_size(px(13.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .text_size(px(26.))
                            .font_weight(FontWeight::BOLD)
                            .child(format!("Shell Lab — Window {}", self.id)),
                    )
                    .child(self.button(
                        "reset-top",
                        "Reset everything",
                        false,
                        true,
                        Action::Reset,
                        cx,
                    )),
            )
            .child(self.session.clone())
            .child(div().p_3().bg(rgb(0x1e293b)).rounded_md().child(frame_note))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .child(
                        div()
                            .id("preview-pane")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(self.preview(window, cx))
                            .child(self.look_inspector(window, cx))
                            .child(state)
                            .child(section(
                                "Recent events",
                                self.events.iter().rev().map(|event| {
                                    div().py_1().child(event.clone()).into_any_element()
                                }),
                            )),
                    )
                    .child(
                        div()
                            .id("controls-pane")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .when(self.show_platform_readings, |s| {
                                s.child(self.platform_preferences(window, cx))
                            })
                            .child(operations)
                            .child(section(
                                "Detected preferences and per-window overrides",
                                preferences,
                            ))
                            .child(self.button(
                                "reset",
                                "Reset everything (overrides, preview and artwork)",
                                false,
                                true,
                                Action::Reset,
                                cx,
                            )),
                    ),
            )
    }
}

fn open_lab(
    cx: &mut App,
    decorations: WindowDecorations,
    fixed: bool,
) -> Result<(WindowHandle<ShellWindow>, Entity<Lab>)> {
    let id = NEXT_WINDOW.fetch_add(1, Ordering::Relaxed);
    let mut content = None;
    let handle = cx.open_shell_window_with_titlebar(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1060.), px(820.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(format!("XGPUI Shell Lab — Window {id}").into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(500.), px(400.))),
            is_resizable: !fixed,
            window_decorations: Some(decorations),
            app_id: Some("dev.xgpui.ShellLab".into()),
            ..Default::default()
        },
        ShellOptions::default(),
        |window, cx| {
            let lab = cx.new(|cx| Lab::new(id, decorations, window, cx));
            content = Some(lab.clone());
            let titlebar = cx.new(|cx| LabTitleBar::new(lab.clone(), cx));
            (titlebar, lab)
        },
    )?;
    Ok((
        handle,
        content.context("shell content was not constructed")?,
    ))
}

fn main() {
    env_logger::init();
    let options = Options::parse();
    gpui_platform::application()
        .with_assets(HeaderAssets)
        .run(move |cx| {
            bind_focus_keys(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let decorations =
                if options.client_decorations && linux_window_operations(cx.platform_info()) {
                    WindowDecorations::Client
                } else {
                    WindowDecorations::Server
                };
            if let Err(error) = open_lab(cx, decorations, options.fixed_size) {
                eprintln!("Could not open shell lab: {error:#}");
                cx.quit();
            } else {
                cx.activate(true);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[gpui::test]
    fn app_titlebar_toggle_is_clickable_and_does_not_change_window_bounds(cx: &mut TestAppContext) {
        cx.set_platform_info(PlatformInfo {
            os: OperatingSystem::Mac,
            target: PlatformTarget::Native,
        });
        let (handle, lab) =
            cx.update(|app| open_lab(app, WindowDecorations::Server, false).unwrap());
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, app| window.draw(app).clear(app));
        let button = visual
            .debug_bounds("header-panel")
            .expect("app titlebar panel button");
        let before = visual.update(|window, _| window.window_bounds());
        visual.simulate_click(button.center(), Modifiers::default());
        visual.read(|app| assert!(!lab.read(app).show_platform_readings));
        assert_eq!(visual.update(|window, _| window.window_bounds()), before);
        visual.simulate_click(button.center(), Modifiers::default());
        visual.read(|app| assert!(lab.read(app).show_platform_readings));
    }

    #[test]
    fn typography_and_motion_overrides_reset_without_changing_another_window() {
        let mut one = Overrides::default();
        let two = Overrides::default();
        assert!(one.apply(Action::Typography(Some(TypographyPreset::Large))));
        assert!(one.apply(Action::ReducedMotion(Some(false))));
        let desktop = DesktopPreferences {
            reduced_motion: Some(true),
            ..Default::default()
        };
        let look = one
            .shell_options()
            .resolved_preferences(&desktop)
            .resolved_look(WindowAppearance::Light, true);
        assert_eq!(look.typography.size, 24.0);
        assert!(!look.reduced_motion);
        assert_eq!(two, Overrides::default());
        assert!(one.apply(Action::Reset));
        assert_eq!(one, two);
        assert!(
            one.shell_options()
                .resolved_preferences(&desktop)
                .reduced_motion
        );
    }

    #[test]
    fn linux_only_operations_are_unavailable_for_mac_and_browser_backends() {
        let native = |os| PlatformInfo {
            os,
            target: PlatformTarget::Native,
        };
        assert!(!linux_window_operations(native(OperatingSystem::Mac)));
        assert!(!linux_window_operations(native(OperatingSystem::Windows)));
        assert!(linux_window_operations(native(OperatingSystem::Linux)));
        assert!(linux_window_operations(native(OperatingSystem::FreeBsd)));
        assert!(!linux_window_operations(PlatformInfo {
            os: OperatingSystem::Linux,
            target: PlatformTarget::Web
        }));
    }

    #[gpui::test]
    fn mac_dispatch_guards_and_preview_simulation_preserve_native_state(cx: &mut TestAppContext) {
        cx.set_platform_info(PlatformInfo {
            os: OperatingSystem::Mac,
            target: PlatformTarget::Native,
        });
        let (handle, content) =
            cx.update(|app| open_lab(app, WindowDecorations::Server, false).unwrap());
        cx.update_window(*handle, |_, window, app| {
            let before = (
                window.window_bounds(),
                window.window_decorations(),
                window.client_inset(),
                window.is_maximized(),
                window.is_fullscreen(),
                app.desktop_preferences(),
            );
            content.update(app, |lab, cx| {
                lab.dispatch(Action::WindowMenu, Point::default(), window, cx);
                assert_eq!(
                    lab.events.back().unwrap(),
                    "Operation unavailable in this window state"
                );
                lab.dispatch(
                    Action::RequestDecorations(WindowDecorations::Client),
                    Point::default(),
                    window,
                    cx,
                );
                assert_eq!(lab.requested, WindowDecorations::Server);
                lab.dispatch(
                    Action::PreviewMode(PreviewMode::Maximized),
                    Point::default(),
                    window,
                    cx,
                );
                lab.dispatch(
                    Action::PreviewAppearance(Some(WindowAppearance::Dark)),
                    Point::default(),
                    window,
                    cx,
                );
                lab.dispatch(Action::PreviewActive(false), Point::default(), window, cx);
                lab.dispatch(
                    Action::PreviewLayout(PreviewLayout::Left),
                    Point::default(),
                    window,
                    cx,
                );
                assert_eq!(lab.preview_mode, PreviewMode::Maximized);
                assert_eq!(lab.preview_appearance, Some(WindowAppearance::Dark));
                assert!(!lab.preview_active);
                assert_eq!(lab.preview_layout, PreviewLayout::Left);
            });
            assert_eq!(
                before,
                (
                    window.window_bounds(),
                    window.window_decorations(),
                    window.client_inset(),
                    window.is_maximized(),
                    window.is_fullscreen(),
                    app.desktop_preferences()
                )
            );
        })
        .unwrap();
    }

    #[gpui::test]
    fn overrides_and_reset_are_local_to_the_selected_window(cx: &mut TestAppContext) {
        let ((first, one), (_, two)) = cx.update(|app| {
            (
                open_lab(app, WindowDecorations::Client, false).unwrap(),
                open_lab(app, WindowDecorations::Server, false).unwrap(),
            )
        });
        cx.update_window(*first, |_, window, app| {
            one.update(app, |lab, cx| {
                lab.dispatch(Action::Contrast(Some(true)), Point::default(), window, cx)
            });
        })
        .unwrap();
        cx.read(|app| {
            assert_eq!(one.read(app).overrides.contrast, Some(true));
            assert_eq!(two.read(app).overrides, Overrides::default());
        });
        cx.update_window(*first, |_, window, app| {
            one.update(app, |lab, cx| {
                lab.dispatch(Action::Reset, Point::default(), window, cx)
            });
        })
        .unwrap();
        cx.read(|app| assert_eq!(one.read(app).overrides, Overrides::default()));
    }

    #[gpui::test]
    fn fixed_windows_do_not_dispatch_resize_or_maximize(cx: &mut TestAppContext) {
        let (handle, content) =
            cx.update(|app| open_lab(app, WindowDecorations::Server, true).unwrap());
        cx.update_window(*handle, |_, window, app| {
            assert!(!window.is_resizable());
            content.update(app, |lab, cx| {
                lab.dispatch(Action::Maximize, Point::default(), window, cx);
                assert_eq!(
                    lab.events.back().unwrap(),
                    "Operation unavailable in this window state"
                );
                lab.dispatch(Action::CompactSize, Point::default(), window, cx);
                assert_eq!(
                    lab.events.back().unwrap(),
                    "Operation unavailable in this window state"
                );
            });
        })
        .unwrap();
    }
}
