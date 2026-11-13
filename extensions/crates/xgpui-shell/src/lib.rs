//! Experimental desktop shell with app-composed title bars and optional Linux client frames.
//! macOS retains native traffic lights; apps supply their own title-bar content and look.
use gpui::{prelude::*, *};
mod app_titlebar;
mod controls;
mod typography;
pub use app_titlebar::ShellDragRegion;
pub use controls::{ShellControl, ShellControlPresenter, ShellControlState};
pub use typography::ShellTypography;

use frame::*;
use std::{cell::RefCell, collections::HashMap};

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
    /// Supply an app-owned title-bar view and a separate content view. Native macOS
    /// traffic lights remain native; client-frame window controls remain shell-owned.
    /// Compose arbitrary app controls and use ShellDragRegion only for drag surfaces.
    fn open_shell_window_with_titlebar<H: Render + 'static, V: Render + 'static>(
        &mut self,
        options: WindowOptions,
        shell: ShellOptions,
        build: impl FnOnce(&mut Window, &mut App) -> (Entity<H>, Entity<V>),
    ) -> anyhow::Result<WindowHandle<ShellWindow>>;
}
impl ShellApp for App {
    fn open_shell_window<V: Render + 'static>(
        &mut self,
        options: WindowOptions,
        shell: ShellOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<WindowHandle<ShellWindow>> {
        open_shell(self, options, shell, |window, cx| {
            (None, build(window, cx).into())
        })
    }

    fn open_shell_window_with_titlebar<H: Render + 'static, V: Render + 'static>(
        &mut self,
        mut options: WindowOptions,
        shell: ShellOptions,
        build: impl FnOnce(&mut Window, &mut App) -> (Entity<H>, Entity<V>),
    ) -> anyhow::Result<WindowHandle<ShellWindow>> {
        app_titlebar::configure_window(&mut options);
        open_shell(self, options, shell, |window, cx| {
            let (titlebar, content) = build(window, cx);
            (Some(titlebar.into()), content.into())
        })
    }
}

fn open_shell(
    app: &mut App,
    options: WindowOptions,
    shell: ShellOptions,
    build: impl FnOnce(&mut Window, &mut App) -> (Option<AnyView>, AnyView),
) -> anyhow::Result<WindowHandle<ShellWindow>> {
    let show_chrome = options.show;
    let initial_title = options
        .titlebar
        .as_ref()
        .and_then(|titlebar| titlebar.title.clone());
    app.open_window(options, |window, cx| {
        if let Some(title) = initial_title {
            window.set_shell_title(&title);
        }
        let (titlebar, content) = build(window, cx);
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
                presenter: None,
                content,
                titlebar,
                show_chrome,
                window_id: window.window_handle().window_id(),
            }
        })
    })
}

pub struct ShellWindow {
    _preferences_subscription: Subscription,
    shell: ShellOptions,
    presenter: Option<ShellControlPresenter>,
    content: AnyView,
    titlebar: Option<AnyView>,
    show_chrome: bool,
    window_id: WindowId,
}
impl ShellWindow {
    /// Replace application-supplied overrides and redraw this window.
    pub fn set_options(&mut self, options: ShellOptions, cx: &mut Context<Self>) {
        self.shell = options;
        cx.notify();
    }

