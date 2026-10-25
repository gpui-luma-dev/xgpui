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
pub struct SvgDemo {
    docs: Vec<SvgDocument>,
    selected: usize,
    zoom: f32,
    pan: Point<Pixels>,
    bounds: [Bounds<Pixels>; 2],
    sharp_key: Option<ViewportKey>,
    sharp_image: Option<Arc<RenderImage>>,
    sharp_ms: f64,
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
            sharp_ms: 0.,
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
            zoom: self.zoom,
            pan: self.pan,
            pixels,
            logical: bounds.size,
        };
        if self.sharp_key != Some(key) {
            self.sharp_key = Some(key);
            let intrinsic = self.docs[self.selected].size();
            let factor = (f32::from(bounds.size.width) / intrinsic.width)
                .min(f32::from(bounds.size.height) / intrinsic.height)
                * self.zoom;
            let view_box = Bounds::new(
                point(
                    intrinsic.width / 2.
                        - (f32::from(bounds.size.width) / 2. + f32::from(self.pan.x)) / factor,
                    intrinsic.height / 2.
                        - (f32::from(bounds.size.height) / 2. + f32::from(self.pan.y)) / factor,
                ),
                size(
                    f32::from(bounds.size.width) / factor,
                    f32::from(bounds.size.height) / factor,
                ),
            );
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
                    self.sharp_image = Some(image);
                    self.sharp_error = None;
                }
                Err(error) => self.sharp_error = Some(error.to_string()),
            }
            cx.notify();
        }
        self.sharp_image.clone()
    }
    fn reset(&mut self) {
        self.zoom = 1.;
        self.pan = point(px(0.), px(0.));
        self.drag = None;
    }
    fn zoom_at(&mut self, anchor: Point<Pixels>, factor: f32) {
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
                                    this.sharp_view(bounds, window.scale_factor(), cx)
                                })
                                .ok()
                                .flatten()
                        },
                        |bounds, image, window, _| {
                            if let Some(image) = image {
                                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                    if let Err(error) = window.paint_image(
                                        bounds,
                                        bounds,
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
                        "Rerasterized · viewport pixels"
                    })
                    .child(viewport),
            );
        }
        let sharp_progress = if let Some(error) = &self.sharp_error {
            format!("Rerasterized: {error}")
        } else if let Some(image) = &self.sharp_image {
            let pixels = image.size(0);
            format!(
                "Rerasterized: {} × {} px • {:.2} MiB output • last {:.1} ms",
                i32::from(pixels.width),
                i32::from(pixels.height),
                (i32::from(pixels.width) as f64 * i32::from(pixels.height) as f64 * 4.)
                    / MIB as f64,
                self.sharp_ms
            )
        } else {
            "Rerasterized: waiting for layout".into()
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
                    this.pan += e.position - previous;
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.drag = None))
            .child(div().text_xl().child("Progressive SVG · WebAssembly"))
            .child("Shared camera: wheel over either view to zoom; drag to pan both. Scroll outside the artwork to explore the gallery.")
            .child("Single-threaded Wasm: scheduled raster jobs run on the browser thread and cannot be interrupted. The sharp comparison rerasterizes visible pixels outside the pool; pool accounting is not total browser memory.")
            .child(samples)
            .child(zooms)
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
