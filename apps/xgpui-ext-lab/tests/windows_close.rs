//! Real-window probe for #277: closing a window must not log errors.
//!
//! Windows destroys the HWND before GPUI drops its window, so anything that still
//! called Win32 with that handle afterwards logged `Invalid window handle`
//! (0x80070578, or `DRAGDROP_E_INVALIDHWND` from `RevokeDragDrop`). This closes one
//! window through each path, with an untouched anchor window keeping the app alive:
//! the system menu's Close (`SC_CLOSE`, sent by the caption X and Alt+F4), a plain
//! `WM_CLOSE`, and GPUI's `remove_window()`. It fails if any error is logged.
#[cfg(target_os = "windows")]
fn main() {
    use gpui::{prelude::*, *};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{sync::Mutex, time::Duration};
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{IsWindow, PostMessageW, SC_CLOSE, WM_CLOSE, WM_SYSCOMMAND},
    };

    /// Records every warning and error the backend logs.
    struct Capture(Mutex<Vec<String>>);
    impl log::Log for Capture {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                let line = format!("{} {}: {}", record.level(), record.target(), record.args());
                eprintln!("{line}");
                if record.level() == log::Level::Error {
                    self.0.lock().unwrap().push(line);
                }
            }
        }
        fn flush(&self) {}
    }
    static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));
    log::set_logger(&CAPTURE).expect("install the capturing logger");
    log::set_max_level(log::LevelFilter::Warn);

    struct Blank;
    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0x202020))
        }
    }

    fn report(result: Result<String, String>) -> ! {
        match result {
            Ok(summary) => {
                println!("windows close probe passed: {summary}");
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("windows close probe failed: {error}");
                std::process::exit(1);
            }
        }
    }

    fn hwnd(window: &Window) -> HWND {
        let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window)
            .expect("window handle")
            .as_raw()
        else {
            panic!("not a Win32 window");
        };
        HWND(handle.hwnd.get() as *mut _)
    }

    gpui_platform::application().run(move |cx| {
        let open = |cx: &mut App, x: f32| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(x), px(80.)),
                        size(px(200.), px(140.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Blank),
            )
            .expect("open real window")
        };
        let anchor = open(cx, 40.);
        let system_close = open(cx, 260.);
        let plain_close = open(cx, 480.);
        let removed = open(cx, 700.);
        cx.activate(true);

        cx.spawn(async move |cx| {
            let executor = cx.background_executor().clone();
            let pause = |ms| executor.timer(Duration::from_millis(ms));
            pause(800).await;
            let system_hwnd = system_close
                .update(cx, |_, window, _| hwnd(window))
                .expect("system-close window");
            let plain_hwnd = plain_close
                .update(cx, |_, window, _| hwnd(window))
                .expect("plain-close window");
            let removed_hwnd = removed
                .update(cx, |_, window, _| hwnd(window))
                .expect("removed window");

            // 1. The caption X and Alt+F4 both arrive as SC_CLOSE.
            unsafe {
                PostMessageW(
                    Some(system_hwnd),
                    WM_SYSCOMMAND,
                    WPARAM(SC_CLOSE as usize),
                    LPARAM(0),
                )
            }
            .expect("post SC_CLOSE");
            pause(500).await;
            // 2. A plain WM_CLOSE, as other apps and the taskbar send.
            unsafe { PostMessageW(Some(plain_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                .expect("post WM_CLOSE");
            pause(500).await;
            // 3. GPUI closing the window itself.
            removed
                .update(cx, |_, window, _| window.remove_window())
                .expect("remove window");
            // Give deferred drops, drag-drop revocation and cursor broadcasts time to run.
            pause(1500).await;

            let still_open: Vec<_> = [
                ("SC_CLOSE", system_hwnd),
                ("WM_CLOSE", plain_hwnd),
                ("remove_window", removed_hwnd),
            ]
            .into_iter()
            .filter(|(_, h)| unsafe { IsWindow(Some(*h)) }.as_bool())
            .map(|(path, _)| path)
            .collect();
            if !still_open.is_empty() {
                report(Err(format!(
                    "windows still open after closing: {still_open:?}"
                )));
            }
            if anchor.update(cx, |_, _, _| ()).is_err() {
                report(Err("the anchor window closed too".into()));
            }
            let errors = CAPTURE.0.lock().unwrap().clone();
            if !errors.is_empty() {
                report(Err(format!(
                    "{} error(s) logged while closing:\n  {}",
                    errors.len(),
                    errors.join("\n  ")
                )));
            }
            report(Ok(
                "SC_CLOSE, WM_CLOSE and remove_window() closed with no errors".into(),
            ));
        })
        .detach();
    });
    panic!("windows close probe ended without a verdict");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("windows_close requires Windows");
}
