// Shared native/Wasm view; the library owns provider scheduling and page caches.
use gpui::{prelude::*, *};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
use xgpui_media::pipeline::*;

pub struct TiledDemo {
    canvas: TileCanvas,
    blurs: [ImageBlur; 4],
    blur_index: usize,
    full_quality: bool,
    generation: u64,
    center: [f64; 2],
    zoom: f64,
    drag: Option<(Point<Pixels>, [f64; 2])>,
    paused: bool,
    pressure: bool,
    error: Option<String>,
    started: Instant,
    view_started: Instant,
    last_view: Option<CanvasView>,
    preview_ms: Option<f64>,
    complete_ms: Option<f64>,
    peak_cpu: u64,
    peak_gpu: u64,
}
impl TiledDemo {
    pub fn new() -> Self {
        Self {
            canvas: Self::make_canvas(0, false),
            blurs: [0., 4., 8., 16.].map(|r| ImageBlur::new(px(r)).unwrap()),
            blur_index: 0,
            full_quality: false,
            generation: 0,
            center: [6000., 3000.],
            zoom: 0.,
            drag: None,
            paused: false,
            pressure: false,
            error: None,
            started: Instant::now(),
            view_started: Instant::now(),
            last_view: None,
            preview_ms: None,
            complete_ms: None,
            peak_cpu: 0,
            peak_gpu: 0,
        }
    }
    fn make_canvas(generation: u64, pressure: bool) -> TileCanvas {
        let mut options = CanvasOptions::default();
        if pressure {
            options.budgets.cpu_bytes = 4 * 1024 * 1024;
            options.budgets.gpu_bytes = 2 * 1024 * 1024;
            options.max_cached_pages = 6;
            options.budgets.uploads_per_frame = 1;
        }
        TileCanvas::new(Arc::new(GeneratedCanvas::new(generation)), options)
            .expect("generated canvas")
    }
    fn replace(&mut self) {
        self.generation += 1;
        self.canvas
            .replace(Arc::new(GeneratedCanvas::new(self.generation)))
            .expect("same generated descriptor");
        self.paused = false;
        self.error = None;
        self.started = Instant::now();
        self.last_view = None;
        self.preview_ms = None;
        self.complete_ms = None;
    }
    fn pan(&mut self, dx: f64, dy: f64, scale: f64) {
        self.center[0] = (self.center[0] + dx / scale).clamp(0., 12000.);
        self.center[1] = (self.center[1] + dy / scale).clamp(0., 6000.);
        self.complete_ms = None;
    }
}
fn button(id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .px_3()
        .py_2()
        .bg(rgb(0x334155))
        .rounded_md()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x475569)))
        .child(label)
}
impl Render for TiledDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut viewport = [
            (f32::from(window.viewport_size().width) - 32.).max(200.) as f64,
            380.,
        ];
        if self.blur_index > 0 {
            let side = (1020. / window.scale_factor()
                - 2. * (16. + 8. / window.scale_factor()) * MAX_LAYER_GROUPS as f32)
                .max(40.) as f64;
            viewport[0] = viewport[0].min(side);
            viewport[1] = viewport[1].min(side).min(300.);
        }
        let fit = (viewport[0] / 12000.).min(viewport[1] / 6000.);
        let scale = if self.zoom == 0. { fit } else { self.zoom };
        let view = CanvasView {
            center: self.center,
            scale,
            viewport,
            device_scale: f64::from(window.scale_factor()),
        };
        if self.last_view != Some(view) {
            self.last_view = Some(view);
            self.view_started = Instant::now();
            self.complete_ms = None;
        }
        if !self.paused
            && let Err(e) = self.canvas.advance(view)
        {
            self.error = Some(format!("{e:#}"));
        }
        let snapshot = self.canvas.snapshot();
        let stats = self.canvas.stats();
        if self.paused
            && (stats.gpu_bytes > 0 || stats.active_jobs > 0 || stats.upload_staging_bytes > 0)
        {
            window.request_animation_frame();
        }
        if snapshot.as_ref().is_some_and(|s| s.preview.is_some()) && self.preview_ms.is_none() {
            self.preview_ms = Some(self.started.elapsed().as_secs_f64() * 1000.);
        }
        if self.canvas.status() == &ProgressiveStatus::Ready
            && stats.ready_pages == stats.visible_pages
            && self.complete_ms.is_none()
        {
            self.complete_ms = Some(self.view_started.elapsed().as_secs_f64() * 1000.);
        }
        self.peak_cpu = self.peak_cpu.max(stats.cpu_bytes);
        self.peak_gpu = self.peak_gpu.max(stats.gpu_bytes);
        if !self.paused
            && self.error.is_none()
            && !matches!(
                self.canvas.status(),
                ProgressiveStatus::Failed(_) | ProgressiveStatus::Cancelled
            )
            && (self.canvas.status() != &ProgressiveStatus::Ready
                || stats.ready_pages < stats.visible_pages)
        {
            window.request_animation_frame();
        }
        let blur = self.blurs[self.blur_index].clone();
        let canvas = div()
            .id("tiled-viewport")
            .w(px(viewport[0] as f32))
            .h(px(viewport[1] as f32))
            .flex_shrink_0()
            .bg(rgb(0x253342))
            .overflow_hidden()
            .cursor_crosshair()
            .on_scroll_wheel(cx.listener(move |s, event: &ScrollWheelEvent, _, cx| {
                let delta = f64::from(f32::from(event.delta.pixel_delta(px(24.)).y));
                s.zoom = (scale * (delta * 0.002).clamp(-1., 1.).exp()).clamp(0.01, 4.);
                s.complete_ms = None;
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, event: &MouseDownEvent, _, cx| {
                    s.drag = Some((event.position, s.center));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(move |s, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    if let Some((anchor, center)) = s.drag {
                        let delta = event.position - anchor;
                        s.center = [
                            (center[0] - f64::from(f32::from(delta.x)) / scale).clamp(0., 12000.),
                            (center[1] - f64::from(f32::from(delta.y)) / scale).clamp(0., 6000.),
                        ];
                        s.complete_ms = None;
                        cx.notify();
                    }
                } else {
                    s.drag = None;
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|s, _, _, _| s.drag = None))
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        if let Some(snapshot) = &snapshot {
                            blur.paint(bounds, window, |window| snapshot.paint(bounds, window))
                                .expect("bounded blur viewport");
                        }
                    },
                )
                .size_full(),
            );
        let mib = |bytes: u64| bytes as f64 / (1024. * 1024.);
        div().id("tiled-demo").size_full().overflow_y_scroll().flex().flex_col().gap_3().p_4()
            .bg(rgb(0x0f172a)).text_color(rgb(0xe2e8f0)).font_family("IBM Plex Sans")
            .child(div().text_xl().child("Progressive tiled canvas · 72 megapixels"))
            .child("12,000 × 6,000 generated source · preview stays visible as selected tiles refine · drag to pan, wheel to zoom")
            .child(div().flex().flex_wrap().gap_2()
                .child(button("tile-fit","Fit").on_click(cx.listener(|s,_,_,cx|{s.zoom=0.;s.center=[6000.,3000.];cx.notify();})))
                .child(button("tile-one","1:1").on_click(cx.listener(|s,_,_,cx|{s.zoom=1.;cx.notify();})))
                .child(button("tile-plus","Zoom +").on_click(cx.listener(move |s,_,_,cx|{s.zoom=(scale*2.).min(4.);cx.notify();})))
                .child(button("tile-minus","Zoom −").on_click(cx.listener(move |s,_,_,cx|{s.zoom=(scale/2.).max(0.01);cx.notify();})))
                .child(button("tile-left","←").on_click(cx.listener(move |s,_,_,cx|{s.pan(-300.,0.,scale);cx.notify();})))
                .child(button("tile-right","→").on_click(cx.listener(move |s,_,_,cx|{s.pan(300.,0.,scale);cx.notify();})))
                .child(button("tile-up","↑").on_click(cx.listener(move |s,_,_,cx|{s.pan(0.,-180.,scale);cx.notify();})))
                .child(button("tile-down","↓").on_click(cx.listener(move |s,_,_,cx|{s.pan(0.,180.,scale);cx.notify();})))
                .child(button("tile-replace","Replace source").on_click(cx.listener(|s,_,_,cx|{s.replace();cx.notify();})))
                .child(button("tile-cancel",if self.paused{"Restart"}else{"Cancel"}).on_click(cx.listener(|s,_,_,cx|{
                    if s.paused{s.replace();}else{s.canvas.cancel();s.paused=true;}cx.notify();
                })))
                .child(button("tile-pressure",if self.pressure{"Cache: 2 MiB"}else{"Cache: 16 MiB"}).on_click(cx.listener(|s,_,_,cx|{
                    s.pressure= !s.pressure;s.canvas=Self::make_canvas(s.generation,s.pressure);
                    s.started=Instant::now();s.last_view=None;s.preview_ms=None;s.complete_ms=None;s.peak_cpu=0;s.peak_gpu=0;s.paused=false;cx.notify();
                }))))
            .child(div().flex().gap_2()
                .child(button("tile-blur",format!("Blur: {} px",[0,4,8,16][self.blur_index])).on_click(cx.listener(|s,_,_,cx|{s.blur_index=(s.blur_index+1)%4;cx.notify();})))
                .child(button("tile-quality",if self.full_quality{"Quality: full"}else{"Quality: adaptive"}).on_click(cx.listener(|s,_,_,cx|{
                    s.full_quality= !s.full_quality;
                    s.blurs=[0.,4.,8.,16.].map(|r|ImageBlur::new(px(r)).unwrap().with_quality(if s.full_quality{LayerBlurQuality::FullResolution}else{LayerBlurQuality::Adaptive}));cx.notify();
                }))))
            .child("Blur runs once over assembled pages: shared tile edges, transparent outside the viewport. Blur uses a bounded preview (1024 device px including halo). Retained sRGB blur targets are additional to page budgets.")
            .child(format!("Generation {} · {:?} · zoom {:.1}% · mip {} · ready {}/{} · cached {}",
                self.generation,self.canvas.status(),scale*100.,stats.mip,stats.ready_pages,stats.visible_pages,stats.cached_pages))
            .child(canvas)
            .child(format!("Reserved CPU {:.2} MiB · GPU payload {:.2} MiB · pending transfer {:.2} MiB · jobs {}",
                mib(stats.cpu_bytes),mib(stats.gpu_bytes),mib(stats.pending_staging_bytes),stats.active_jobs))
            .child(format!("Upload staging {:.2} MiB · native staging pool allocated {:.2} MiB",mib(stats.upload_staging_bytes),mib(stats.staging_pool_bytes)))
            .child(format!("Peaks CPU {:.2} / GPU {:.2} MiB · requests {} · evictions {} · cancelled {} · stale {} · failures {}",
                mib(self.peak_cpu),mib(self.peak_gpu),stats.requests,stats.evictions,stats.cancelled_requests,stats.stale_results,stats.failures))
            .child(format!("Generated preview CPU-ready: {} · current view submitted: {}",
                self.preview_ms.map(|v|format!("{v:.1} ms")).unwrap_or_else(||"pending".into()),
                self.complete_ms.map(|v|format!("{v:.1} ms")).unwrap_or_else(||"refining".into())))
            .child(self.error.clone().unwrap_or_default())
            .child(if cfg!(target_arch="wasm32"){
                "Browser: one generated page per frame on the host thread; no file I/O/decoder or browser worker is measured. GPU completion retains immutable pages."
            }else{
                "Native: bounded provider workers and background GPU staging. Stats count payload reservations, not process RSS or driver VRAM. Old scenes remain charged until GPU completion."
            })
            .child("Grid boundaries expose tile seams. The generated provider evaluates mip-center samples; a real asset provider must supply prefiltered pyramid data. 60 Hz remains the performance baseline.")
    }
}
