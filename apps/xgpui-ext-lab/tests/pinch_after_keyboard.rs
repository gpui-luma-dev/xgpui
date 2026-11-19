//! #384: real pinch input must reach the hovered element after keyboard navigation.
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc};

struct Probe {
    focus: FocusHandle,
    count: Rc<Cell<usize>>,
    scheduled: bool,
}
impl Render for Probe {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if !self.scheduled {
            self.scheduled = true;
            let count = self.count.clone();
            window.on_next_frame(move |window, cx| {
                let inside = point(px(100.), px(100.));
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: inside,
                        ..Default::default()
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::KeyDown(KeyDownEvent {
                        keystroke: Keystroke::parse("f").unwrap(),
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
                assert!(window.last_input_was_keyboard());
                window.dispatch_event(
                    PlatformInput::Pinch(PinchEvent {
                        position: inside,
                        delta: 0.2,
                        ..Default::default()
                    }),
                    cx,
                );
                assert_eq!(
                    count.get(),
                    1,
                    "keyboard mode must not swallow pinch on the hovered element"
                );
                window.dispatch_event(
                    PlatformInput::Pinch(PinchEvent {
                        position: point(px(500.), px(500.)),
                        delta: 0.2,
                        ..Default::default()
                    }),
                    cx,
                );
                assert_eq!(
                    count.get(),
                    1,
                    "pinch outside the element must remain ignored"
                );
                println!("pinch-after-keyboard: passed (inside delivered, outside ignored)");
                cx.quit();
            });
            window.request_animation_frame();
        }
        let count = self.count.clone();
        div()
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(0x172238))
            .on_pinch(move |_, _, _| count.set(count.get() + 1))
    }
}
fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(300.), px(240.)),
                    cx,
                ))),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    focus.focus(window, cx);
                    Probe {
                        focus,
                        count: Rc::new(Cell::new(0)),
                        scheduled: false,
                    }
                })
            },
        )
        .expect("open pinch probe");
        cx.activate(true);
    });
}
