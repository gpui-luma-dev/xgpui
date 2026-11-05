//! Shared application support for the unpublished native labs.
use gpui::{prelude::*, *};

use std::{cell::RefCell, collections::HashMap};

thread_local! {
    // Platform title reads are macOS-only. Keep the lab chrome's title locally.
    static LAB_TITLES: RefCell<HashMap<WindowId, String>> = RefCell::new(HashMap::new());
}
pub trait LabWindowTitle {
    fn set_lab_title(&mut self, title: &str);
    fn lab_title(&self) -> String;
}
impl LabWindowTitle for Window {
    fn set_lab_title(&mut self, title: &str) {
        self.set_window_title(title);
        LAB_TITLES.with(|titles| {
            titles
                .borrow_mut()
                .insert(self.window_handle().window_id(), title.to_owned())
        });
        self.refresh();
    }
    fn lab_title(&self) -> String {
        LAB_TITLES
            .with(|titles| {
                titles
                    .borrow()
                    .get(&self.window_handle().window_id())
                    .cloned()
            })
            .unwrap_or_default()
    }
}

/// Open a lab with compositor-provided chrome or a shared client-side frame.
pub trait LabApp {
    fn open_lab_window<V: Render + 'static>(
        &mut self,
        options: WindowOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<WindowHandle<LabWindow>>;
}
impl LabApp for App {
    fn open_lab_window<V: Render + 'static>(
        &mut self,
        options: WindowOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<WindowHandle<LabWindow>> {
        let show_chrome = options.show;
        self.open_window(options, |window, cx| {
            let content = build(window, cx).into();
            cx.new(|_| {
                window
                    .observe_button_layout_changed(|window, _| window.refresh())
                    .detach();
                LabWindow {
                    content,
                    show_chrome,
                    window_id: window.window_handle().window_id(),
                }
            })
        })
    }
}

pub struct LabWindow {
    content: AnyView,
    show_chrome: bool,
    window_id: WindowId,
}
impl Drop for LabWindow {
    fn drop(&mut self) {
        let _ = LAB_TITLES.try_with(|titles| titles.borrow_mut().remove(&self.window_id));
    }
}
impl Render for LabWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.content.clone();
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        if self.show_chrome
            && !window.is_fullscreen()
            && let Decorations::Client { tiling } = window.window_decorations()
        {
            let maximized = window.is_maximized();
            let inset = if maximized { px(0.) } else { px(5.) };
            window.set_client_inset(inset);
            let layout = cx
                .button_layout()
                .unwrap_or_else(WindowButtonLayout::linux_default);
            let controls = window.window_controls();
            let buttons = |side: [Option<WindowButton>; MAX_BUTTONS_PER_SIDE]| {
                div()
                    .flex()
                    .items_center()
                    .children(side.into_iter().flatten().filter_map(|button| {
                        if (button == WindowButton::Minimize && !controls.minimize)
                            || (button == WindowButton::Maximize && !controls.maximize)
                        {
                            return None;
                        }
                        Some(
                            div()
                                .id(button.id())
                                .cursor_pointer()
                                .px_3()
                                .py_1()
                                .hover(|s| s.bg(rgb(0x475569)))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    match button {
                                        WindowButton::Minimize => window.minimize_window(),
                                        WindowButton::Maximize => window.zoom_window(),
                                        WindowButton::Close => window.remove_window(),
                                    }
                                })
                                .child(match button {
                                    WindowButton::Minimize => "−",
                                    WindowButton::Maximize => {
                                        if maximized {
                                            "❐"
                                        } else {
                                            "□"
                                        }
                                    }
                                    WindowButton::Close => "×",
                                }),
                        )
                    }))
            };
            let title = div()
                .id("lab-titlebar")
                .flex_none()
                .h(px(34.))
                .flex()
                .items_center()
                .bg(rgb(0x1e293b))
                .text_color(rgb(0xe2e8f0))
                .child(buttons(layout.left))
                .child(
                    div()
                        .id("lab-title-drag")
                        .flex_1()
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(MouseButton::Left, |event, window, _| {
                            if event.click_count == 2 {
                                if window.window_controls().maximize {
                                    window.zoom_window();
                                }
                            } else {
                                window.start_window_move();
                            }
                        })
                        .on_mouse_down(MouseButton::Right, |event, window, _| {
                            window.show_window_menu(event.position)
                        })
                        .child(window.lab_title()),
                )
                .child(buttons(layout.right));
            // The inset handles resizing; content events must not initiate window moves.
            let mut frame = div()
                .id("lab-frame")
                .size_full()
                .relative()
                .bg(rgb(0x475569));
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
                    div().size_full().flex().flex_col().child(title).child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_hidden()
                            .child(content),
                    ),
                );
            if !maximized {
                for (index, (edge, enabled, cursor)) in edges.into_iter().enumerate() {
                    if !enabled {
                        continue;
                    }
                    let region = div()
                        .id(("lab-resize", index))
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
        let _ = (cx, self.show_chrome);
        content.into_any_element()
    }
}