    /// Replace the artwork of shell-drawn window controls, or pass `None` for the
    /// built-in look. Applies wherever the shell draws controls (Linux client
    /// frames and Windows app-composed title bars); native macOS traffic lights
    /// and compositor decorations are unaffected. Set it right after opening the
    /// window to have it in place before the first frame.
    pub fn set_control_presenter(
        &mut self,
        presenter: Option<ShellControlPresenter>,
        cx: &mut Context<Self>,
    ) {
        self.presenter = presenter;
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
            && matches!(
                cx.platform_info().os,
                OperatingSystem::Linux | OperatingSystem::FreeBsd
            )
            && !window.is_fullscreen()
            && let Decorations::Client { .. } = window.window_decorations()
        {
            let mut state = ClientFrameState::for_window(&self.shell, window, cx);
            state.presenter = self.presenter.clone();
            window.set_client_inset(if state.maximized { px(0.) } else { px(5.) });
            return render_client_frame(
                state,
                content.into_any_element(),
                self.titlebar.clone().map(IntoElement::into_any_element),
                true,
                window,
                cx,
            );
        }
        // Clear any prior client frame when switching to server chrome/fullscreen.
        window.set_client_inset(px(0.));
        if self.show_chrome
            && let Some(titlebar) = self.titlebar.clone()
        {
            let mut state = ClientFrameState::for_window(&self.shell, window, cx);
            state.presenter = self.presenter.clone();
            let mac_native = cx.platform_info().os == OperatingSystem::Mac;
            let custom_windows = cx.platform_info().os == OperatingSystem::Windows;
            if mac_native || !custom_windows || window.is_fullscreen() {
                state.button_layout = WindowButtonLayout {
                    left: [None; 3],
                    right: [None; 3],
                };
            }
            #[cfg(target_os = "macos")]
            if mac_native && !window.is_fullscreen() {
                let height = state
                    .preferences
                    .resolved_look(state.appearance, state.active)
                    .titlebar_height;
                window.set_traffic_light_position(point(px(9.), px((height - 16.).max(0.) / 2.)));
            }
            let fullscreen = window.is_fullscreen();
            let header = render_titlebar(
                &state,
                Some(titlebar.into_any_element()),
                true,
                mac_native && !fullscreen,
                window,
                cx,
            );
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(header)
                .child(div().flex_1().min_h_0().child(content))
                .into_any_element();
        }
        content.into_any_element()
    }
}

struct ClientFrameState {
    preferences: ResolvedShellPreferences,
    appearance: WindowAppearance,
    active: bool,
    maximized: bool,
    tiling: Tiling,
    controls: WindowControls,
    resizable: bool,
    button_layout: WindowButtonLayout,
    title: SharedString,
    preview_focus: bool,
    preview_hover: bool,
    presenter: Option<ShellControlPresenter>,
}

impl ClientFrameState {
    fn for_window(options: &ShellOptions, window: &Window, cx: &App) -> Self {
        let tiling = match window.window_decorations() {
            Decorations::Client { tiling } => tiling,
            Decorations::Server => Tiling::default(),
        };
        let mut controls = window.window_controls();
        controls.minimize &= window.is_minimizable();
        controls.maximize &= window.is_resizable();
        Self {
            preferences: options.resolved_preferences(&cx.desktop_preferences()),
            appearance: window.appearance(),
            active: window.is_window_active(),
            maximized: window.is_maximized(),
            tiling,
            controls,
            resizable: window.is_resizable(),
            button_layout: cx.button_layout().unwrap_or_else(fallback_button_layout),
            title: window.shell_title().into(),
            preview_focus: false,
            preview_hover: false,
            presenter: None,
        }
    }
}

fn fallback_button_layout() -> WindowButtonLayout {
    WindowButtonLayout {
        left: [None; MAX_BUTTONS_PER_SIDE],
        right: [
            Some(WindowButton::Minimize),
            Some(WindowButton::Maximize),
            Some(WindowButton::Close),
        ],
    }
}

/// A visual-only client-frame preview inside an application's content area.
///
/// Uses the production frame renderer and resolver, but attaches no window actions,
/// drag/resize regions or focusable frame buttons. It never changes the host window's
/// decorations, insets or state. Simulated state is not detected desktop information.
#[derive(IntoElement)]
pub struct ClientFramePreview {
    /// Displayed preview title.
    pub title: SharedString,
    /// Per-window overrides to preview against the current desktop snapshot.
    pub options: ShellOptions,
    /// Simulated appearance, independent of native window appearance.
    pub appearance: WindowAppearance,
    /// Simulated activation state.
    pub active: bool,
    /// Simulated maximization state.
    pub maximized: bool,
    /// Hide preview chrome, as a real fullscreen client frame does.
    pub fullscreen: bool,
    /// Simulated tiled edges.
    pub tiling: Tiling,
    /// Preview button layout; None uses the current platform layout or the shell fallback.
    pub button_layout: Option<WindowButtonLayout>,
    /// Visually simulate keyboard focus on the close button without making it actionable.
    pub show_focus: bool,
    /// Visually simulate hovering over close, without a native window action.
    pub show_hover: bool,
    /// Draw controls with the same artwork as a real window; they stay inert.
    pub control_presenter: Option<ShellControlPresenter>,
    content: AnyElement,
}

