//! Real macOS window probe for #284 package 06: CAMetalDisplayLink delivery.
//!
//! - An animating window must render well above 30 fps (the old `nextDrawable`
//!   stall capped animation at 30 fps).
//! - An idle window must stop receiving frames (the link pauses).
//! - `render_to_image` on a live window must succeed (`nextDrawable` aborts
//!   while a display link drives the layer).
//!
//! Frame limits are disabled so CI focus or power state can't throttle it.
#[cfg(target_os = "macos")]
fn main() {
    use gpui::{prelude::*, *};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        time::{Duration, Instant},
    };

    struct Probe {
        animate: Rc<Cell<bool>>,
        frames: Rc<RefCell<Vec<Instant>>>,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.frames.borrow_mut().push(Instant::now());
            let animate = self.animate.get();
            if animate {
                window.request_animation_frame();
            }
            let n = self.frames.borrow().len();
            div()
                .size_full()
                .bg(if n % 2 == 0 { rgb(0x224466) } else { rgb(0x446622) })
        }
    }

    gpui_platform::application().run(move |cx| {
        let animate = Rc::new(Cell::new(true));
        let frames: Rc<RefCell<Vec<Instant>>> = Rc::default();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(80.), px(80.)),
                        size(px(480.), px(320.)),
                    ))),
                    inactive_frame_interval: None,
                    low_power_frame_interval: None,
                    thermal_frame_interval: None,
                    ..Default::default()
                },
                {
                    let (animate, frames) = (animate.clone(), frames.clone());
                    move |_, cx| cx.new(|_| Probe { animate, frames })
                },
            )
            .expect("open real macOS window");
        cx.activate(true);
        cx.spawn(async move |cx| {
            let executor = cx.background_executor().clone();
            let sleep = move |ms| executor.timer(Duration::from_millis(ms));
            let count_since = |since: Instant| frames.borrow().iter().filter(|t| **t >= since).count();

            // 0. Wait for the first frame. A freshly built binary's first window can
            //    take seconds to appear, which must not count as a slow frame rate.
            let mut waited = 0;
            while frames.borrow().is_empty() {
                if waited >= 15_000 {
                    eprintln!("macOS frame delivery probe failed: the window never rendered a frame in 15 s");
                    std::process::exit(1);
                }
                sleep(100).await;
                waited += 100;
            }

            // 1. Animate: warm up, then measure two seconds.
            sleep(750).await;
            let start = Instant::now();
            sleep(2000).await;
            let animated = count_since(start);
            let fps = animated as f64 / start.elapsed().as_secs_f64();

            // 2. Idle: stop requesting frames; renders must stop.
            animate.set(false);
            sleep(500).await;
            let idle_start = Instant::now();
            sleep(1500).await;
            let idle = count_since(idle_start);

            // 3. Screenshot a live, display-link-driven window.
            let image = window
                .update(cx, |_, window, _| window.render_to_image())
                .map_err(|e| format!("window gone: {e:#}"))
                .and_then(|result| result.map_err(|e| format!("render_to_image failed: {e:#}")));

            let result = if fps < 45.0 {
                Err(format!(
                    "animation ran at {fps:.1} fps ({animated} frames in 2 s); expected the display rate, not a ~30 fps cap"
                ))
            } else if idle > 2 {
                Err(format!("idle window kept rendering: {idle} frames in 1.5 s"))
            } else {
                match image {
                    Err(error) => Err(error),
                    Ok(image) if image.width() == 0 || image.height() == 0 => {
                        Err("render_to_image returned an empty image".into())
                    }
                    Ok(image) => Ok(format!(
                        "{fps:.1} fps animating, {idle} frames idle, live screenshot {}x{}",
                        image.width(),
                        image.height()
                    )),
                }
            };
            // Report and exit here: on macOS `quit` terminates the process, so
            // code after `run` never executes and could not fail the test.
            match result {
                Ok(summary) => {
                    println!("macOS frame delivery probe passed: {summary}");
                    std::process::exit(0);
                }
                Err(error) => {
                    eprintln!("macOS frame delivery probe failed: {error}");
                    std::process::exit(1);
                }
            }
        })
        .detach();
    });
    panic!("macOS probe ended without a verdict");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("macos_frames requires macOS");
}