/// Attempt native wide SDR, retaining a usable default presentation on failure.
/// A notice is returned whenever the effective presentation is standard SDR.
pub fn negotiate_wide(window: &mut Window) -> Option<String> {
    negotiate_with(|mode| window.set_color_mode(mode))
}

fn negotiate_with(
    mut set_mode: impl FnMut(WindowColorMode) -> anyhow::Result<WindowColorStatus>,
) -> Option<String> {
    match set_mode(WindowColorMode::WideGamutSdr) {
        Ok(status) if status.effective() == WindowColorMode::WideGamutSdr => None,
        result => {
            let reason = match result {
                Ok(_) => "the backend selected standard SDR".to_owned(),
                Err(error) => error.to_string(),
            };
            eprintln!("Wide-gamut presentation diagnostic: {reason}");
            let fallback = set_mode(WindowColorMode::Default);
            Some(match fallback {
                Ok(_) => "Wide-gamut output isn’t available with the current display and graphics setup. Using standard SDR.".into(),
                Err(error) => {
                    format!("Wide-gamut output isn’t available with the current display and graphics setup. Could not switch to standard SDR: {error}")
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn unsupported_wide_resets_to_sdr_with_a_user_facing_notice() {
        let mut requests = Vec::new();
        let notice = negotiate_with(|mode| {
            requests.push(mode);
            if mode == WindowColorMode::WideGamutSdr {
                anyhow::bail!("unsupported backend");
            }
            Ok(WindowColorStatus::default())
        })
        .unwrap();
        assert_eq!(
            requests,
            [WindowColorMode::WideGamutSdr, WindowColorMode::Default]
        );
        assert!(notice.contains("Using standard SDR"));
        assert!(!notice.contains("unsupported backend"));
    }

    #[::core::prelude::v1::test]
    fn accepted_request_with_effective_sdr_still_reports_fallback() {
        let mut requests = Vec::new();
        let notice = negotiate_with(|mode| {
            requests.push(mode);
            Ok(WindowColorStatus::new(
                mode,
                WindowColorMode::Default,
                true,
                Some(false),
                Some(WindowColorFallback::SdrDisplay),
            ))
        });
        assert!(notice.unwrap().contains("Using standard SDR"));
        assert_eq!(requests.len(), 2);
    }

    #[::core::prelude::v1::test]
    fn supported_wide_remains_enabled_without_an_sdr_request() {
        let mut requests = Vec::new();
        assert!(
            negotiate_with(|mode| {
                requests.push(mode);
                Ok(WindowColorStatus::new(mode, mode, true, Some(true), None))
            })
            .is_none()
        );
        assert_eq!(requests, [WindowColorMode::WideGamutSdr]);
    }

    #[::core::prelude::v1::test]
    fn failed_sdr_request_is_not_reported_as_active_sdr() {
        let notice = negotiate_with(|_| anyhow::bail!("request rejected")).unwrap();
        assert!(notice.contains("Could not switch to standard SDR"));
        assert!(!notice.contains("Using standard SDR"));
    }
}