impl ClientFramePreview {
    /// Construct an active, floating, light preview. Give its parent a bounded size.
    pub fn new(
        title: impl Into<SharedString>,
        options: ShellOptions,
        content: impl IntoElement,
    ) -> Self {
        Self {
            title: title.into(),
            options,
            appearance: WindowAppearance::Light,
            active: true,
            maximized: false,
            fullscreen: false,
            tiling: Tiling::default(),
            button_layout: None,
            show_focus: false,
            show_hover: false,
            control_presenter: None,
            content: content.into_any_element(),
        }
    }
}

impl RenderOnce for ClientFramePreview {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.fullscreen {
            return self.content;
        }
        render_client_frame(
            ClientFrameState {
                preferences: self.options.resolved_preferences(&cx.desktop_preferences()),
                appearance: self.appearance,
                active: self.active,
                maximized: self.maximized,
                tiling: self.tiling,
                controls: WindowControls::default(),
                resizable: true,
                button_layout: self
                    .button_layout
                    .or_else(|| cx.button_layout())
                    .unwrap_or_else(fallback_button_layout),
                title: self.title,
                preview_focus: self.show_focus,
                preview_hover: self.show_hover,
                presenter: self.control_presenter,
            },
            self.content,
            None,
            false,
            window,
            cx,
        )
    }
}

fn render_client_frame(
    state: ClientFrameState,
    content: AnyElement,
    titlebar: Option<AnyElement>,
    interactive: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let maximized = state.maximized;
    let tiling = state.tiling;
    let palette = state
        .preferences
        .resolved_look(state.appearance, state.active)
        .colors;
    let inset = if maximized { px(0.) } else { px(5.) };
    let title = render_titlebar(&state, titlebar, interactive, false, window, cx);
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
    if interactive && !maximized && state.resizable {
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
    frame.into_any_element()
}

fn render_titlebar(
    state: &ClientFrameState,
    titlebar: Option<AnyElement>,
    interactive: bool,
    mac_native: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let preferences = &state.preferences;
    let look = preferences.resolved_look(state.appearance, state.active);
    let palette = look.colors;
    let scale = preferences.text_scale;
    let controls = state.controls;
    let left_buttons = visible_buttons(state.button_layout.left, controls);
    let right_buttons = visible_buttons(state.button_layout.right, controls);
    let side_width = px((left_buttons.len().max(right_buttons.len()) as f32 * 33. + 8.) * scale);
    let custom_header = titlebar.is_some();
    let mut buttons = |side: Vec<WindowButton>, right: bool| {
        let mut row = div()
            .flex()
            .items_center()
            .flex_none()
            .min_w(if custom_header {
                px((side.len() as f32 * 33. + 8.) * scale)
            } else {
                side_width
            })
            .when(right, |s| s.justify_end());
        for button in side {
            let preview_focus =
                !interactive && state.preview_focus && button == WindowButton::Close;
            let preview_hover =
                !interactive && state.preview_hover && button == WindowButton::Close;
            let element = if let Some(presenter) = &state.presenter {
                presented_control(
                    presenter,
                    button,
                    state,
                    &look,
                    interactive,
                    (preview_focus, preview_hover),
                    window,
                    cx,
                )
            } else {
                builtin_control(
                    button,
                    state,
                    &look,
                    interactive,
                    (preview_focus, preview_hover),
                )
            };
            row = row.child(element);
        }
        row
    };
    let left = buttons(left_buttons, false);
    let right = buttons(right_buttons, true);
    let title = div()
        .id("shell-titlebar")
        .debug_selector(|| "shell-titlebar-bounds".into())
        .flex_none()
        .h(px(look.titlebar_height))
        .px(px(4. * scale))
        .flex()
        .items_center()
        .bg(rgb(palette.header))
        .text_color(rgb(palette.text))
        .font(look.typography.font.clone())
        .text_size(px(look.typography.size))
        .border_b_1()
        .border_color(rgb(palette.border))
        .child(left)
        .pl(px(if mac_native { 80.0 } else { 0.0 }))
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
                .when(interactive && titlebar.is_none(), |title| {
                    title
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
                })
                .child(titlebar.unwrap_or_else(|| {
                    div()
                        .w_full()
                        .truncate()
                        .text_center()
                        .child(state.title.clone())
                        .into_any_element()
                })),
        )
        .child(right);
    title.into_any_element()
}

