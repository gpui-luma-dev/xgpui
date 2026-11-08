//! Interactive diagnostics for the shared shell; no desktop settings are written.
use anyhow::{Context as _, Result};
use clap::Parser;
use gpui::{prelude::*, *};
use std::{
    collections::VecDeque,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};
use xgpui_shell::{ShellApp, ShellOptions, ShellWindow, ShellWindowTitle, TitlebarAction};

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
    Titlebar(Click, Option<TitlebarAction>),
    Reset,
    Refresh,
    LongTitle,
    Maximize,
    Minimize,
    Fullscreen,
    WindowMenu,
    CompactSize,
    NormalSize,
    RequestDecorations(WindowDecorations),
    NewWindow { fixed: bool },
}

static NEXT_WINDOW: AtomicUsize = AtomicUsize::new(1);

struct Lab {
    id: usize,
    overrides: Overrides,
    requested: WindowDecorations,
    long_title: bool,
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
            Action::WindowMenu if controls.window_menu => window.show_window_menu(position),
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
            Action::RequestDecorations(mode) => {
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
                Action::Contrast(value),
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
                Action::Accent(value),
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
                Action::Scale(value),
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
                    Action::Titlebar(kind, value),
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
        preferences.push(property(
            "Font description",
            format!(
                "{} (reported only; not yet applied to frame typography)",
                detected(desktop.font)
            ),
        ));
        preferences.push(property(
            "Reduced motion",
            format!(
                "{} (reported only; this frame has no animation)",
                detected(desktop.reduced_motion)
            ),
        ));
        let native = !cfg!(any(target_os = "linux", target_os = "freebsd"));
        let frame_note = if native {
            "Native platform chrome is retained. Overrides below describe the client frame and do not recolor native window controls."
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
                property("Capabilities", format!("{controls:?}")),
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
            ("Window menu", Action::WindowMenu, controls.window_menu),
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
        div().id("shell-lab").size_full().overflow_y_scroll().bg(rgb(0x0f172a)).text_color(rgb(0xe2e8f0))
            .text_size(px(13.)).p_4().flex().flex_col().gap_4()
            .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).child(format!("Shell Lab — Window {}", self.id)))
            .child(self.session.clone())
            .child(div().p_3().bg(rgb(0x1e293b)).rounded_md().child(frame_note))
            .child(section("Window operations", [
                div().flex().flex_wrap().gap_2().children(actions).into_any_element(),
                div().flex().flex_wrap().gap_2().mt_2().children(decoration_buttons).into_any_element(),
                div().mt_2().child("Tiling is controlled by your desktop: use its snap/tiling gestures. Restore minimized windows through the desktop. New windows start with independent system defaults.").into_any_element(),
            ]))
            .child(state)
            .child(section("Detected preferences and per-window overrides", preferences))
            .child(self.button("reset", "Reset this window to system defaults", false, true, Action::Reset, cx))
            .child(section("Recent events", self.events.iter().rev().map(|event| div().py_1().child(event.clone()).into_any_element())))
    }
}

fn open_lab(
    cx: &mut App,
    decorations: WindowDecorations,
    fixed: bool,
) -> Result<(WindowHandle<ShellWindow>, Entity<Lab>)> {
    let id = NEXT_WINDOW.fetch_add(1, Ordering::Relaxed);
    let mut content = None;
    let handle = cx.open_shell_window(
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
            lab
        },
    )?;
    Ok((
        handle,
        content.context("shell content was not constructed")?,
    ))
}

fn main() {
    let options = Options::parse();
    gpui_platform::application().run(move |cx| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let decorations = if options.client_decorations {
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
