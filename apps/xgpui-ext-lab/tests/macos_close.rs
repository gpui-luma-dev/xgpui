//! Real-window probe: closing a key window on macOS must not log errors.
//!
//! Closing a window makes AppKit resign its key status. GPUI's macOS backend
//! delivered that activation change from a spawned task, which ran after GPUI
//! had already removed the window and logged
//! `handling active status change for window …: window not found`.
//! The macOS counterpart of `windows_close` (#277). Each window is made key,
//! then closed through one path, with an untouched anchor window keeping the
//! app alive: `performClose:` (the title-bar close button and ⌘W), a plain
//! `close`, and GPUI's `remove_window()`. It fails if any error is logged.
#[cfg(target_os = "macos")]
fn main() {
    use gpui::{prelude::*, *};
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{ptr, sync::Mutex, time::Duration};

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
                println!("macos close probe passed: {summary}");
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("macos close probe failed: {error}");
                std::process::exit(1);
            }
        }
    }

    /// The window's `NSWindow`, as an address (the window outlives this call).
    fn ns_window(window: &Window) -> usize {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)
            .expect("window handle")
            .as_raw()
        else {
            panic!("not an AppKit window");
        };
        let view = handle.ns_view.as_ptr() as *mut AnyObject;
        let window: *mut AnyObject = unsafe { msg_send![view, window] };
        window as usize
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
        let perform_close = open(cx, 260.);
        let plain_close = open(cx, 480.);
        let removed = open(cx, 700.);
        cx.activate(true);

        cx.spawn(async move |cx| {
            let executor = cx.background_executor().clone();
            let pause = |ms| executor.timer(Duration::from_millis(ms));
            pause(800).await;
            let make_key = |handle: WindowHandle<Blank>, cx: &mut AsyncApp| {
                handle
                    .update(cx, |_, window, _| {
                        window.activate_window();
                        ns_window(window)
                    })
                    .expect("window to close")
            };

            // 1. The title-bar close button and ⌘W send performClose:.
            let ns = make_key(perform_close, cx);
            pause(400).await;
            unsafe {
                let _: () =
                    msg_send![ns as *mut AnyObject, performClose: ptr::null_mut::<AnyObject>()];
            }
            pause(500).await;
            // 2. A plain close, as AppleScript and other apps send.
            let ns = make_key(plain_close, cx);
            pause(400).await;
            unsafe {
                let _: () = msg_send![ns as *mut AnyObject, close];
            }
            pause(500).await;
            // 3. GPUI closing the window itself.
            make_key(removed, cx);
            pause(400).await;
            removed
                .update(cx, |_, window, _| window.remove_window())
                .expect("remove window");
            // Give deferred drops and activation reports time to run.
            pause(1500).await;

            let still_open: Vec<_> = [
                ("performClose:", perform_close),
                ("close", plain_close),
                ("remove_window", removed),
            ]
            .into_iter()
            .filter(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok())
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
                "performClose:, close and remove_window() closed with no errors".into(),
            ));
        })
        .detach();
    });
    panic!("macos close probe ended without a verdict");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("macos_close requires macOS");
}
