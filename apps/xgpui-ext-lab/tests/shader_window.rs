//! Real-window probe for xgpui-shader custom paints (DirectX 11 on Windows, Metal on
//! macOS). Opens a window whose whole surface is a red-to-blue linear gradient painted
//! through the platform's custom-draw path, keeps it animating for two seconds, then
//! reads the live window back and checks the gradient is there.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn main() {
    use gpui::{prelude::*, *};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::Arc,
        time::{Duration, Instant},
    };
    use xgpui_shader::{Gradient, GradientKind, GradientStop, Paint, painted};

    struct Probe {
        paint: Paint,
        animate: Rc<Cell<bool>>,
        frames: Rc<RefCell<Vec<Instant>>>,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.frames.borrow_mut().push(Instant::now());
            if self.animate.get() {
                window.request_animation_frame();
            }
            painted().size_full().bg_paint(self.paint.clone())
        }
    }

    fn report(result: Result<String, String>) -> ! {
        match result {
            Ok(summary) => {
                println!("shader window probe passed: {summary}");
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("shader window probe failed: {error}");
                std::process::exit(1);
            }
        }
    }

    let gradient = Gradient::new(
        GradientKind::Linear { angle: 90. },
        vec![
            GradientStop::new(0., [1., 0., 0., 1.]),
            GradientStop::new(1., [0., 0., 1., 1.]),
        ],
    )
    .expect("valid gradient");
    let paint: Paint = Arc::new(gradient).into();

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
                    move |_, cx| {
                        cx.new(|_| Probe {
                            paint,
                            animate,
                            frames,
                        })
                    }
                },
            )
            .expect("open real window");
        cx.activate(true);
        cx.spawn(async move |cx| {
            let executor = cx.background_executor().clone();
            let sleep = move |ms| executor.timer(Duration::from_millis(ms));

            let mut waited = 0;
            while frames.borrow().is_empty() {
                if waited >= 15_000 {
                    report(Err("the window never rendered a frame in 15 s".into()));
                }
                sleep(100).await;
                waited += 100;
            }
            sleep(750).await;
            let start = Instant::now();
            sleep(2000).await;
            let animated = frames.borrow().iter().filter(|t| **t >= start).count();
            let fps = animated as f64 / start.elapsed().as_secs_f64();
            animate.set(false);

            let image = match window.update(cx, |_, window, _| window.render_to_image()) {
                Ok(Ok(image)) => image,
                Ok(Err(error)) => report(Err(format!("render_to_image failed: {error:#}"))),
                Err(error) => report(Err(format!("window gone: {error:#}"))),
            };
            let (w, h) = (image.width(), image.height());
            if w == 0 || h == 0 {
                report(Err("render_to_image returned an empty image".into()));
            }
            let left = image.get_pixel(w / 10, h / 2).0;
            let right = image.get_pixel(w * 9 / 10, h / 2).0;
            let summary = format!(
                "{fps:.1} fps animating ({animated} frames in 2 s); left pixel {left:?}, right pixel {right:?}, image {w}x{h}"
            );
            // Left must be mostly red, right mostly blue.
            let reddish = left[0] > 150 && left[2] < 110;
            let bluish = right[2] > 150 && right[0] < 110;
            if !reddish || !bluish {
                report(Err(format!("gradient custom paint missing from the window: {summary}")));
            }
            if animated == 0 {
                report(Err(format!("no frames while animating: {summary}")));
            }
            report(Ok(summary));
        })
        .detach();
    });
    panic!("shader window probe ended without a verdict");
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn main() {
    panic!("shader_window requires Windows or macOS");
}