/// Shell-owned semantics shared by built-in and presented controls: Windows
/// caption hit-testing, pointer/keyboard/accessibility activation.
fn with_control_behavior(
    element: Stateful<Div>,
    button: WindowButton,
    interactive: bool,
) -> Stateful<Div> {
    element
        .when(interactive && cfg!(target_os = "windows"), |element| {
            element.window_control_area(match button {
                WindowButton::Close => WindowControlArea::Close,
                WindowButton::Minimize => WindowControlArea::Min,
                WindowButton::Maximize => WindowControlArea::Max,
            })
        })
        .when(interactive, |element| {
            element
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
        })
}

fn shell_control(button: WindowButton, maximized: bool) -> ShellControl {
    match button {
        WindowButton::Minimize => ShellControl::Minimize,
        WindowButton::Maximize if maximized => ShellControl::Restore,
        WindowButton::Maximize => ShellControl::Maximize,
        WindowButton::Close => ShellControl::Close,
    }
}

/// Pointer state the shell tracks for presented controls.
struct ControlInteraction {
    hovered: bool,
    pressed: bool,
    focus: FocusHandle,
}

#[allow(clippy::too_many_arguments)]
fn presented_control(
    presenter: &ShellControlPresenter,
    button: WindowButton,
    state: &ClientFrameState,
    look: &ResolvedShellLook,
    interactive: bool,
    (preview_focus, preview_hover): (bool, bool),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let control = shell_control(button, state.maximized);
    let label = control.accessible_label();
    let target_size = px(look.button_size);
    let interaction = interactive.then(|| {
        window.use_keyed_state(
            SharedString::from(format!("shell-control-state-{}", button.id())),
            cx,
            |_, cx| ControlInteraction {
                hovered: false,
                pressed: false,
                focus: cx.focus_handle().tab_stop(true),
            },
        )
    });
    let (hovered, pressed, focused) = match &interaction {
        Some(interaction) => {
            let interaction = interaction.read(cx);
            (
                interaction.hovered,
                interaction.pressed,
                interaction.focus.is_focused(window) && window.last_input_was_keyboard(),
            )
        }
        None => (preview_hover, false, preview_focus),
    };
    let control_state = ShellControlState {
        control,
        hovered,
        pressed,
        focused,
        window_active: state.active,
        appearance: state.appearance,
        high_contrast: state.preferences.high_contrast,
        colors: look.colors,
        focus_color: look.focus,
        scale: state.preferences.text_scale,
        target_size,
        preview: !interactive,
    };
    let artwork = presenter.render(&control_state, window, cx);
    let accent = look.focus;
    // The shell paints only the focus ring; the presenter owns everything inside.
    let mut element = div()
        .id(button.id())
        .debug_selector(move || format!("shell-control-{}", button.id()))
        .role(if interactive {
            Role::Button
        } else {
            Role::Image
        })
        .aria_label(if interactive {
            label.to_owned()
        } else {
            format!("Preview: {label}")
        })
        .size(target_size)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded_full()
        .border_2()
        .border_color(transparent_black())
        .when(focused, |s| s.border_color(rgb(accent)))
        .child(artwork);
    if let Some(interaction) = interaction {
        let focus = interaction.read(cx).focus.clone();
        let (hover_state, down_state, up_state, out_state) = (
            interaction.clone(),
            interaction.clone(),
            interaction.clone(),
            interaction,
        );
        element = element
            .track_focus(&focus)
            .cursor_pointer()
            .tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
            .on_hover(move |hovered, _, cx| {
                hover_state.update(cx, |state, cx| {
                    state.hovered = *hovered;
                    if !*hovered {
                        state.pressed = false;
                    }
                    cx.notify();
                })
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                // Never let a control press start a window drag.
                cx.stop_propagation();
                down_state.update(cx, |state, cx| {
                    state.pressed = true;
                    cx.notify();
                })
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                up_state.update(cx, |state, cx| {
                    state.pressed = false;
                    cx.notify();
                })
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                out_state.update(cx, |state, cx| {
                    state.pressed = false;
                    cx.notify();
                })
            });
    }
    with_control_behavior(element, button, interactive).into_any_element()
}

