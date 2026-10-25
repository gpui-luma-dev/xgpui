use gpui::{prelude::*, *};
use std::{sync::Arc, time::Duration};
use web_time::Instant;
use xgpui_svg::{
    MIB, RasterPolicy, SchedulerLimits, SvgDocument, SvgScheduler, SvgStatus, svg_image,
};

const SAMPLES: &[(&str, &[u8])] = &[
    (
        "Ribbon + leaf",
        include_bytes!("../../svg-lab/samples/ribbon-leaf.svg"),
    ),
    (
        "Camera · AJ Ashton",
        include_bytes!("../../svg-lab/samples/AJ_Digital_Camera.svg"),
    ),
    (
        "Grapes · rg1024",
        include_bytes!("../../svg-lab/samples/rg1024_green_grapes.svg"),
    ),
    (
        "Metal · rg1024",
        include_bytes!("../../svg-lab/samples/rg1024_metal_effect.svg"),
    ),
    (
        "Dragon · Ebaychatter0",
        include_bytes!("../../svg-lab/samples/dragon.svg"),
    ),
];
fn pool(mib: usize) -> SvgScheduler {
    SvgScheduler::new(SchedulerLimits {
        max_jobs: 1,
        max_bytes: mib * MIB,
    })
    .unwrap()
}
fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(if active { 0x4f46a5 } else { 0x374151 }))
        .child(label.into())
}
#[derive(Clone, Copy, PartialEq)]
struct ViewportKey {
    selected: usize,
    zoom: f32,
    pan: Point<Pixels>,
    pixels: Size<DevicePixels>,
    logical: Size<Pixels>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ZoomMode {
    Hybrid,
    Sharp,
    Texture,
}
pub struct SvgDemo {
    docs: Vec<SvgDocument>,
    selected: usize,
    zoom: f32,
    pan: Point<Pixels>,
    bounds: [Bounds<Pixels>; 2],
    sharp_key: Option<ViewportKey>,
    sharp_image: Option<Arc<RenderImage>>,
    hybrid_base: Option<(ViewportKey, Arc<RenderImage>)>,
    sharp_ms: f64,
    zoom_mode: ZoomMode,
    hybrid_task: Option<Task<()>>,
    hybrid_pending: Option<ViewportKey>,
    hybrid_failed: Option<ViewportKey>,
    hybrid_generation: u64,
    hybrid_state: &'static str,
    sharp_rasters: usize,
    sharp_error: Option<String>,
    drag: Option<Point<Pixels>>,
    light: bool,
    gallery: bool,
    budget: usize,
    scheduler: SvgScheduler,
    status: Option<SvgStatus>,
}
impl SvgDemo {
    pub fn new(cx: &mut Context<Self>) -> anyhow::Result<Self> {
        let renderer = cx.svg_renderer();
        let docs = SAMPLES
            .iter()
            .map(|(_, bytes)| SvgDocument::parse(&renderer, bytes))
            .collect::<anyhow::Result<Vec<_>>>()?;
        cx.on_release(|this, cx| {
            crate::svg_worker::dispose();
            if let Some((_, base)) = this.hybrid_base.take()
                && this
                    .sharp_image
                    .as_ref()
                    .is_none_or(|current| !Arc::ptr_eq(current, &base))
            {
                cx.drop_image(base, None);
            }
            if let Some(image) = this.sharp_image.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        Ok(Self {
            docs,
            selected: 0,
            zoom: 1.,
            pan: point(px(0.), px(0.)),
            bounds: [Bounds::default(); 2],
            sharp_key: None,
            sharp_image: None,
            hybrid_base: None,
            sharp_ms: 0.,
            zoom_mode: ZoomMode::Hybrid,
            hybrid_task: None,
            hybrid_pending: None,
            hybrid_failed: None,
            hybrid_generation: 0,
            hybrid_state: "Starting worker",
            sharp_rasters: 0,
            sharp_error: None,
            drag: None,
            light: true,
            gallery: false,
            budget: 32,
            scheduler: pool(32),
            status: None,
        })
    }
    /// The comparison raster covers only the viewport, at device density. Cache
    /// by camera and geometry so ordinary redraws never rerasterize the SVG.
    fn sharp_view(
        &mut self,
        bounds: Bounds<Pixels>,
        density: f32,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let pixels = size(
            DevicePixels((f32::from(bounds.size.width) * density).ceil() as i32),
            DevicePixels((f32::from(bounds.size.height) * density).ceil() as i32),
        );
        let key = ViewportKey {
            selected: self.selected,
            zoom: if self.zoom_mode == ZoomMode::Texture {
                1.
            } else {
                self.zoom
            },
            pan: if self.zoom_mode == ZoomMode::Texture {
                point(px(0.), px(0.))
            } else {
                self.pan
            },
            pixels,
            logical: bounds.size,
        };
        if self.zoom_mode == ZoomMode::Hybrid {
            if self
                .hybrid_base
                .as_ref()
                .is_some_and(|(base, _)| base.selected != key.selected)
            {
                let (_, base) = self.hybrid_base.take().unwrap();
                if self
                    .sharp_image
                    .as_ref()
                    .is_none_or(|current| !Arc::ptr_eq(current, &base))
                {
                    cx.defer(move |cx| cx.drop_image(base, None));
                }
            }
            let render_key = if self.hybrid_base.is_none() {
                ViewportKey {
                    zoom: 1.,
                    pan: point(px(0.), px(0.)),
                    ..key
                }
            } else {
                key
            };
            if self.sharp_image.is_some()
                && self.sharp_key == Some(key)
                && self.hybrid_base.is_some()
            {
                if self.hybrid_pending.is_some() {
                    self.camera_changed();
                }
                self.hybrid_state = "Sharp worker result ready";
            } else if self.hybrid_pending != Some(render_key)
                && self.hybrid_failed != Some(render_key)
            {
                self.schedule_hybrid(render_key, cx);
            }
            return self.sharp_image.clone();
        }
        if self.sharp_key != Some(key) {
            self.sharp_key = Some(key);
            let view_box = self.region(key);
            let start = Instant::now();
            let result = cx.svg_renderer().render_parsed(
                &self.docs[self.selected],
                SvgSize::Viewport {
                    size: pixels,
                    view_box,
                },
            );
            self.sharp_ms = start.elapsed().as_secs_f64() * 1000.;
            // Retire the old atlas entry after the current frame has finished.
            if let Some(image) = self.sharp_image.take() {
                cx.defer(move |cx| cx.drop_image(image, None));
            }
            match result {
                Ok(image) => {
                    self.sharp_rasters += 1;
                    self.sharp_image = Some(image);
                    self.sharp_error = None;
                }
                Err(error) => self.sharp_error = Some(error.to_string()),
            }
            // Prepaint updates happen after the diagnostic labels are built.
            // Notify after this frame so the new count/time appear immediately.
            let entity = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = entity.update(cx, |_, cx| cx.notify());
            });
        }
        self.sharp_image.clone()
    }
    fn region(&self, key: ViewportKey) -> Bounds<f32> {
        let intrinsic = self.docs[key.selected].size();
        let factor = (f32::from(key.logical.width) / intrinsic.width)
            .min(f32::from(key.logical.height) / intrinsic.height)
            * key.zoom;
        Bounds::new(
            point(
                intrinsic.width / 2.
                    - (f32::from(key.logical.width) / 2. + f32::from(key.pan.x)) / factor,
                intrinsic.height / 2.
                    - (f32::from(key.logical.height) / 2. + f32::from(key.pan.y)) / factor,
            ),
            size(
                f32::from(key.logical.width) / factor,
                f32::from(key.logical.height) / factor,
            ),
        )
    }
    fn camera_changed(&mut self) {
        self.hybrid_generation += 1;
        self.hybrid_pending = None;
        self.hybrid_failed = None;
        self.hybrid_task.take();
        crate::svg_worker::invalidate();
        self.hybrid_state = "Texture preview • moving";
    }
    fn schedule_hybrid(&mut self, key: ViewportKey, cx: &mut Context<Self>) {
        self.camera_changed();
        self.hybrid_pending = Some(key);
        self.hybrid_state = "Texture preview • settling for 160 ms";
        let generation = self.hybrid_generation;
        let region = self.region(key);
        let delay = if self.sharp_image.is_none() { 0 } else { 160 };
        let timer = cx.background_executor().timer(Duration::from_millis(delay));
        self.hybrid_task = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let dispatch = this
                .update(cx, |state, cx| {
                    if state.hybrid_generation != generation || state.zoom_mode != ZoomMode::Hybrid
                    {
                        return false;
                    }
                    if state.drag.is_some() {
                        state.hybrid_state = "Texture preview • dragging";
                        return false;
                    }
                    state.hybrid_state = "Texture preview • worker rendering";
                    state.sharp_error = None;
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !dispatch {
                return;
            }
            let result = match crate::svg_worker::request(
                SAMPLES[key.selected].1,
                i32::from(key.pixels.width) as u32,
                i32::from(key.pixels.height) as u32,
                region.origin.x,
                region.origin.y,
                region.size.width,
                region.size.height,
            ) {
                Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise).await,
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |state, cx| {
                if state.hybrid_generation != generation || state.zoom_mode != ZoomMode::Hybrid {
                    return;
                }
                state.hybrid_pending = None;
                let result = result
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|reply| decode_worker_reply(key, reply));
                match result {
                    Ok((image, ms)) => {
                        let mut retired = Vec::new();
                        if key.zoom == 1.
                            && key.pan == point(px(0.), px(0.))
                            && let Some((_, previous)) =
                                state.hybrid_base.replace((key, image.clone()))
                        {
                            retired.push(previous);
                        }
                        if let Some(previous) = state.sharp_image.replace(image)
                            && !retired.iter().any(|old| Arc::ptr_eq(old, &previous))
                        {
                            retired.push(previous);
                        }
                        for previous in retired {
                            let retained = state
                                .hybrid_base
                                .as_ref()
                                .is_some_and(|(_, base)| Arc::ptr_eq(base, &previous))
                                || state
                                    .sharp_image
                                    .as_ref()
                                    .is_some_and(|current| Arc::ptr_eq(current, &previous));
                            if !retained {
                                cx.defer(move |cx| cx.drop_image(previous, None));
                            }
                        }
                        state.sharp_key = Some(key);
                        state.sharp_ms = ms;
                        state.sharp_rasters += 1;
                        state.sharp_error = None;
                        state.hybrid_state = "Sharp worker result ready";
                    }
                    Err(error) => {
                        state.sharp_error = Some(error);
                        state.hybrid_failed = Some(key);
                        state.hybrid_state = "Worker failed • texture preview retained";
                    }
                }
                cx.notify();
            });
        }));
    }
    /// Transform the last viewport raster to the current camera while the worker
    /// prepares a replacement. Its key tracks the camera that actually produced it.
    fn displayed_raster(
        &self,
        bounds: Bounds<Pixels>,
    ) -> Option<(Arc<RenderImage>, Bounds<Pixels>)> {
        let (key, image) = if self.zoom_mode == ZoomMode::Hybrid
            && (self.hybrid_pending.is_some() || self.hybrid_failed.is_some())
        {
            self.hybrid_base
                .as_ref()
                .map(|(key, image)| (*key, image.clone()))
                .or_else(|| Some((self.sharp_key?, self.sharp_image.clone()?)))?
        } else {
            (self.sharp_key?, self.sharp_image.clone()?)
        };
        if key.selected != self.selected {
            return None;
        }
        let intrinsic = self.docs[self.selected].size();
        let fit = |logical: Size<Pixels>| {
            (f32::from(logical.width) / intrinsic.width)
                .min(f32::from(logical.height) / intrinsic.height)
        };
        let ratio = fit(bounds.size) * self.zoom / (fit(key.logical) * key.zoom);
        let image_size = size(key.logical.width * ratio, key.logical.height * ratio);
        let origin = bounds.center() + self.pan
            - key.pan * ratio
            - point(image_size.width / 2., image_size.height / 2.);
        Some((image, Bounds::new(origin, image_size)))
    }
    fn reset(&mut self) {
        self.camera_changed();
        self.zoom = 1.;
        self.pan = point(px(0.), px(0.));
        self.drag = None;
    }
    fn zoom_at(&mut self, anchor: Point<Pixels>, factor: f32) {
        self.camera_changed();
        let zoom = (self.zoom * factor).clamp(0.25, 16.);
        self.pan = anchor - (anchor - self.pan) * (zoom / self.zoom);
        self.zoom = zoom;
    }
}
impl Render for SvgDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut samples = div().flex().flex_wrap().gap_2();
        for (i, (name, _)) in SAMPLES.iter().enumerate() {
            samples = samples.child(button(("sample", i), *name, self.selected == i).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.selected = i;
                    this.reset();
                    cx.notify();
                }),
            ));
        }
        let mut zooms = div().flex().flex_wrap().gap_2();
        for (i, zoom) in [0.5, 1., 2., 4., 8., 16.].into_iter().enumerate() {
            zooms = zooms.child(
                button(
                    ("zoom", i),
                    format!("{}%", (zoom * 100.) as i32),
                    (self.zoom - zoom).abs() < 0.001,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.zoom_at(point(px(0.), px(0.)), zoom / this.zoom);
                    cx.notify();
                })),
            );
        }
        for (mode, label) in [
            (ZoomMode::Hybrid, "Hybrid · worker"),
            (ZoomMode::Sharp, "Sharp · UI thread"),
            (ZoomMode::Texture, "Texture only"),
        ] {
            zooms = zooms.child(button(label, label, self.zoom_mode == mode).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.camera_changed();
                    crate::svg_worker::dispose();
                    if let Some((_, base)) = this.hybrid_base.take()
                        && this
                            .sharp_image
                            .as_ref()
                            .is_none_or(|current| !Arc::ptr_eq(current, &base))
                    {
                        cx.defer(move |cx| cx.drop_image(base, None));
                    }
                    this.zoom_mode = mode;
                    cx.notify();
                }),
            ));
        }
        zooms = zooms
            .child(
                button("reset", "Reset", false).on_click(cx.listener(|this, _, _, cx| {
                    this.reset();
                    cx.notify();
                })),
            )
            .child(
                button(
                    "backdrop",
                    if self.light {
                        "Dark canvas"
                    } else {
                        "Light canvas"
                    },
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.light = !this.light;
                    cx.notify();
                })),
            )
            .child(
                button(
                    "gallery",
                    if self.gallery {
                        "Unmount gallery"
                    } else {
                        "Mount 10 tiles"
                    },
                    self.gallery,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery = !this.gallery;
                    cx.notify();
                })),
            );
        for mib in [8, 32] {
            zooms = zooms.child(
                button(("pool", mib), format!("Pool {mib} MiB"), self.budget == mib).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.budget = mib;
                        this.scheduler = pool(mib);
                        cx.notify();
                    }),
                ),
            );
        }
        let policy = RasterPolicy {
            max_output_bytes: 2 * MIB,
            preview_edge: 384,
            settle_delay: Duration::from_millis(180),
        };
        let backdrop = rgb(if self.light { 0xf8fafc } else { 0x172238 });
        let mut comparison = div().flex().gap_3().flex_shrink_0();
        for mode in 0..2 {
            let entity = cx.entity().downgrade();
            let mut viewport = div()
                .id(("svg-viewport", mode))
                .relative()
                .w_full()
                .h(px(300.))
                .flex_shrink_0()
                .overflow_hidden()
                .bg(backdrop)
                .cursor_crosshair()
                .on_scroll_wheel(cx.listener(move |this, e: &ScrollWheelEvent, _, cx| {
                    let delta = f32::from(e.delta.pixel_delta(px(24.)).y);
                    this.zoom_at(
                        e.position - this.bounds[mode].center(),
                        (delta * 0.002).clamp(-2., 2.).exp(),
                    );
                    this.drag = None;
                    cx.stop_propagation();
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, _, cx| {
                        this.drag = Some(e.position);
                        cx.stop_propagation();
                    }),
                );
            if mode == 0 {
                viewport = viewport
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                let _ = entity.update(cx, |this, _| this.bounds[mode] = bounds);
                            },
                            |_, _, _, _| (),
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(
                        svg_image("hero", self.docs[self.selected].clone())
                            .scheduler(self.scheduler.clone())
                            .policy(policy)
                            .zoom(self.zoom)
                            .pan(self.pan)
                            .on_status_change(cx.listener(|this, s: &SvgStatus, _, cx| {
                                this.status = Some(s.clone());
                                cx.notify();
                            })),
                    );
            } else {
                viewport = viewport.child(
                    canvas(
                        move |bounds, window, cx| {
                            entity
                                .update(cx, |this, cx| {
                                    this.bounds[mode] = bounds;
                                    this.sharp_view(bounds, window.scale_factor(), cx);
                                    this.displayed_raster(bounds)
                                })
                                .ok()
                                .flatten()
                        },
                        move |bounds, image, window, _| {
                            if let Some((image, image_bounds)) = image {
                                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                    if let Err(error) = window.paint_image(
                                        image_bounds,
                                        image_bounds,
                                        Corners::default(),
                                        image,
                                        0,
                                        false,
                                    ) {
                                        web_sys::console::error_1(
                                            &format!("Viewport SVG paint: {error}").into(),
                                        );
                                    }
                                });
                            }
                        },
                    )
                    .size_full(),
                );
            }
            comparison = comparison.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(if mode == 0 {
                        "Bounded · 2 MiB output cap"
                    } else {
                        match self.zoom_mode {
                            ZoomMode::Hybrid => "Hybrid · texture preview + worker detail",
                            ZoomMode::Sharp => "Rerasterized · UI thread",
                            ZoomMode::Texture => "Texture zoom · cached SVG raster",
                        }
                    })
                    .child(viewport),
            );
        }
        let right_mode = match self.zoom_mode {
            ZoomMode::Hybrid => self.hybrid_state,
            ZoomMode::Sharp => "Sharp (UI thread)",
            ZoomMode::Texture => "Texture only",
        };
        let sharp_progress = if let Some(error) = &self.sharp_error {
            format!("{right_mode}: {error}")
        } else if let Some(image) = &self.sharp_image {
            let pixels = image.size(0);
            format!(
                "{right_mode}: {} × {} px • {:.2} MiB output • {} rasters • last {:.1} ms",
                i32::from(pixels.width),
                i32::from(pixels.height),
                (i32::from(pixels.width) as f64 * i32::from(pixels.height) as f64 * 4.)
                    / MIB as f64,
                self.sharp_rasters,
                self.sharp_ms
            )
        } else {
            format!("{right_mode}: no completed raster yet")
        };
        let progress = self
            .status
            .as_ref()
            .map(|s| {
                format!(
                    "{:?} • displayed {} • {} rasters • last {:.1} ms • {}",
                    s.phase,
                    s.displayed
                        .map(|p| format!("{} × {}", i32::from(p.width), i32::from(p.height)))
                        .unwrap_or_else(|| "waiting".into()),
                    s.completed_rasters,
                    s.raster_ms.unwrap_or(0.),
                    s.error.as_deref().unwrap_or("")
                )
            })
            .unwrap_or_else(|| "Waiting for layout".into());
        let stats = self.scheduler.stats();
        let mut content = div()
            .id("svg-demo")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .bg(rgb(0x111827))
            .text_color(rgb(0xeff4fc))
            .font_family("IBM Plex Sans")
            .flex()
            .flex_col()
            .gap_3()
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if e.pressed_button != Some(MouseButton::Left) {
                    this.drag = None;
                } else if let Some(previous) = this.drag {
                    this.drag = Some(e.position);
                    this.camera_changed();
                    this.pan += e.position - previous;
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if this.drag.take().is_some() { this.camera_changed(); cx.notify(); }
            }))
            .child(div().text_xl().child("Progressive SVG · WebAssembly"))
            .child("Shared camera: wheel over either view to zoom; drag to pan both. Scroll outside the artwork to explore the gallery.")
            .child("Hybrid uses an isolated Web Worker for sharp renders. The bounded left view and gallery still use the browser thread. Comparison output is outside the pool; pool accounting is not total browser memory.")
            .child(samples)
            .child(zooms)
            .child(div().p_3().rounded_md().bg(rgb(0x273449)).child(match self.zoom_mode {
                ZoomMode::Hybrid => "Hybrid: GPU texture zoom/pan while moving; after 160 ms idle a Web Worker renders sharp visible pixels. Old results cannot replace a newer camera. A cached full-artwork base avoids holes while moving. The bounded left view can still refine on the UI thread.",
                ZoomMode::Sharp => "Sharp: the right view rerasterizes synchronously on zoom/pan. Deep zoom and shadows can freeze the UI for seconds. Compare with Hybrid to move that work to a worker.",
                ZoomMode::Texture => "Texture only: the right view scales a cached base raster with the GPU. Zoom/pan does not rerasterize it; detail softens. Artwork changes or resizing rebuild the base raster. The bounded left view may still refine.",
            }))
            .child(comparison)
            .child(sharp_progress)
            .child(format!("Zoom {:.0}% • {progress}", self.zoom * 100.))
            .child(format!(
                "Pool {} MiB • {} members • {} active / {} queued • {:.2} MiB accounted • peak {:.2} MiB",
                self.budget, stats.members, stats.active_jobs, stats.queued_jobs,
                stats.managed_bytes as f64 / MIB as f64,
                stats.peak_managed_bytes as f64 / MIB as f64
            ))
            .child("Artwork: xgpui contributors (Apache-2.0); AJ Ashton and rg1024 (public domain); Ebaychatter0 (CC BY-SA 3.0). Full notices: Artwork credits link below.");
        if self.gallery {
            content = content.child(div().flex().flex_wrap().gap_3().children((0..10).map(|i| {
                let index = i % self.docs.len();
                div()
                    .w(px(260.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(SAMPLES[index].0)
                    .child(
                        svg_image(("tile", i), self.docs[index].clone())
                            .scheduler(self.scheduler.clone())
                            .policy(policy)
                            .h(px(190.))
                            .zoom(self.zoom)
                            .on_status_change(cx.listener(|_, _: &SvgStatus, _, cx| cx.notify()))
                            .bg(backdrop),
                    )
            })));
        }
        content
    }
}

fn decode_worker_reply(
    key: ViewportKey,
    reply: wasm_bindgen::JsValue,
) -> Result<(Arc<RenderImage>, f64), String> {
    use wasm_bindgen::{JsCast, JsValue};
    let field =
        |name| js_sys::Reflect::get(&reply, &JsValue::from_str(name)).map_err(|e| format!("{e:?}"));
    let data = field("bytes")?;
    if !data.is_instance_of::<js_sys::Uint8Array>() {
        return Err("Worker returned invalid pixel data".into());
    }
    let bytes = js_sys::Uint8Array::new(&data).to_vec();
    let width = i32::from(key.pixels.width) as u32;
    let height = i32::from(key.pixels.height) as u32;
    let expected = width as usize * height as usize * 4;
    if bytes.len() != expected || bytes.len() > 32 * MIB {
        return Err("Worker pixel dimensions do not match the request".into());
    }
    let buffer = image::RgbaImage::from_raw(width, height, bytes).ok_or("Invalid worker image")?;
    let ms = field("ms")?
        .as_f64()
        .filter(|ms| ms.is_finite() && *ms >= 0.)
        .ok_or("Invalid worker timing")?;
    Ok((
        Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])),
        ms,
    ))
}
