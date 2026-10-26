//! Browser presentation uses shared public media math and backend image SPI.
//! No native loader threads, browser video decoder, or DOM/CSS image emulation.
use gpui::{prelude::*, *};
use gpui_wgpu::custom_draw::{WgpuCustomContext, WgpuCustomOutput};
use std::{
    any::Any,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use web_time::Instant;
use xgpui_media::ImageFilter;
use xgpui_svg::path_effects::{Contour, MeasuredPath, PathEffect};

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Filters,
    Mipmaps,
    Paths,
}
#[derive(Debug)]
struct Source {
    data: ImageTextureData,
    cached: Mutex<Option<WgpuCustomOutput>>,
    preparations: AtomicUsize,
}
#[derive(Debug)]
struct Placement {
    source: Arc<Source>,
    filter: ImageFilter,
}
impl CustomContent for Placement {
    fn image_color_transform(&self) -> [[f32; 4]; 3] {
        self.filter.rows()
    }
    fn prepare(&self, backend: &dyn Any, _: Size<DevicePixels>) -> anyhow::Result<Box<dyn Any>> {
        let host = backend
            .downcast_ref::<WgpuCustomContext>()
            .ok_or_else(|| anyhow::anyhow!("browser media requires wgpu"))?;
        let mut cached = self.source.cached.lock().unwrap();
        if let Some(output) = cached.as_ref().filter(|v| host.can_reuse(v)) {
            return Ok(Box::new(output.clone()));
        }
        let output = host.create_image_output(&self.source.data)?;
        *cached = Some(output.clone());
        self.source.preparations.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(output))
    }
}
pub struct MediaDemo {
    mode: Mode,
    source: Arc<Source>,
    atlas: Arc<RenderImage>,
    filters: [usize; 5],
    zoom: usize,
    opacity: usize,
    corners: bool,
    animate: bool,
    started: Instant,
    trim: usize,
    dash: usize,
    phase: f32,
    width: usize,
    paths: Vec<MeasuredPath>,
}
fn button(id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0x334155))
        .hover(|s| s.bg(rgb(0x475569)))
        .cursor_pointer()
        .child(label)
}
impl MediaDemo {
    pub fn new(mode: Mode) -> anyhow::Result<Self> {
        let pixels = image::RgbaImage::from_fn(1024, 1024, |x, y| {
            let u = x as f32 / 1024.;
            let v = y as f32 / 1024.;
            let a = (((0.48 - ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt()) * 70.).clamp(0., 1.)
                * 255.) as u8;
            let fine = if (x / 2 + y / 2) % 2 == 0 { 0.3 } else { 1. };
            image::Rgba([
                ((0.15 + u * 0.85) * fine * 255.) as u8,
                ((0.2 + v * 0.8) * fine * 255.) as u8,
                ((1. - u * 0.6) * fine * 255.) as u8,
                a,
            ])
        });
        let data = xgpui_media::mipmapped_rgba8(1024, 1024, pixels.as_raw().clone())?;
        let mut bgra = pixels;
        for p in bgra.pixels_mut() {
            p.0.swap(0, 2);
        }
        let p = |x, y| point(px(x), px(y));
        let contours = [
            vec![
                Contour::new(p(20., 100.))
                    .cubic_to(p(60., -30.), p(140., 210.), p(190., 85.))
                    .quadratic_to(p(250., -20.), p(300., 100.)),
            ],
            vec![
                Contour::new(p(160., 150.))
                    .cubic_to(p(120., 115.), p(20., 60.), p(70., 30.))
                    .cubic_to(p(105., 5.), p(145., 20.), p(160., 50.))
                    .cubic_to(p(175., 20.), p(215., 5.), p(250., 30.))
                    .cubic_to(p(300., 60.), p(200., 115.), p(160., 150.))
                    .close(),
            ],
            vec![
                Contour::new(p(20., 45.))
                    .line_to(p(110., 45.))
                    .line_to(p(110., 120.))
                    .line_to(p(20., 120.))
                    .close(),
                Contour::new(p(170., 120.))
                    .quadratic_to(p(170., 5.), p(230., 45.))
                    .cubic_to(p(300., -5.), p(320., 120.), p(250., 135.)),
            ],
        ];
        Ok(Self {
            mode,
            source: Arc::new(Source {
                data,
                cached: Mutex::new(None),
                preparations: AtomicUsize::new(0),
            }),
            atlas: Arc::new(RenderImage::new(vec![image::Frame::new(bgra)])),
            filters: [0; 5],
            zoom: 0,
            opacity: 0,
            corners: true,
            animate: false,
            started: Instant::now(),
            trim: 1,
            dash: 1,
            phase: 0.,
            width: 0,
            paths: contours
                .iter()
                .map(|v| MeasuredPath::new(v, px(0.05)))
                .collect::<anyhow::Result<_>>()?,
        })
    }
    fn filter(&self) -> ImageFilter {
        ImageFilter::new()
            .brightness([1., 0.6, 1.4][self.filters[0]])
            .contrast([1., 0.6, 1.5][self.filters[1]])
            .saturation([1., 0., 1.8][self.filters[2]])
            .hue_rotate([0., 90., 180.][self.filters[3]])
            .tint([[0., 0., 0., 0.], [1., 0.4, 0.1, 0.3], [0.1, 0.6, 1., 0.3]][self.filters[4]])
    }
    fn image_panel(&self, atlas: bool, filter: ImageFilter, zoom: f32, reference: bool) -> Div {
        let source = self.source.clone();
        let original = self.atlas.clone();
        let opacity = [1., 0.65, 0.3][self.opacity];
        let radius = if self.corners { 20. } else { 0. };
        div()
            .flex_1()
            .min_w_0()
            .flex_shrink_0()
            .rounded_lg()
            .bg(rgb(0x253342))
            .overflow_hidden()
            .child(div().h(px(48.)).child(if atlas {
                "Atlas · single level"
            } else if self.mode == Mode::Filters && reference {
                "Original · no filter"
            } else if self.mode == Mode::Filters {
                "Filtered source"
            } else {
                "Dedicated source · 11 mip levels"
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        let side = bounds.size.width.min(bounds.size.height) * zoom;
                        let image_bounds = Bounds::new(
                            bounds.center() - point(side / 2., side / 2.),
                            size(side, side),
                        );
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            window.with_element_opacity(Some(opacity), |window| {
                                if atlas {
                                    window
                                        .paint_image(
                                            image_bounds,
                                            image_bounds,
                                            Corners::all(px(radius)),
                                            original.clone(),
                                            0,
                                            false,
                                        )
                                        .expect("atlas fixture");
                                } else {
                                    window.paint_custom(
                                        CustomGeometry::RoundedRect {
                                            bounds: image_bounds,
                                            corner_radii: Corners::all(px(radius)),
                                        },
                                        Arc::new(Placement {
                                            source: source.clone(),
                                            filter,
                                        }),
                                    );
                                }
                            })
                        });
                    },
                )
                .w_full()
                .h(px(330.)),
            )
    }
    fn path_panel(&self, measured: &MeasuredPath, effect: PathEffect, color: u32) -> Div {
        let built = measured
            .stroke(&effect, px([4., 8., 14.][self.width]))
            .expect("bounded fixture effects");
        div()
            .flex_1()
            .min_w_0()
            .h(px(165.))
            .bg(rgb(0x1e293b))
            .rounded_lg()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        let scale = (f32::from(bounds.size.width) / 340.)
                            .min(f32::from(bounds.size.height) / 180.);
                        let mut path = built.clone();
                        let offset = bounds.origin + point(px(8.), px(5.));
                        for v in &mut path.vertices {
                            v.xy_position = offset + v.xy_position * scale;
                        }
                        path.bounds.origin = offset + path.bounds.origin * scale;
                        path.bounds.size = path.bounds.size.map(|v| v * scale);
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            window.paint_path(path, rgb(color))
                        });
                    },
                )
                .size_full(),
            )
    }
}
impl Render for MediaDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let animated = self.animate && !cx.reduce_motion();
        if animated {
            window.request_animation_frame();
        }
        let time = self.started.elapsed().as_secs_f32();
        let mut root = div()
            .id("media-demo")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(0x0f172a))
            .text_color(rgb(0xe2e8f0))
            .font_family("IBM Plex Sans")
            .child(div().text_xl().child(match self.mode {
                Mode::Filters => "Image filters · WebAssembly",
                Mode::Mipmaps => "Mipmapping · WebAssembly",
                Mode::Paths => "Trim / dash · WebAssembly",
            }));
        if self.mode == Mode::Paths {
            let trim = if animated {
                0.15 + 0.85 * ((time * 0.6).sin() * 0.5 + 0.5)
            } else {
                [1., 0.75, 0.5, 0.25, 0.][self.trim]
            };
            let phase = self.phase + if animated { time * 24. } else { 0. };
            let effect = PathEffect::new().trim(0. ..trim).unwrap();
            let dashed = match self.dash {
                0 => effect.clone(),
                1 => effect.clone().dash(&[px(16.), px(8.)], px(phase)).unwrap(),
                2 => effect.clone().dash(&[px(4.), px(7.)], px(phase)).unwrap(),
                _ => effect
                    .clone()
                    .dash(&[px(18.), px(5.), px(3.)], px(phase))
                    .unwrap(),
            };
            root=root.child("Original / trimmed / trimmed + dashed. Open curves, closed heart, multiple contours.")
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("trim",format!("Trim {:.0}%",trim*100.)).on_click(cx.listener(|s,_,_,cx|{s.trim=(s.trim+1)%5;cx.notify();})))
                    .child(button("dash",["Solid","Dash 16 / 8","Dash 4 / 7","Odd dash 18 / 5 / 3"][self.dash]).on_click(cx.listener(|s,_,_,cx|{s.dash=(s.dash+1)%4;cx.notify();})))
                    .child(button("phase",format!("Phase {:.0}",self.phase)).on_click(cx.listener(|s,_,_,cx|{s.phase=(s.phase+5.).rem_euclid(60.);cx.notify();})))
                    .child(button("width",format!("Stroke {}",[4,8,14][self.width])).on_click(cx.listener(|s,_,_,cx|{s.width=(s.width+1)%3;cx.notify();}))))
                .children(self.paths.iter().map(|p|div().flex().gap_3().child(self.path_panel(p,PathEffect::new(),0x708096)).child(self.path_panel(p,effect.clone(),0x9ee3bd)).child(self.path_panel(p,dashed.clone(),0xffae81))))
                .child("Shared xgpui-svg contour effects before native stroke tessellation; odd dash arrays repeat. Trim uses total length; phase restarts per retained contour.");
        } else {
            let zoom = [0.9, 0.5, 0.25, 0.125][self.zoom]
                * if animated {
                    1. + 0.1 * (time * 0.9).sin()
                } else {
                    1.
                };
            let mut controls = div().flex().flex_wrap().gap_2();
            if self.mode == Mode::Filters {
                for (index, (id, label)) in [
                    ("brightness", "Brightness"),
                    ("contrast", "Contrast"),
                    ("saturation", "Saturation"),
                    ("hue", "Hue"),
                    ("tint", "Tint"),
                ]
                .into_iter()
                .enumerate()
                {
                    controls = controls.child(
                        button(
                            id,
                            match index {
                                0 | 1 => format!(
                                    "{label} ×{:.1}",
                                    [[1., 0.6, 1.4], [1., 0.6, 1.5]][index][self.filters[index]]
                                ),
                                2 => format!("Saturation ×{:.1}", [1., 0., 1.8][self.filters[2]]),
                                3 => format!("Hue {}°", [0, 90, 180][self.filters[3]]),
                                _ => format!("Tint {}", ["off", "warm", "cool"][self.filters[4]]),
                            },
                        )
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.filters[index] = (s.filters[index] + 1) % 3;
                            cx.notify();
                        })),
                    );
                }
                controls = controls.child(button("reset", "Reset filters").on_click(cx.listener(
                    |s, _, _, cx| {
                        s.filters = [0; 5];
                        cx.notify();
                    },
                )));
            }
            root=root.child("1024² generated RGBA fixture · transparent edges · no external assets")
                .child(controls)
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("zoom",format!("Zoom {:.1}%",[90.,50.,25.,12.5][self.zoom])).on_click(cx.listener(|s,_,_,cx|{s.zoom=(s.zoom+1)%4;cx.notify();})))
                    .child(button("opacity",format!("Opacity {}%",[100,65,30][self.opacity])).on_click(cx.listener(|s,_,_,cx|{s.opacity=(s.opacity+1)%3;cx.notify();})))
                    .child(button("corners",if self.corners{"Corners ON"}else{"Corners OFF"}).on_click(cx.listener(|s,_,_,cx|{s.corners = !s.corners;cx.notify();}))))
                .child(div().flex().gap_3().child(self.image_panel(self.mode==Mode::Mipmaps,ImageFilter::new(),zoom,true)).child(self.image_panel(false,if self.mode==Mode::Filters{self.filter()}else{ImageFilter::new()},zoom,false)))
                .child(format!("Dedicated texture preparations: {} · cached across redraws and controls",self.source.preparations.load(Ordering::Relaxed)))
                .child(if self.mode==Mode::Mipmaps{"Fine patterns reveal aliasing while shrinking. Mips use alpha-aware linear-light area filtering; brightness can differ from atlas sampling."}else{"Brightness / contrast / saturation / hue / tint compose in encoded sRGB, preserve alpha and add no offscreen pass. Click each control to cycle presets."})
                .child("Browser fixture generation and first upload are synchronous, once per source. This does not exercise native background decode, staging budgets, or video imports.");
        }
        root.child(
            button(
                "animate",
                if self.animate {
                    "Pause animation"
                } else {
                    "Animate"
                },
            )
            .on_click(cx.listener(|s, _, _, cx| {
                s.animate = !s.animate;
                cx.notify();
            })),
        )
        .child(if cx.reduce_motion() {
            "Reduced motion: animation suppressed."
        } else {
            "SDR sRGB · WebGPU / WebGL2 · animation is optional."
        })
    }
}
