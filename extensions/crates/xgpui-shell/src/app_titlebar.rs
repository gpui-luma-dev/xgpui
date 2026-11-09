use super::ShellOptions;
use gpui::{prelude::*, *};

pub(super) fn configure_window(options: &mut WindowOptions) {
    configure_for_platform(
        options,
        if cfg!(target_os = "macos") {
            OperatingSystem::Mac
        } else if cfg!(target_os = "windows") {
            OperatingSystem::Windows
        } else {
            OperatingSystem::Linux
        },
    );
}

fn configure_for_platform(options: &mut WindowOptions, os: OperatingSystem) {
    if matches!(os, OperatingSystem::Mac | OperatingSystem::Windows) {
        let titlebar = options
            .titlebar
            .get_or_insert_with(TitlebarOptions::default);
        titlebar.appears_transparent = true;
        if os == OperatingSystem::Mac {
            titlebar.traffic_light_position = Some(point(px(9.), px(12.)));
            options.app_owns_titlebar_drag = true;
        }
    }
}

/// An explicit drag surface inside an app-composed title bar.
/// Place title text or empty space here; keep buttons/inputs as siblings so their
/// events never initiate a window move. Fill available space with a styled child.
/// Double click honors native macOS behavior or resolved desktop title-bar actions.
#[derive(IntoElement)]
pub struct ShellDragRegion {
    id: ElementId,
    content: AnyElement,
    options: ShellOptions,
}
impl ShellDragRegion {
    pub fn new(id: impl Into<ElementId>, content: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            content: content.into_any_element(),
            options: ShellOptions::default(),
        }
    }
    /// Match the surrounding window's title-bar action overrides, if any.
    pub fn options(mut self, options: ShellOptions) -> Self {
        self.options = options;
        self
    }
}
struct DragState(Option<Point<Pixels>>);
impl Render for DragState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
impl RenderOnce for ShellDragRegion {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let actions = self.options.resolved_preferences(&cx.desktop_preferences());
        let state = window.use_state(cx, |_, _| DragState(None));
        let mac = cx.platform_info().os == OperatingSystem::Mac;
        let native_double_click = mac && self.options.double_click_action.is_none();
        let windows = cx.platform_info().os == OperatingSystem::Windows;
        div()
            .id(self.id)
            .flex_1()
            .min_w_0()
            .h_full()
            .when(windows, |s| s.window_control_area(WindowControlArea::Drag))
            .when(!windows, |s| {
                s.on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(
                        &state,
                        move |state, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            if event.click_count == 2 {
                                state.0 = None;
                                if native_double_click {
                                    window.titlebar_double_click();
                                } else {
                                    actions
                                        .double_click_action
                                        .perform_at(window, event.position);
                                }
                            } else {
                                state.0 = Some(event.position);
                            }
                        },
                    ),
                )
                .on_mouse_move(window.listener_for(
                    &state,
                    |state, event: &MouseMoveEvent, window, _| {
                        if !event.dragging() {
                            state.0 = None;
                            return;
                        }
                        if let Some(origin) = state.0 {
                            let delta = event.position - origin;
                            if delta.x.abs() >= px(4.) || delta.y.abs() >= px(4.) {
                                state.0 = None;
                                window.start_window_move();
                            }
                        }
                    },
                ))
                .on_mouse_up(
                    MouseButton::Left,
                    window.listener_for(&state, |state, _, _, _| state.0 = None),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    window.listener_for(&state, |state, _, _, _| state.0 = None),
                )
                .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| state.0 = None))
                .on_mouse_down(MouseButton::Middle, move |event, window, cx| {
                    cx.stop_propagation();
                    actions
                        .middle_click_action
                        .perform_at(window, event.position);
                })
                .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                    cx.stop_propagation();
                    actions
                        .right_click_action
                        .perform_at(window, event.position);
                })
            })
            .child(self.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn mac_composition_keeps_native_controls_and_title_while_claiming_drag() {
        let mut options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("App title".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        configure_for_platform(&mut options, OperatingSystem::Mac);
        let titlebar = options.titlebar.unwrap();
        assert_eq!(titlebar.title.as_deref(), Some("App title"));
        assert!(titlebar.appears_transparent);
        assert!(titlebar.traffic_light_position.is_some());
        assert!(options.app_owns_titlebar_drag);
    }
    #[::core::prelude::v1::test]
    fn linux_preserves_decoration_negotiation_and_windows_requests_custom_chrome() {
        let mut linux = WindowOptions {
            window_decorations: Some(WindowDecorations::Server),
            ..Default::default()
        };
        configure_for_platform(&mut linux, OperatingSystem::Linux);
        assert_eq!(linux.window_decorations, Some(WindowDecorations::Server));
        assert!(!linux.titlebar.unwrap().appears_transparent);
        let mut windows = WindowOptions::default();
        configure_for_platform(&mut windows, OperatingSystem::Windows);
        assert!(windows.titlebar.unwrap().appears_transparent);
        assert!(!windows.app_owns_titlebar_drag);
    }
}
