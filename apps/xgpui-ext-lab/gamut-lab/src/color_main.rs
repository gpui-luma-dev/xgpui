//! Native P3 and mapped sRGB comparison using the browser's reference samples.
#[path = "../../extensions-web/src/color_samples.rs"]
mod color_samples;
use gpui::{prelude::*, *};
use xgpui_color::{EncodedRgb, GamutMapping, RgbSpace};
use xgpui_gamut::{WideColor, instanced::WidePainter};

struct ColorLab {
    preset: usize,
    policy: GamutMapping,
    translucent: bool,
    painter: WidePainter,
}
impl Render for ColorLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = window.color_status();
        let srgb = color_samples::rows(self.preset, self.policy, self.translucent).unwrap();
        let p3 = color_samples::rows_in_space(
            self.preset,
            self.policy,
            self.translucent,
            RgbSpace::DisplayP3,
        )
        .unwrap();
        div().id("native-color").size_full().overflow_y_scroll().p_6().flex().flex_col().gap_4().bg(rgb(0x0f172a)).text_color(rgb(0xe2e8f0))
            .child(div().text_xl().child("Color / Gamut · Native P3"))
            .child(format!("Requested: {:?} · Effective: {:?} · Display wide gamut: {:?}", status.requested(), status.effective(), status.display_wide_gamut()))
            .child(div().flex().gap_4()
                .child(div().id("preset").cursor_pointer().child(format!("Colors: {}", color_samples::PRESETS[self.preset])).on_click(cx.listener(|this,_,_,cx| { this.preset=(this.preset+1)%3; cx.notify(); })))
                .child(div().id("mapping").cursor_pointer().child(format!("Mapping: {:?}",self.policy)).on_click(cx.listener(|this,_,_,cx| {this.policy=if this.policy==GamutMapping::Clip {GamutMapping::ReduceChroma} else {GamutMapping::Clip};cx.notify();})))
                .child(div().id("alpha").cursor_pointer().child(format!("End alpha: {}",if self.translucent {"25%"} else {"100%"})).on_click(cx.listener(|this,_,_,cx|{this.translucent=!this.translucent;cx.notify();}))))
            .children(srgb.into_iter().zip(p3).map(|(srgb,p3)| {
                div().flex().flex_col().gap_2()
                    .child(srgb.label)
                    .child("Mapped sRGB")
                    .child(div().flex().w_full().h(px(46.)).children(srgb.rgba.into_iter().map(|[r,g,b,a]| div().flex_1().h_full().bg(Rgba{r,g,b,a}))))
                    .child("Native Display P3")
                    .child(div().flex().w_full().h(px(46.)).children(p3.rgba.into_iter().map(|[r,g,b,a]| {
                        let rgb=EncodedRgb::new(RgbSpace::DisplayP3,[r as f64,g as f64,b as f64]).unwrap().decode().unwrap().components();
                        let color=WideColor::linear_p3([rgb[0] as f32,rgb[1] as f32,rgb[2] as f32,a]).unwrap();
                        let painter=self.painter.clone();
                        div().flex_1().h_full().child(canvas(|_,_,_|(),move |bounds,_,window,_| {painter.paint_color(window,bounds,px(0.),&color).expect("native P3 swatch");}).size_full())
                    })))
            }))
            .child("32 discrete reference samples per row, shared with the Wasm demo. P3 samples map to P3; sRGB samples map to sRGB. Alpha composites over the same background. Native P3 requires effective WideGamutSdr; display reporting does not verify calibration.")
    }
}
fn main() -> anyhow::Result<()> {
    let painter = WidePainter::new()?;
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(1050.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window
                    .set_color_mode(WindowColorMode::WideGamutSdr)
                    .expect("native P3 presentation");
                window.set_window_title("XGPUI Color Lab — native P3 comparison");
                cx.new(|_| ColorLab {
                    preset: 1,
                    policy: GamutMapping::Clip,
                    translucent: false,
                    painter,
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
    Ok(())
}