fn builtin_control(
    button: WindowButton,
    state: &ClientFrameState,
    look: &ResolvedShellLook,
    interactive: bool,
    preview: (bool, bool),
) -> AnyElement {
    let maximized = state.maximized;
    let palette = look.colors;
    let scale = state.preferences.text_scale;
    let label = control_label(button, maximized);
    with_control_behavior(
        control_button(
            button.id(),
            label,
            palette,
            look.focus,
            px(look.button_size),
            interactive,
            preview,
        ),
        button,
        interactive,
    )
    .child(if button == WindowButton::Close {
        // Draw around the geometric center, independent of font baselines.
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let margin = px(2. * scale);
                let start = bounds.origin + point(margin, margin);
                let end = bounds.bottom_right() - point(margin, margin);
                for (a, b) in [(start, end), (point(start.x, end.y), point(end.x, start.y))] {
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
            .font(gpui::font(".SystemUIFont"))
            .text_size(px(14. * scale))
            .child(match button {
                WindowButton::Minimize => "−",
                WindowButton::Maximize if maximized => "▣",
                _ => "□",
            })
            .into_any_element()
    })
    .into_any_element()
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
    /// Override client-frame typography; None parses the desktop description or uses system UI.
    pub typography: Option<ShellTypography>,
    /// Resolve motion policy for consumers; the current frame has no animations.
    pub reduced_motion: Option<bool>,
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
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedShellPreferences {
    /// Whether the frame uses high-contrast colors.
    pub high_contrast: bool,
    /// Encoded sRGB focus accent, 0xRRGGBB.
    pub accent: u32,
    /// Logical text/target scale, clamped to 0.5–4 independently of device DPI.
    pub text_scale: f32,
    /// Unscaled client-frame typography.
    pub typography: ShellTypography,
    /// Resolved policy; no frame animations are currently implemented.
    pub reduced_motion: bool,
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
            typography: self
                .typography
                .clone()
                .or_else(|| {
                    desktop
                        .font
                        .as_deref()
                        .and_then(ShellTypography::from_desktop_description)
                })
                .unwrap_or_default()
                .normalized(),
            reduced_motion: self
                .reduced_motion
                .or(desktop.reduced_motion)
                .unwrap_or(false),
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

/// Client-frame colors after appearance, activation and contrast resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellColors {
    pub header: u32,
    pub text: u32,
    pub border: u32,
    pub hover: u32,
}

/// The exact logical style tokens consumed by the shared frame renderer.
/// Does not theme application content or native/server decorations.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedShellLook {
    pub colors: ShellColors,
    /// Effective focus-ring color (foreground in high contrast).
    pub focus: u32,
    /// Typography with logical text scaling already applied once.
    pub typography: ShellTypography,
    pub titlebar_height: f32,
    pub button_size: f32,
    /// Motion policy only; current hover/focus changes are instantaneous.
    pub reduced_motion: bool,
}

impl ResolvedShellPreferences {
    /// Resolve the same style tokens used by the native client frame and its preview.
    pub fn resolved_look(&self, appearance: WindowAppearance, active: bool) -> ResolvedShellLook {
        let colors = ShellColors::new(appearance, active, self.high_contrast);
        let mut typography = self.typography.clone();
        typography.size *= self.text_scale;
        ResolvedShellLook {
            colors,
            focus: if self.high_contrast {
                colors.text
            } else {
                self.accent
            },
            titlebar_height: (40.0 * self.text_scale)
                .max(typography.size * 1.5 + 12.0 * self.text_scale),
            button_size: 33.0 * self.text_scale,
            typography,
            reduced_motion: self.reduced_motion,
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

mod frame {
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
    pub(super) type Palette = ShellColors;
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
    pub(super) struct ControlTooltip(pub(super) &'static str);
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
        interactive: bool,
        (preview_focus, preview_hover): (bool, bool),
    ) -> Stateful<Div> {
        div()
            .id(id)
            .role(if interactive {
                Role::Button
            } else {
                Role::Image
            })
            .aria_label(if interactive {
                label.to_owned()
            } else {
                format!("Preview: {label}")
            })
            .when(interactive, |s| s.focusable().tab_stop(true))
            .size(size)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_2()
            .border_color(transparent_black())
            .when(preview_focus, |s| s.border_color(rgb(accent)))
            .when(preview_hover, |s| s.bg(rgb(palette.hover)))
            .when(interactive, |s| {
                s.cursor_pointer().hover(move |s| s.bg(rgb(palette.hover)))
            })
            .focus_visible(move |s| s.border_color(rgb(accent)))
            .when(interactive, |s| {
                s.tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            })
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
        fn simulated_hover_reaches_the_control_background_without_enabling_actions() {
            let palette = Palette::new(WindowAppearance::Light, true, false);
            let mut normal = control_button(
                "normal",
                "Close",
                palette,
                0x3584e4,
                px(33.),
                false,
                (false, false),
            );
            let mut hovered = control_button(
                "hovered",
                "Close",
                palette,
                0x3584e4,
                px(33.),
                false,
                (false, true),
            );
            assert!(normal.style().background.is_none());
            assert!(hovered.style().background.is_some());
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
            ..Default::default()
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
    fn look_resolution_applies_typography_scale_once_and_preserves_explicit_policy() {
        let desktop = DesktopPreferences {
            font: Some("Noto Sans 12".into()),
            text_scale: Some(1.5),
            reduced_motion: Some(true),
            ..Default::default()
        };
        let resolved = ShellOptions::default().resolved_preferences(&desktop);
        assert_eq!(resolved.typography.size, 16.0);
        let look = resolved.resolved_look(WindowAppearance::Light, true);
        assert_eq!(look.typography.size, 24.0);
        assert!(look.titlebar_height >= look.typography.size * 1.5);
        assert!(look.reduced_motion);
        let options = ShellOptions {
            typography: Some(ShellTypography {
                font: font("Explicit Family"),
                size: 24.0,
            }),
            reduced_motion: Some(false),
            high_contrast: Some(true),
            accent: Some(0xff0000),
            ..Default::default()
        };
        let look = options
            .resolved_preferences(&desktop)
            .resolved_look(WindowAppearance::Dark, false);
        assert_eq!(look.typography.font.family.as_ref(), "Explicit Family");
        assert_eq!(look.typography.size, 36.0);
        assert_eq!(look.focus, look.colors.text);
        assert!(!look.reduced_motion);
        let reset = ShellOptions::default().resolved_preferences(&DesktopPreferences {
            font: Some("Sans Condensed 12".into()),
            ..Default::default()
        });
        assert_eq!(reset.typography, ShellTypography::default());
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

#[cfg(test)]
mod preview_tests {
    use super::*;
    use ::core::prelude::v1::test;

    struct PreviewHost;
    impl Render for PreviewHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(ClientFramePreview::new(
                "Preview only",
                ShellOptions::default(),
                div().size_full().child("Content"),
            ))
        }
    }

    struct ContentStyleHost(std::rc::Rc<RefCell<Option<SharedString>>>);
    impl Render for ContentStyleHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let observed = self.0.clone();
            div()
                .size_full()
                .font_family("Content Typeface")
                .child(ClientFramePreview::new(
                    "Different chrome typography",
                    ShellOptions {
                        typography: Some(ShellTypography {
                            font: font("Frame Typeface"),
                            size: 30.0,
                        }),
                        ..Default::default()
                    },
                    canvas(
                        move |_, window, _| {
                            *observed.borrow_mut() = Some(window.text_style().font_family);
                        },
                        |_, _, _, _| (),
                    )
                    .size_full(),
                ))
        }
    }

    #[gpui::test]
    fn client_frame_typography_does_not_leak_into_application_content(cx: &mut TestAppContext) {
        let observed = std::rc::Rc::new(RefCell::new(None));
        let copy = observed.clone();
        let host = cx.open_window(size(px(600.), px(240.)), move |_, _| ContentStyleHost(copy));
        let mut test_window = VisualTestContext::from_window(host.into(), cx);
        test_window.update(|window, app| window.draw(app).clear(app));
        assert_eq!(
            observed.borrow().as_ref().map(|s| s.as_ref()),
            Some("Content Typeface")
        );
    }

    #[gpui::test]
    fn preview_pointer_events_do_not_change_or_close_the_host(cx: &mut TestAppContext) {
        let host = cx.open_window(size(px(600.), px(200.)), |_, _| PreviewHost);
        let mut test_window = VisualTestContext::from_window(host.into(), cx);
        test_window.update(|window, app| window.draw(app).clear(app));
        let before = cx
            .update_window(*host, |_, window, _| {
                (
                    window.window_bounds(),
                    window.window_decorations(),
                    window.client_inset(),
                    window.is_maximized(),
                    window.is_fullscreen(),
                )
            })
            .unwrap();
        // The three preview window controls, title drag area, and resize edge must
        // all remain decorative even though they share the real frame's renderer.
        for position in [
            point(px(575.), px(20.)),
            point(px(542.), px(20.)),
            point(px(509.), px(20.)),
            point(px(300.), px(20.)),
            point(px(2.), px(80.)),
        ] {
            test_window.simulate_click(position, Modifiers::default());
        }
        test_window.simulate_event(MouseDownEvent {
            position: point(px(300.), px(20.)),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
            modifiers: Modifiers::default(),
        });
        test_window.simulate_mouse_up(
            point(px(300.), px(20.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        let after = cx
            .update_window(*host, |_, window, _| {
                (
                    window.window_bounds(),
                    window.window_decorations(),
                    window.client_inset(),
                    window.is_maximized(),
                    window.is_fullscreen(),
                )
            })
            .expect("preview close must not close the real window");
        assert_eq!(before, after);
    }
}

#[cfg(test)]
mod composition_tests {
    use super::*;
    use ::core::prelude::v1::test;
    use std::{cell::Cell, rc::Rc};
    struct Header(Rc<Cell<usize>>);
    impl Render for Header {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.0.clone();
            div()
                .size_full()
                .flex()
                .items_center()
                .child(
                    div()
                        .id("app-button")
                        .debug_selector(|| "app-button".into())
                        .w(px(32.))
                        .h_full()
                        .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
                        .child("App"),
                )
                .child(ShellDragRegion::new(
                    "app-drag",
                    div().size_full().child("App title"),
                ))
        }
    }
    struct Body;
    impl Render for Body {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .debug_selector(|| "app-body".into())
                .size_full()
                .child("Body")
        }
    }
    #[gpui::test]
    fn composed_mac_header_reserves_native_control_space_and_preserves_app_clicks(
        cx: &mut TestAppContext,
    ) {
        cx.set_platform_info(PlatformInfo {
            os: OperatingSystem::Mac,
            target: PlatformTarget::Native,
        });
        let clicks = Rc::new(Cell::new(0));
        let copy = clicks.clone();
        let handle = cx.update(|app| {
            app.open_shell_window_with_titlebar(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(600.), px(240.)),
                    })),
                    ..Default::default()
                },
                ShellOptions::default(),
                |_, app| (app.new(|_| Header(copy)), app.new(|_| Body)),
            )
            .unwrap()
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, app| window.draw(app).clear(app));
        let button = visual
            .debug_bounds("app-button")
            .expect("app header rendered");
        let body = visual.debug_bounds("app-body").expect("app body rendered");
        let header = visual.debug_bounds("shell-titlebar-bounds").unwrap();
        assert!(
            button.left() >= px(80.),
            "reserve native traffic-light space"
        );
        assert_eq!(body.top(), header.bottom());
        visual.simulate_click(button.center(), Modifiers::default());
        assert_eq!(clicks.get(), 1);
        assert_eq!(visual.debug_bounds("app-body").unwrap(), body);
    }
}

#[cfg(test)]
mod presenter_tests {
    use super::*;
    use ::core::prelude::v1::test;
    use std::rc::Rc;

    type Seen = Rc<RefCell<Vec<ShellControlState>>>;

    fn recording(seen: &Seen) -> ShellControlPresenter {
        let seen = seen.clone();
        ShellControlPresenter::new(move |state, _, _| {
            seen.borrow_mut().push(state.clone());
            div()
                .debug_selector(|| "app-artwork".into())
                .size(px(16. * state.scale))
                .into_any_element()
        })
    }

    struct Header;
    impl Render for Header {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child("App header")
        }
    }
    struct Body;
    impl Render for Body {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    /// A Windows app-composed title bar: the shell draws min/max/close itself.
    fn open_windows_shell(cx: &mut TestAppContext, seen: &Seen) -> WindowHandle<ShellWindow> {
        cx.set_platform_info(PlatformInfo {
            os: OperatingSystem::Windows,
            target: PlatformTarget::Native,
        });
        let presenter = recording(seen);
        let handle = cx.update(|app| {
            app.open_shell_window_with_titlebar(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(600.), px(240.)),
                    })),
                    ..Default::default()
                },
                ShellOptions::default(),
                |_, app| (app.new(|_| Header), app.new(|_| Body)),
            )
            .unwrap()
        });
        handle
            .update(cx, |shell, _, cx| {
                shell.set_control_presenter(Some(presenter), cx)
            })
            .unwrap();
        handle
    }

    fn last_frame(seen: &Seen, count: usize) -> Vec<ShellControlState> {
        let seen = seen.borrow();
        seen[seen.len() - count..].to_vec()
    }

    #[gpui::test]
    fn presenter_draws_each_visible_control_with_shell_order_and_identity(cx: &mut TestAppContext) {
        let seen = Seen::default();
        let handle = open_windows_shell(cx, &seen);
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, app| window.draw(app).clear(app));
        let frame = last_frame(&seen, 3);
        assert_eq!(
            frame.iter().map(|s| s.control).collect::<Vec<_>>(),
            [
                ShellControl::Minimize,
                ShellControl::Maximize,
                ShellControl::Close
            ]
        );
        for state in &frame {
            assert!(!state.preview && !state.hovered && !state.pressed && !state.focused);
            assert_eq!(state.target_size, px(33.));
            assert_eq!(state.scale, 1.);
        }
        // The built-in glyphs are replaced, not drawn underneath.
        assert!(visual.debug_bounds("app-artwork").is_some());
    }

    #[gpui::test]
    fn hover_and_press_reach_the_presenter(cx: &mut TestAppContext) {
        let seen = Seen::default();
        let handle = open_windows_shell(cx, &seen);
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, app| window.draw(app).clear(app));
        let close = visual.debug_bounds("shell-control-close").unwrap();
        visual.simulate_mouse_move(close.center(), None, Modifiers::default());
        visual.update(|window, app| window.draw(app).clear(app));
        let close_state = last_frame(&seen, 3)[2].clone();
        assert_eq!(close_state.control, ShellControl::Close);
        assert!(close_state.hovered);
        assert!(!last_frame(&seen, 3)[0].hovered, "only Close is hovered");
        visual.simulate_mouse_down(close.center(), MouseButton::Left, Modifiers::default());
        visual.update(|window, app| window.draw(app).clear(app));
        assert!(last_frame(&seen, 3)[2].pressed);
    }

    #[gpui::test]
    fn presented_close_activates_once_without_starting_a_window_move(cx: &mut TestAppContext) {
        let seen = Seen::default();
        let handle = open_windows_shell(cx, &seen);
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, app| window.draw(app).clear(app));
        let close = visual.debug_bounds("shell-control-close").unwrap();
        // The test window panics on start_window_move, so a drag would fail here.
        visual.simulate_click(close.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(
            handle.update(cx, |_, _, _| ()).is_err(),
            "close removed the window"
        );
    }

    #[gpui::test]
    fn removing_the_presenter_restores_the_builtin_controls(cx: &mut TestAppContext) {
        let seen = Seen::default();
        let handle = open_windows_shell(cx, &seen);
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, app| window.draw(app).clear(app));
        assert!(visual.debug_bounds("app-artwork").is_some());
        handle
            .update(cx, |shell, _, cx| shell.set_control_presenter(None, cx))
            .unwrap();
        let before = seen.borrow().len();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, app| window.draw(app).clear(app));
        assert_eq!(seen.borrow().len(), before, "presenter no longer called");
        assert!(visual.debug_bounds("app-artwork").is_none());
    }

    struct PreviewHost(ShellControlPresenter);
    impl Render for PreviewHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let mut preview =
                ClientFramePreview::new("Preview", ShellOptions::default(), div().size_full());
            preview.maximized = true;
            preview.show_hover = true;
            preview.show_focus = true;
            preview.control_presenter = Some(self.0.clone());
            div().size_full().child(preview)
        }
    }

    #[gpui::test]
    fn preview_uses_the_presenter_with_simulated_state_and_stays_inert(cx: &mut TestAppContext) {
        let seen = Seen::default();
        let presenter = recording(&seen);
        let host = cx.open_window(size(px(600.), px(200.)), move |_, _| PreviewHost(presenter));
        let mut visual = VisualTestContext::from_window(host.into(), cx);
        visual.update(|window, app| window.draw(app).clear(app));
        let frame = last_frame(&seen, 3);
        assert_eq!(
            frame.iter().map(|s| s.control).collect::<Vec<_>>(),
            [
                ShellControl::Minimize,
                ShellControl::Restore,
                ShellControl::Close
            ]
        );
        assert!(frame.iter().all(|s| s.preview));
        assert!(frame[2].hovered && frame[2].focused);
        assert!(!frame[0].hovered && !frame[0].focused);
        let close = visual.debug_bounds("shell-control-close").unwrap();
        visual.simulate_click(close.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(
            host.update(cx, |_, _, _| ()).is_ok(),
            "preview close must not close the host"
        );
    }
}
