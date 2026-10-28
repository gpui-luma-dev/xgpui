//! Supplied RGBA16F pixels, with explicit mapped preview and native wide output.
use anyhow::Result;
use gpui::{prelude::*, *};
use xgpui_media::{
    ImageJob, LoadStatus, MediaImage, MediaLoader, image_view,
    pipeline::{ColorMatrix, ImageAdjustments, ImageColor, ImagePrimaries, Vignette},
};

struct FloatLab {
    loader: MediaLoader,
    job: ImageJob,
    image: Option<MediaImage>,
    exposure: usize,
    saturation: usize,
    vignette: bool,
    opacity: bool,
    wide: bool,
}
fn pixels(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let u = (i % width) as f32 / (width - 1) as f32;
            let v = (i / width) as f32 / (height - 1) as f32;
            let alpha = ((0.49 - (u - 0.5).abs().max((v - 0.5).abs())) * 60.).clamp(0., 1.);
            let rgb = if v < 0.33 {
                [1. - u, u, 0.02]
            } else if v < 0.66 {
                [u * 0.12, u * 0.12, u * 0.12]
            } else {
                [0.05, 1. - u, u]
            };
            [rgb[0], rgb[1], rgb[2], alpha]
        })
        .collect()
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
impl Render for FloatLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.image.is_none() {
            self.image = self.job.try_take();
            if self.job.status() == LoadStatus::Loading {
                window.request_animation_frame();
            }
        }
        let status = window.color_status();
        let accepted = status.effective() == WindowColorMode::WideGamutSdr;
        let matrix = ColorMatrix::identity(ImagePrimaries::DisplayP3)
            .exposure([0., -1., 1., 2.][self.exposure])
            .unwrap()
            .saturation([1., 0., 1.8][self.saturation])
            .unwrap();
        let adjustments = ImageAdjustments::new(
            matrix,
            Some(Vignette::new(if self.vignette { 0.65 } else { 0. }, 0.35, 0.65, 1.).unwrap()),
        )
        .unwrap();
        let mut panels = div().flex().gap_4().w_full().h(px(440.));
        if let Some(image) = &self.image {
            for wide in [false, self.wide && accepted] {
                let view = image_view(image.clone())
                    .adjustments(adjustments)
                    .w_full()
                    .h(px(360.))
                    .rounded_xl()
                    .opacity(if self.opacity { 0.5 } else { 1. });
                let view = if wide {
                    view.wide_gamut().unwrap()
                } else {
                    view
                };
                panels = panels.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .bg(rgb(0x253342))
                        .p_3()
                        .child(if wide {
                            "Native linear Display P3"
                        } else {
                            "Clipped sRGB preview"
                        })
                        .child(view),
                );
            }
        } else {
            panels = panels.child(format!("CPU source: {:?}", self.job.status()));
        }
        div().size_full().p_6().flex().flex_col().gap_4().bg(rgb(0x0f172a)).text_color(rgb(0xe2e8f0))
            .child(div().text_xl().child(if cfg!(debug_assertions) { "RGBA16F images · DEBUG" } else { "RGBA16F images · RELEASE" }))
            .child("Generated linear Display P3 pixels · saturated colors / near-black gradient / transparent edges")
            .child(format!("Window requested {:?} · effective {:?} · display wide: {:?}",
                status.requested(),status.effective(),status.display_wide_gamut()))
            .child(div().flex().flex_wrap().gap_2()
                .child(button("float-exposure",format!("Exposure {:+.0} EV",[0.,-1.,1.,2.][self.exposure])).on_click(cx.listener(|s,_,_,cx|{s.exposure=(s.exposure+1)%4;cx.notify();})))
                .child(button("float-saturation",format!("Saturation ×{:.1}",[1.,0.,1.8][self.saturation])).on_click(cx.listener(|s,_,_,cx|{s.saturation=(s.saturation+1)%3;cx.notify();})))
                .child(button("float-vignette","Toggle vignette").on_click(cx.listener(|s,_,_,cx|{s.vignette= !s.vignette;cx.notify();})))
                .child(button("float-opacity","Toggle opacity").on_click(cx.listener(|s,_,_,cx|{s.opacity= !s.opacity;cx.notify();})))
                .child(button("float-output",if self.wide{"Right: native wide"}else{"Right: sRGB preview"}).on_click(cx.listener(|s,_,_,cx|{s.wide= !s.wide;cx.notify();})))
                .child(button("float-reset","Reset").on_click(cx.listener(|s,_,_,cx|{s.exposure=0;s.saturation=0;s.vignette=false;s.opacity=false;s.wide=true;cx.notify();}))))
            .child(panels)
            .child(self.image.as_ref().map(|i|format!("1280 × 800 · {:?} · {:?} · preparations {} · {:?}",i.format(),i.source_color(),i.upload_count(),i.upload_status())).unwrap_or_default())
            .child(format!("Staging pool: {:?}",self.loader.staging_pool_stats()))
            .child("Both views share one retained RGBA16F texture. Adjustments run in linear P3; output clips once to the selected gamut.")
            .child("A screenshot is an sRGB preview, not evidence of the display’s wider gamut. Unsupported native wide presentation is labeled as preview.")
    }
}
pub fn run() -> Result<()> {
    let loader = MediaLoader::default();
    let job = loader.load_rgba16f(1280, 800, pixels(1280, 800), ImageColor::LINEAR_P3)?;
    let lab = FloatLab {
        loader,
        job,
        image: None,
        exposure: 0,
        saturation: 0,
        vignette: false,
        opacity: false,
        wide: true,
    };
    gpui_platform::application().run(move |cx: &mut App| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1360.), px(820.)),
                    cx,
                ))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title(if cfg!(debug_assertions) {
                    "XGPUI RGBA16F Lab — DEBUG"
                } else {
                    "XGPUI RGBA16F Lab — RELEASE"
                });
                if let Err(e) = window.set_color_mode(WindowColorMode::WideGamutSdr) {
                    eprintln!("Wide presentation unavailable: {e:#}");
                }
                cx.new(|_| lab)
            },
        )
        .expect("open RGBA16F lab");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit()
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}
