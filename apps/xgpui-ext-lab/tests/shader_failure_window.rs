//! Real-window probe: an instanced xgpui-shader program that DirectX cannot compile
//! must not take the rest of the window down with it (#284).
//!
//! The WGSL below is valid (naga accepts it) but writes a vector component at a
//! runtime index, which FXC rejects (X3500). The Windows backend used to fail the
//! whole frame and retry the compile on every frame. With a large shader each FXC
//! attempt takes seconds, so xgpui-luma's Color Spaces page froze. Now the program
//! is compiled once, its draws are skipped, and the rest of the window renders.
#[cfg(target_os = "windows")]
fn main() {
    use gpui::{prelude::*, *};
    use std::{
        cell::RefCell,
        rc::Rc,
        time::{Duration, Instant},
    };
    use xgpui_shader::{
        WindowShaderExt,
        instanced::{InstanceField, ShaderInstance, ShaderProgram},
    };

    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct Parameters {
        color: [f32; 4],
        effect: [f32; 4],
    }
    impl ShaderInstance for Parameters {
        const FIELDS: &'static [InstanceField] = &[
            InstanceField { name: "color", offset: 0, components: 4 },
            InstanceField { name: "effect", offset: 16, components: 4 },
        ];
    }
    const BROKEN_ON_FXC: &str = "
struct Parameters { color: vec4<f32>, effect: vec4<f32>, }
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    var rgb = p.color.rgb;
    rgb[u32(p.effect.x)] = input.position.x / input.size.x;
    return vec4(rgb, 1.0);
}
";

    struct Probe {
        program: ShaderProgram<Parameters>,
        frames: Rc<RefCell<Vec<Instant>>>,
        paint_errors: Rc<RefCell<Vec<String>>>,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.frames.borrow_mut().push(Instant::now());
            window.request_animation_frame();
            let program = self.program.clone();
            let errors = self.paint_errors.clone();
            div()
                .size_full()
                .flex()
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let instance = Parameters {
                                color: [0., 1., 0., 1.],
                                effect: [1., 0., 0., 0.],
                            };
                            if let Err(error) = window.paint_custom_shader(bounds, &program, instance)
                            {
                                errors.borrow_mut().push(format!("{error:#}"));
                            }
                        },
                    )
                    .w_1_2()
                    .h_full(),
                )
                .child(div().w_1_2().h_full().bg(rgb(0xff0000)))
        }
    }

    fn report(result: Result<String, String>) -> ! {
        match result {
            Ok(summary) => {
                println!("shader failure window probe passed: {summary}");
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("shader failure window probe failed: {error}");
                std::process::exit(1);
            }
        }
    }

    let program = ShaderProgram::<Parameters>::new(BROKEN_ON_FXC).expect("valid WGSL");
    gpui_platform::application().run(move |cx| {
        let frames: Rc<RefCell<Vec<Instant>>> = Rc::default();
        let paint_errors: Rc<RefCell<Vec<String>>> = Rc::default();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(80.), px(80.)),
                        size(px(320.), px(160.)),
                    ))),
                    inactive_frame_interval: None,
                    low_power_frame_interval: None,
                    thermal_frame_interval: None,
                    ..Default::default()
                },
                {
                    let (frames, paint_errors) = (frames.clone(), paint_errors.clone());
                    move |_, cx| {
                        cx.new(|_| Probe {
                            program,
                            frames,
                            paint_errors,
                        })
                    }
                },
            )
            .expect("open real window");
        cx.activate(true);
        cx.spawn(async move |cx| {
            let executor = cx.background_executor().clone();
            executor.timer(Duration::from_millis(1000)).await;
            let start = Instant::now();
            executor.timer(Duration::from_millis(2000)).await;
            let animated = frames.borrow().iter().filter(|t| **t >= start).count();
            let fps = animated as f64 / start.elapsed().as_secs_f64();
            if let Some(error) = paint_errors.borrow().first() {
                report(Err(format!("paint_custom_shader rejected the instance: {error}")));
            }
            let image = match window.update(cx, |_, window, _| window.render_to_image()) {
                Ok(Ok(image)) => image,
                Ok(Err(error)) => report(Err(format!(
                    "the frame failed with the broken program in it: {error:#}"
                ))),
                Err(error) => report(Err(format!("window gone: {error:#}"))),
            };
            let (w, h) = (image.width(), image.height());
            let right = image.get_pixel(w * 3 / 4, h / 2).0;
            let summary = format!("{fps:.1} fps; right half pixel {right:?}");
            if !(right[0] > 200 && right[1] < 60 && right[2] < 60) {
                report(Err(format!("the rest of the window did not render: {summary}")));
            }
            if fps < 20. {
                report(Err(format!("frames stalled: {summary}")));
            }
            report(Ok(summary));
        })
        .detach();
    });
    panic!("shader failure window probe ended without a verdict");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("shader_failure_window requires Windows");
}
