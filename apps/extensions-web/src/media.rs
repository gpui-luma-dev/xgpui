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
use xgpui_media::{
    ImageFilter,
    pipeline::{ColorMatrix, ImageAdjustments, ImagePrimaries, Vignette},
};
use xgpui_svg::path_effects::{Contour, MeasuredPath, PathEffect};

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Filters,
    FloatImages,
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
    adjustments: Option<ImageAdjustments>,
}
impl CustomContent for Placement {
    fn image_color_mode(&self) -> ImageColorMode {
        if self.source.data.format() == ImageTextureFormat::LinearP3Rgba16Float {
            ImageColorMode::LinearP3Preview
        } else {
            ImageColorMode::EncodedSrgb
        }
    }

    fn image_linear_adjustments(&self) -> Option<gpui::LinearImagePaint> {
        self.adjustments.map(|v| v.paint())
    }
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
    linear_mode: bool,
    adjustments: [usize; 4],
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
        let (width, height) = if matches!(mode, Mode::Filters | Mode::FloatImages) {
            (1280, 800)
        } else {
            (1024, 1024)
        };
        let pixels = image::RgbaImage::from_fn(width, height, |x, y| {
            let u = x as f32 / width as f32;
            let v = y as f32 / height as f32;
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
        let data = if mode == Mode::FloatImages {
            let rgba = (0..width * height)
                .map(|i| {
                    let u = (i % width) as f32 / (width - 1) as f32;
                    let v = (i / width) as f32 / (height - 1) as f32;
                    let a = ((0.49 - (u - 0.5).abs().max((v - 0.5).abs())) * 60.).clamp(0., 1.);
                    let c = if v < 0.33 {
                        [1. - u, u, 0.02]
                    } else if v < 0.66 {
                        [u * 0.12; 3]
                    } else {
                        [0.05, 1. - u, u]
                    };
                    [c[0], c[1], c[2], a]
                })
                .collect();
            xgpui_media::pipeline::rgba16f_data(
                width,
                height,
                rgba,
                xgpui_media::pipeline::ImageColor::LINEAR_P3,
            )?
        } else {
            xgpui_media::mipmapped_rgba8(width, height, pixels.as_raw().clone())?
        };
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
            linear_mode: matches!(mode, Mode::Filters | Mode::FloatImages),
            adjustments: [0; 4],
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
    fn image_panel(
        &self,
        atlas: bool,
        filter: ImageFilter,
        zoom: f32,
        reference: bool,
        adjustments: Option<ImageAdjustments>,
    ) -> Div {
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
            } else if self.mode == Mode::FloatImages {
                if reference {
                    "RGBA16F P3 · sRGB preview"
                } else {
                    "Linear P3 adjustments · sRGB preview"
                }
            } else if self.mode == Mode::Filters && reference {
                "Original · no filter"
            } else if self.mode == Mode::Filters {
                if self.linear_mode {
                    "Linear-sRGB adjustments"
                } else {
                    "Encoded-sRGB filters"
                }
            } else {
                "Dedicated source · 11 mip levels"
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        let w = source.data.width() as f32;
                        let h = source.data.height() as f32;
                        let fit = (f32::from(bounds.size.width) / w)
                            .min(f32::from(bounds.size.height) / h)
                            * zoom;
                        let extent = size(px(w * fit), px(h * fit));
                        let image_bounds = Bounds::new(
                            bounds.center() - point(extent.width / 2., extent.height / 2.),
                            extent,
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
                                            adjustments,
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
                Mode::FloatImages => "RGBA16F / Display P3 · explicit sRGB preview",
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
            if self.mode == Mode::Filters && !self.linear_mode {
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
            let linear_adjustments =
                if matches!(self.mode, Mode::Filters | Mode::FloatImages) && self.linear_mode {
                    for (index, (id, label, count)) in [
                        (
                            "exposure",
                            format!("Exposure {:+.0} EV", [0., -1., 1., 2.][self.adjustments[0]]),
                            4,
                        ),
                        (
                            "linear-contrast",
                            format!(
                                "Linear contrast ×{:.1}",
                                [1., 0.6, 1.5][self.adjustments[1]]
                            ),
                            3,
                        ),
                        (
                            "linear-saturation",
                            format!(
                                "Linear saturation ×{:.1}",
                                [1., 0., 1.8][self.adjustments[2]]
                            ),
                            3,
                        ),
                        (
                            "vignette",
                            format!("Vignette {}%", [0, 40, 80][self.adjustments[3]]),
                            3,
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        controls = controls.child(button(id, label).on_click(cx.listener(
                            move |s, _, _, cx| {
                                s.adjustments[index] = (s.adjustments[index] + 1) % count;
                                cx.notify();
                            },
                        )));
                    }
                    controls =
                        controls.child(button("reset-adjustments", "Reset adjustments").on_click(
                            cx.listener(|s, _, _, cx| {
                                s.adjustments = [0; 4];
                                s.animate = false;
                                cx.notify();
                            }),
                        ));
                    let ev = [0., -1., 1., 2.][self.adjustments[0]]
                        + if animated {
                            (f64::from(time) * 1.5).sin() * 0.8
                        } else {
                            0.
                        };
                    let matrix = ColorMatrix::identity(if self.mode == Mode::FloatImages {
                        ImagePrimaries::DisplayP3
                    } else {
                        ImagePrimaries::Srgb
                    })
                    .exposure(ev)
                    .unwrap()
                    .contrast([1., 0.6, 1.5][self.adjustments[1]], 0.18)
                    .unwrap()
                    .saturation([1., 0., 1.8][self.adjustments[2]])
                    .unwrap();
                    Some(
                        ImageAdjustments::new(
                            matrix,
                            Some(
                                Vignette::new([0., 0.4, 0.8][self.adjustments[3]], 0.35, 0.65, 1.)
                                    .unwrap(),
                            ),
                        )
                        .unwrap(),
                    )
                } else {
                    None
                };
            if self.mode == Mode::Filters {
                controls = controls.child(
                    button(
                        "linear-mode",
                        if self.linear_mode {
                            "Use encoded filters"
                        } else {
                            "Use linear adjustments"
                        },
                    )
                    .on_click(cx.listener(|s, _, _, cx| {
                        s.linear_mode = !s.linear_mode;
                        cx.notify();
                    })),
                );
            }
            if self.mode == Mode::FloatImages {
                root=root.child("RGBA16F storage · premultiplied linear Display P3 / D65 · both panels clip to sRGB. Browser wide-gamut presentation is not enabled.");
            }
            root=root.child(if matches!(self.mode,Mode::Filters | Mode::FloatImages) { "1280 × 800 generated RGBA fixture · transparent edges · no external assets" } else { "1024² generated RGBA fixture · transparent edges · no external assets" })
                .child(controls)
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("zoom",format!("Zoom {:.1}%",[90.,50.,25.,12.5][self.zoom])).on_click(cx.listener(|s,_,_,cx|{s.zoom=(s.zoom+1)%4;cx.notify();})))
                    .child(button("opacity",format!("Opacity {}%",[100,65,30][self.opacity])).on_click(cx.listener(|s,_,_,cx|{s.opacity=(s.opacity+1)%3;cx.notify();})))
                    .child(button("corners",if self.corners{"Corners ON"}else{"Corners OFF"}).on_click(cx.listener(|s,_,_,cx|{s.corners = !s.corners;cx.notify();}))))
                .child(div().flex().gap_3().child(self.image_panel(self.mode==Mode::Mipmaps,ImageFilter::new(),zoom,true,None)).child(self.image_panel(false,if self.mode==Mode::Filters && !self.linear_mode{self.filter()}else{ImageFilter::new()},zoom,false,linear_adjustments)))
                .child(format!("Dedicated texture preparations: {} · cached across redraws and controls",self.source.preparations.load(Ordering::Relaxed)))
                .child(if self.mode==Mode::Mipmaps{"Fine patterns reveal aliasing while shrinking. Mips use alpha-aware linear-light area filtering; brightness can differ from atlas sampling."}else if self.linear_mode {"Exposure in stops → linear contrast around 0.18 → working-space saturation → aspect-correct vignette → SDR clipping. Alpha is preserved. Animate sweeps exposure and zoom; preparations should stay at one."} else {"Brightness / contrast / saturation / hue / tint compose in encoded sRGB, preserve alpha and add no offscreen pass. Click each control to cycle presets."})
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
