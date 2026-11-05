//! Real display-server smoke check. Run explicitly under Wayland or X11.
#[cfg(target_os = "linux")]
fn main() {
    use gpui::{prelude::*, *};
    use std::{cell::Cell, rc::Rc};
    use xgpui_ext_lab::{LabApp, LabWindowTitle};

    struct Probe {
        frames: usize,
        completed: Rc<Cell<usize>>,
        title: &'static str,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.frames += 1;
            eprintln!("Linux smoke window frame {}", self.frames);
            if self.frames == 1 {
                let title = format!("{} (updated)", self.title);
                window.on_next_frame(move |window, _| window.set_lab_title(&title));
            }
            if self.frames == 3 {
                let completed = self.completed.clone();
                let expected_title = format!("{} (updated)", self.title);
                window.on_next_frame(move |window, cx| {
                    assert!(window.viewport_size().width > px(0.));
                    assert!(window.viewport_size().height > px(0.));
                    assert_eq!(window.lab_title(), expected_title);
                    assert!(
                        !window.painted_quads().is_empty(),
                        "window must paint content"
                    );
                    match window.window_decorations() {
                        Decorations::Client { .. } => {
                            assert!(
                                window.client_inset().is_some_and(|inset| inset > px(0.)),
                                "client-decorated window must have the shared resize frame"
                            );
                        }
                        Decorations::Server => assert_eq!(window.client_inset(), Some(px(0.))),
                    }
                    let count = completed.get() + 1;
                    completed.set(count);
                    window.remove_window();
                    if count == 2 {
                        cx.quit();
                    }
                });
            } else if self.frames < 3 {
                // Repaint the shell as well as the content, independent of element caching.
                window.on_next_frame(|window, _| window.refresh());
            }
            div()
                .size_full()
                .bg(rgb(0x224466))
                .text_color(white())
                .child("Linux lab CI content")
        }
    }
    let completed = Rc::new(Cell::new(0));
    let done = completed.clone();
    gpui_platform::application().run(move |cx| {
        assert!(
            !cx.displays().is_empty(),
            "smoke test needs a real display server"
        );
        for (decorations, title) in [
            (None, "Linux default window"),
            (Some(WindowDecorations::Client), "Linux client window"),
        ] {
            let completed = completed.clone();
            cx.open_lab_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(20.), px(20.)),
                        size(px(480.), px(320.)),
                    ))),
                    window_decorations: decorations,
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_lab_title(title);
                    let notice = xgpui_ext_lab::negotiate_wide(window);
                    let status = window.color_status();
                    assert!(
                        status.effective() == WindowColorMode::WideGamutSdr || notice.is_some(),
                        "fallback must provide a user notice"
                    );
                    cx.new(|_| Probe {
                        frames: 0,
                        completed,
                        title,
                    })
                },
            )
            .expect("open real Linux lab window");
        }
        cx.activate(true);
    });
    assert_eq!(
        done.get(),
        2,
        "both default and client-decorated windows must complete frames"
    );
    println!(
        "Linux display smoke passed: window creation, rendered frames, title, decoration insets and color negotiation"
    );
}

#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("linux_windows requires a Linux display server");
}
