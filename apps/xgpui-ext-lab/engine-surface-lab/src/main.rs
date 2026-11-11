//! Engine surface lab (#197): a wgpu engine on its own device and queue
//! (Metal on macOS, Vulkan on Linux) renders every frame into an
//! `EngineSurface`; xgpui composites the latest frame zero-copy beside ordinary
//! sRGB controls, in two placements.

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn main() {
    eprintln!("engine-surface-lab requires macOS (Metal) or Linux (Vulkan); see issue #197");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    lab::main()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod lab {
    use anyhow::{Context as _, Result, ensure};
    use clap::Parser;
    use gpui::{prelude::*, *};
    use std::{
        cell::RefCell,
        path::PathBuf,
        rc::Rc,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        time::{Duration, Instant},
    };
    use xgpui_direct_gpu::{
        AcquireError, EngineSurface, SurfaceDesc, SurfaceFormat, WgpuSurfaceTarget, wgpu,
    };

    #[cfg(target_os = "macos")]
    const BACKEND: (wgpu::Backends, &str) = (wgpu::Backends::METAL, "Metal");
    #[cfg(target_os = "linux")]
    const BACKEND: (wgpu::Backends, &str) = (wgpu::Backends::VULKAN, "Vulkan");
    #[cfg(target_os = "macos")]
    const TRANSPORT: &str = "zero-copy IOSurface ring";
    #[cfg(target_os = "linux")]
    const TRANSPORT: &str = "zero-copy opaque-fd ring";
    use xgpui_ext_lab::{LabApp, LabWindowTitle};

    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 400;
    const RADIUS: f32 = 8.;

    #[derive(Parser)]
    #[command(about = "wgpu engine on its own device presenting into xgpui")]
    struct Options {
        /// Use a standard SDR window and BGRA8 sRGB slots.
        #[arg(long)]
        sdr: bool,
        /// Use RGBA16F linear-P3 slots even without a wide window (Linux shows
        /// them as a mapped sRGB preview; macOS requires a wide window).
        #[arg(long)]
        p3: bool,
        /// Ring length (2..=8).
        #[arg(long, default_value_t = 3)]
        slots: usize,
        /// Render this many frames in a hidden window, then exit.
        #[arg(long)]
        headless: bool,
        #[arg(long, default_value_t = 120)]
        frames: usize,
        /// Save the last captured frame (clipped sRGB preview).
        #[arg(long)]
        png: Option<PathBuf>,
        /// Show the window normally for this many seconds, print stats, exit.
        #[arg(long)]
        run_seconds: Option<f32>,
        /// Present one frame, then keep recompositing it (isolates compositor cost).
        #[arg(long = "static")]
        static_frame: bool,
        /// Run the engine on its own thread at its own rate, independent of
        /// the display. The UI composites whatever frame is latest.
        #[arg(long)]
        threaded: bool,
        /// Engine frame rate with `--threaded`.
        #[arg(long, default_value_t = 60.)]
        engine_hz: f32,
    }

    /// The engine running on its own thread, presenting at its own rate.
    struct EngineThread {
        rendered: Arc<AtomicU64>,
        skipped: Arc<AtomicU64>,
        error: Arc<Mutex<Option<String>>>,
        stop: Arc<AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
        hz: f32,
    }
    impl EngineThread {
        fn spawn(mut engine: WaveEngine, hz: f32) -> Self {
            let rendered = Arc::new(AtomicU64::new(0));
            let skipped = Arc::new(AtomicU64::new(0));
            let error = Arc::new(Mutex::new(None));
            let stop = Arc::new(AtomicBool::new(false));
            let (r, k, e, s) = (
                rendered.clone(),
                skipped.clone(),
                error.clone(),
                stop.clone(),
            );
            let handle = std::thread::Builder::new()
                .name("engine".into())
                .spawn(move || {
                    let period = Duration::from_secs_f32(1. / hz);
                    let mut next = Instant::now();
                    while !s.load(Ordering::Acquire) {
                        match engine.frame() {
                            Ok(true) => r.fetch_add(1, Ordering::Relaxed),
                            Ok(false) => k.fetch_add(1, Ordering::Relaxed),
                            Err(error) => {
                                *e.lock().unwrap() = Some(format!("{error:#}"));
                                return;
                            }
                        };
                        next += period;
                        let now = Instant::now();
                        if next > now {
                            std::thread::sleep(next - now);
                        } else {
                            next = now;
                        }
                    }
                })
                .expect("engine thread");
            Self {
                rendered,
                skipped,
                error,
                stop,
                handle: Some(handle),
                hz,
            }
        }
    }
    impl Drop for EngineThread {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    const SHADER: &str = r#"
struct Uniforms { time: f32, wide: f32, width: f32, height: f32 };
@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn encode_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy / vec2<f32>(u.width, u.height);
    let t = u.time;
    let v = sin(uv.x * 9.0 + t) + sin(uv.y * 7.0 - t * 1.3) + sin((uv.x + uv.y) * 5.0 + t * 0.7);
    let hue = v * 0.25 + t * 0.1;
    // Saturated primaries: in a wide window these are linear Display P3.
    var color = 0.5 + 0.5 * cos(6.2831853 * (hue + vec3<f32>(0.0, 0.33, 0.67)));
    color = color * color;
    // A moving bar makes dropped or repeated frames visible.
    if abs(uv.x - fract(t * 0.2)) < 0.004 {
        color = vec3<f32>(1.0);
    }
    if u.wide > 0.5 {
        return vec4<f32>(color, 1.0);
    }
    return vec4<f32>(encode_srgb(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}
"#;

    /// The independent engine: its own wgpu device, queue and pipeline.
    struct WaveEngine {
        device: wgpu::Device,
        queue: wgpu::Queue,
        pipeline: wgpu::RenderPipeline,
        uniforms: wgpu::Buffer,
        bind_group: wgpu::BindGroup,
        target: WgpuSurfaceTarget,
        wide: bool,
        start: Instant,
    }

    impl WaveEngine {
        fn new(desc: SurfaceDesc) -> Result<Self> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: BACKEND.0,
                flags: wgpu::InstanceFlags::default(),
                backend_options: wgpu::BackendOptions::default(),
                memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
                display: None,
            });
            let adapter =
                gpui::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                    .with_context(|| format!("no wgpu {} adapter", BACKEND.1))?;
            #[cfg(target_os = "linux")]
            let required_features = WgpuSurfaceTarget::required_features();
            #[cfg(not(target_os = "linux"))]
            let required_features = wgpu::Features::empty();
            let (device, queue) =
                gpui::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: Some("engine"),
                    required_features,
                    ..Default::default()
                }))?;
            let target = WgpuSurfaceTarget::new(desc, &device, &queue)?;
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("engine_waves"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("engine_uniforms"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("engine_waves"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("engine_waves"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(target.format().into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("engine_uniforms"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("engine_uniforms"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                }],
            });
            Ok(Self {
                device,
                queue,
                pipeline,
                uniforms,
                bind_group,
                wide: desc.format == SurfaceFormat::Rgba16FloatLinearP3,
                target,
                start: Instant::now(),
            })
        }

        /// Render and present one frame; `false` when the ring was full.
        fn frame(&mut self) -> Result<bool> {
            let slot = match self.target.surface().acquire() {
                Ok(slot) => slot,
                Err(AcquireError::WouldBlock) => return Ok(false),
            };
            let texture = self.target.texture(&slot)?;
            let view = texture.create_view(&Default::default());
            let uniforms = [
                self.start.elapsed().as_secs_f32(),
                if self.wide { 1. } else { 0. },
                WIDTH as f32,
                HEIGHT as f32,
            ];
            let bytes: Vec<u8> = uniforms.iter().flat_map(|v| v.to_ne_bytes()).collect();
            self.queue.write_buffer(&self.uniforms, 0, &bytes);
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("engine_waves"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            self.target.present(slot, [encoder.finish()])?;
            Ok(true)
        }
    }

    struct Lab {
        surface: EngineSurface,
        engine: Option<WaveEngine>,
        thread: Option<EngineThread>,
        last_presents: u64,
        new_frames_shown: u64,
        repeated_frames_shown: u64,
        wide: bool,
        notice: Option<String>,
        rendered: u64,
        skipped: u64,
        fps: f32,
        window_start: Instant,
        window_frames: u32,
        error: Option<String>,
        started: Instant,
        run_seconds: Option<f32>,
        frame_times: Vec<f32>,
        last_frame: Option<Instant>,
        static_frame: bool,
    }

    impl Lab {
        fn engine_mode(&self) -> String {
            match &self.thread {
                Some(thread) => format!("engine thread @ {:.0} Hz", thread.hz),
                None => "engine on UI thread".into(),
            }
        }
        fn surface_view(&self, width: f32, height: f32) -> impl IntoElement {
            let surface = self.surface.clone();
            div()
                .w(px(width))
                .h(px(height))
                .rounded(px(RADIUS))
                .border_1()
                .border_color(rgb(0x3a3a3a))
                .child(
                    canvas(
                        |_, _, _| (),
                        // Inside the 1px border the inner corner radius is one smaller.
                        move |bounds, _, window, _| {
                            surface.paint(window, bounds, Corners::all(px(RADIUS - 1.)))
                        },
                    )
                    .size_full(),
                )
        }
    }

    impl Render for Lab {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let now = Instant::now();
            if let Some(last) = self.last_frame.replace(now) {
                self.frame_times
                    .push(now.duration_since(last).as_secs_f32() * 1000.);
            }
            if let Some(limit) = self.run_seconds
                && self.started.elapsed().as_secs_f32() >= limit
            {
                let stats = self.surface.stats();
                let mut times = self.frame_times.clone();
                times.sort_by(f32::total_cmp);
                let pick = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
                println!(
                    "{:.1}s shown ({}): {} window frames ({} new / {} repeated engine frames), engine rendered {} / skipped {} (ring full {}), presents {}; frame interval ms p50 {:.2} p95 {:.2} p99 {:.2} max {:.2}; {}",
                    self.started.elapsed().as_secs_f32(),
                    self.engine_mode(),
                    times.len() + 1,
                    self.new_frames_shown,
                    self.repeated_frames_shown,
                    self.rendered,
                    self.skipped,
                    stats.would_block,
                    stats.presents,
                    pick(0.5),
                    pick(0.95),
                    pick(0.99),
                    pick(1.0),
                    format!(
                        "{} window, {:?} slots",
                        if self.wide { "wide" } else { "SDR" },
                        self.surface.desc().format
                    ),
                );
                if let Some(error) = &self.error {
                    println!("engine error: {error}");
                }
                cx.quit();
            }
            if let Some(thread) = &self.thread {
                self.rendered = thread.rendered.load(Ordering::Relaxed);
                self.skipped = thread.skipped.load(Ordering::Relaxed);
                if let Some(error) = thread.error.lock().unwrap().take() {
                    self.error = Some(error);
                }
            } else if let Some(engine) = &mut self.engine {
                let step = !(self.static_frame && self.rendered > 0);
                match if step { engine.frame() } else { Ok(false) } {
                    Ok(true) => self.rendered += 1,
                    Ok(false) => self.skipped += 1,
                    Err(error) => self.error = Some(format!("{error:#}")),
                }
            }
            // Which frame this UI frame will show: new since the last UI frame, or a repeat.
            let presents = self.surface.stats().presents;
            if presents != self.last_presents {
                self.new_frames_shown += 1;
                self.last_presents = presents;
            } else {
                self.repeated_frames_shown += 1;
            }
            self.window_frames += 1;
            let elapsed = self.window_start.elapsed().as_secs_f32();
            if elapsed >= 1. {
                self.fps = self.window_frames as f32 / elapsed;
                self.window_frames = 0;
                self.window_start = Instant::now();
            }
            window.request_animation_frame();
            let stats = self.surface.stats();
            let desc = self.surface.desc();
            let status = format!(
                "{} · {} · {}×{} {:?} · {} slots (free {} / writing {} / published {}) · presented {} · ring full {} · {:.0} fps",
                self.engine_mode(),
                if self.wide {
                    "wide-gamut SDR window"
                } else {
                    "standard SDR window"
                },
                desc.width,
                desc.height,
                desc.format,
                desc.slots,
                stats.free,
                stats.writing,
                stats.published,
                stats.presents,
                stats.would_block,
                self.fps,
            );
            div()
                .size_full()
                .bg(rgb(0x1b1b1b))
                .text_color(rgb(0xe8e8e8))
                .text_sm()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_lg().child(format!(
                    "wgpu engine on its own {} device → xgpui ({TRANSPORT})",
                    BACKEND.1
                )))
                .child(div().text_color(rgb(0xb0b0b0)).child(status))
                .children(
                    self.notice
                        .clone()
                        .map(|notice| div().text_color(rgb(0xffcc66)).child(notice)),
                )
                .children(self.error.clone().map(|error| {
                    div()
                        .text_color(rgb(0xff6666))
                        .child(format!("engine error: {error}"))
                }))
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .items_start()
                        .child(self.surface_view(WIDTH as f32, HEIGHT as f32))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child("Same frame, second placement")
                                .child(self.surface_view(240., 150.))
                                .child("Ordinary sRGB controls")
                                .child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(div().size(px(40.)).bg(rgb(0xff0000)))
                                        .child(div().size(px(40.)).bg(rgb(0x00ff00)))
                                        .child(div().size(px(40.)).bg(rgb(0x0000ff))),
                                ),
                        ),
                )
        }
    }

    pub fn main() -> Result<()> {
        let options = Options::parse();
        ensure!((2..=8).contains(&options.slots), "slots must be 2..=8");
        ensure!(
            options.engine_hz.is_finite() && (1. ..=1000.).contains(&options.engine_hz),
            "engine-hz must be 1..=1000"
        );
        ensure!(
            (1..=10000).contains(&options.frames),
            "frames must be 1..=10000"
        );
        let completion: Rc<RefCell<Option<Result<()>>>> = Rc::new(RefCell::new(None));
        let capture_result = completion.clone();
        let headless = options.headless;
        gpui_platform::application().run(move |cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(1000.), px(640.)), cx);
            let window = cx
                .open_lab_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        show: !options.headless,
                        focus: !options.headless,
                        ..Default::default()
                    },
                    |window, cx| {
                        let notice = if options.sdr {
                            None
                        } else {
                            xgpui_ext_lab::negotiate_wide(window)
                        };
                        let wide =
                            window.color_status().effective() == WindowColorMode::WideGamutSdr;
                        let format = if wide || options.p3 {
                            SurfaceFormat::Rgba16FloatLinearP3
                        } else {
                            SurfaceFormat::Bgra8Srgb
                        };
                        let engine = WaveEngine::new(SurfaceDesc {
                            width: WIDTH,
                            height: HEIGHT,
                            format,
                            slots: options.slots,
                        })
                        .expect("wgpu engine");
                        let surface = engine.target.surface().clone();
                        let (engine, thread) = if options.threaded {
                            (None, Some(EngineThread::spawn(engine, options.engine_hz)))
                        } else {
                            (Some(engine), None)
                        };
                        window.set_lab_title("XGPUI Engine Surface Lab");
                        cx.new(|_| Lab {
                            surface,
                            engine,
                            thread,
                            last_presents: 0,
                            new_frames_shown: 0,
                            repeated_frames_shown: 0,
                            wide,
                            notice,
                            rendered: 0,
                            skipped: 0,
                            fps: 0.,
                            window_start: Instant::now(),
                            window_frames: 0,
                            error: None,
                            started: Instant::now(),
                            run_seconds: options.run_seconds,
                            frame_times: Vec::new(),
                            last_frame: None,
                            static_frame: options.static_frame,
                        })
                    },
                )
                .unwrap();
            if headless {
                let frames = options.frames;
                let png = options.png.clone();
                cx.defer(move |cx| {
                    cx.update_window(window.into(), |_, window, cx| {
                        let result = (|| -> Result<()> {
                            let started = Instant::now();
                            let mut image = None;
                            for _ in 0..frames {
                                window.refresh();
                                window.draw(cx).clear(cx);
                                image = Some(window.render_to_image()?);
                            }
                            let image = image.context("no frame captured")?;
                            if let Some(path) = &png {
                                image.save(path)?;
                                println!("saved {}", path.display());
                            }
                            println!(
                                "{frames} window frames in {:?}; surface center pixel {:?}",
                                started.elapsed(),
                                image.get_pixel(16 + WIDTH / 2, 100 + HEIGHT / 2).0
                            );
                            Ok(())
                        })();
                        if let Err(error) = &result {
                            eprintln!("capture failed: {error:#}");
                        }
                        *capture_result.borrow_mut() = Some(result);
                        cx.quit();
                    })
                    .expect("capture window");
                });
            } else {
                cx.activate(true);
            }
        });
        if headless {
            completion
                .borrow_mut()
                .take()
                .unwrap_or_else(|| Err(anyhow::anyhow!("application exited before capture")))?;
        }
        Ok(())
    }
}
