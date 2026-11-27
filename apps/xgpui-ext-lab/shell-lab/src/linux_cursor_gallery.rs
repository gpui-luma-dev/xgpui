use super::LinuxCursorBackendChoice;

#[cfg(not(target_os = "linux"))]
pub(super) fn run(_: Option<LinuxCursorBackendChoice>) {
    eprintln!("The Linux cursor gallery requires Linux");
}

#[cfg(target_os = "linux")]
pub(super) use linux::run;

#[cfg(target_os = "linux")]
mod linux {
    use super::LinuxCursorBackendChoice;
    use gpui::{prelude::*, *};
    use gpui_linux::system_cursor::{
        LinuxCursorAccess, LinuxCursorAvailability, LinuxCursorBackend, LinuxSystemCursor,
    };
    use std::{cell::Cell, rc::Rc};

    pub(crate) fn run(backend: Option<LinuxCursorBackendChoice>) {
        let modes = match backend {
            Some(LinuxCursorBackendChoice::Wayland) => WindowingModes::WAYLAND,
            Some(LinuxCursorBackendChoice::X11) => WindowingModes::X11,
            None => WindowingModes::WAYLAND | WindowingModes::X11,
        } | WindowingModes::HEADLESS;
        let (platform, access) = LinuxCursorAccess::create_platform(modes);
        Application::with_platform(platform).run(move |cx| {
            if matches!(access.backend(), LinuxCursorBackend::Headless) {
                eprintln!("No display connection for the Linux cursor gallery");
                cx.quit();
                return;
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let bounds = Bounds::centered(None, size(px(860.), px(700.)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Linux cursor gallery".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| Gallery {
                        access,
                        active: Rc::default(),
                    })
                },
            );
            if let Err(error) = opened {
                eprintln!("Could not open Linux cursor gallery: {error:#}");
                cx.quit();
            } else {
                cx.activate(true);
            }
        });
    }

    struct Gallery {
        access: LinuxCursorAccess,
        active: Rc<Cell<Option<LinuxSystemCursor>>>,
    }

    impl Render for Gallery {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let backend = match self.access.backend() {
                LinuxCursorBackend::WaylandShape => "Wayland · compositor cursor-shape protocol",
                LinuxCursorBackend::WaylandTheme => "Wayland · cursor theme",
                LinuxCursorBackend::X11 => "X11 · Xcursor theme and cursor font",
                _ => "No cursor backend",
            };
            div()
                .id("linux-cursors")
                .size_full()
                .overflow_y_scroll()
                .bg(rgb(0xf6f8fa))
                .text_color(rgb(0x17212b))
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::BOLD)
                        .child("Linux cursor gallery"),
                )
                .child(div().font_weight(FontWeight::SEMIBOLD).child(backend))
                .child("Hover over an available tile to see the cursor chosen by this session.")
                .child(div().flex().flex_wrap().flex_shrink_0().gap_2().children(
                    LinuxSystemCursor::ALL.iter().copied().map(|cursor| {
                        let (available, source) = match self.access.availability(window, cursor) {
                            LinuxCursorAvailability::WaylandShape => {
                                (true, "Wayland protocol shape".to_owned())
                            }
                            LinuxCursorAvailability::XcursorName(name) => {
                                (true, format!("Xcursor · {name}"))
                            }
                            _ => (false, "Unavailable in this session".to_owned()),
                        };
                        let access = self.access.clone();
                        let active = self.active.clone();
                        div()
                            .id(cursor.name())
                            .w(px(196.))
                            .min_h(px(58.))
                            .p_2()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0xc4cfd9))
                            .bg(rgb(0xffffff))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(cursor.name()),
                            )
                            .child(div().text_xs().child(source))
                            .when(available, |tile| {
                                tile.hover(|style| style.bg(rgb(0xe3efff))).on_hover(
                                    move |hovered, window, _| {
                                        let next = if *hovered {
                                            Some(cursor)
                                        } else if active.get() == Some(cursor) {
                                            None
                                        } else {
                                            return;
                                        };
                                        match access.set_for_window(window, next) {
                                            Ok(()) => active.set(next),
                                            Err(error) => {
                                                log::warn!("Could not select {cursor:?}: {error:#}")
                                            }
                                        }
                                    },
                                )
                            })
                    }),
                ))
        }
    }
}
