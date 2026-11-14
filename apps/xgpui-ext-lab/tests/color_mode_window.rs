//! Real-window probe: a color-mode change requested from `defer_in` inside `render`
//! takes effect and redraws without input (#284).
//!
//! Those callbacks run while the Windows backend is still inside `draw_window`. The
//! backend used to refuse the change there ("cannot change color mode during a
//! Windows draw/transition"), so an app that switches to SDR for captured effects
//! retried every frame and showed its placeholder until a focus change. The request
//! is now queued and applied by the next frame.
#[cfg(target_os = "windows")]
fn main() {
    use gpui::{prelude::*, *};
    use std::{cell::RefCell, rc::Rc, time::Duration};

    #[derive(Default)]
    struct Log {
        calls: Vec<Result<WindowColorStatus, String>>,
        renders_seeing_default: usize,
    }
    struct Probe {
        log: Rc<RefCell<Log>>,
        /// 0: switch to wide SDR, 1: switch back to Default, 2: done.
        stage: u8,
        pending: bool,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let effective = window.color_status().effective();
            if self.stage == 2 && effective == WindowColorMode::Default {
                self.log.borrow_mut().renders_seeing_default += 1;
            }
            let target = match self.stage {
                0 => Some(WindowColorMode::WideGamutSdr),
                1 => Some(WindowColorMode::Default),
                _ => None,
            };
            // Same shape as xgpui-luma's shell: request from a deferred callback,
            // advance on success, notify either way.
            if let Some(target) = target
                && !self.pending
            {
                self.pending = true;
                cx.defer_in(window, move |this, window, cx| {
                    let result = window.set_color_mode(target);
                    if result.is_ok() {
                        this.stage += 1;
                    }
                    this.log
                        .borrow_mut()
                        .calls
                        .push(result.map_err(|error| format!("{error:#}")));
                    this.pending = false;
                    cx.notify();
                });
            }
            div().size_full().bg(if effective == WindowColorMode::Default {
                rgb(0x00ff00)
            } else {
                rgb(0x0000ff)
            })
        }
    }

    fn report(result: Result<String, String>) -> ! {
        match result {
            Ok(summary) => {
                println!("color mode window probe passed: {summary}");
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("color mode window probe failed: {error}");
                std::process::exit(1);
            }
        }
    }

    gpui_platform::application().run(move |cx| {
        let log: Rc<RefCell<Log>> = Rc::default();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(80.), px(80.)),
                        size(px(240.), px(160.)),
                    ))),
                    ..Default::default()
                },
                {
                    let log = log.clone();
                    move |_, cx| {
                        cx.new(|_| Probe {
                            log,
                            stage: 0,
                            pending: false,
                        })
                    }
                },
            )
            .expect("open real window");
        cx.activate(true);
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(3000))
                .await;
            let effective = match window.update(cx, |_, window, _| window.color_status().effective())
            {
                Ok(effective) => effective,
                Err(error) => report(Err(format!("window gone: {error:#}"))),
            };
            let log = log.borrow();
            let failures: Vec<_> = log.calls.iter().filter_map(|c| c.as_ref().err()).collect();
            let summary = format!(
                "{} set_color_mode calls, {} refused, final effective {effective:?}, {} renders saw Default",
                log.calls.len(),
                failures.len(),
                log.renders_seeing_default
            );
            if log.calls.first().is_some_and(|c| c.is_err()) {
                // Wide SDR unavailable on this adapter: nothing to switch back from.
                report(Ok(format!("skipped, wide SDR unavailable: {:?}", log.calls[0])));
            }
            if let Some(error) = failures.first() {
                report(Err(format!("a deferred request was refused ({error}): {summary}")));
            }
            if effective != WindowColorMode::Default {
                report(Err(format!("the switch back to Default never applied: {summary}")));
            }
            if log.renders_seeing_default == 0 {
                report(Err(format!("no frame rendered after the switch: {summary}")));
            }
            report(Ok(summary));
        })
        .detach();
    });
    panic!("color mode window probe ended without a verdict");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("color_mode_window requires Windows");
}
