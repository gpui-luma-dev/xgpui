use crate::color_samples::{self, PRESETS};
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_color::GamutMapping;
use xgpui_shader::{Gradient, GradientStop, Paint, painted};
struct Row {
    label: &'static str,
    paint: Paint,
    outside: Vec<bool>,
}
pub struct ColorDemo {
    policy: GamutMapping,
    preset: usize,
    translucent: bool,
    rows: Vec<Row>,
}
impl ColorDemo {
    pub fn new() -> Self {
        let mut demo = Self {
            policy: GamutMapping::Clip,
            preset: 0,
            translucent: false,
            rows: Vec::new(),
        };
        demo.rebuild();
        demo
    }
    fn rebuild(&mut self) {
        self.rows = color_samples::rows(self.preset, self.policy, self.translucent)
            .expect("finite bundled colors")
            .into_iter()
            .map(|row| {
                let denominator = (row.rgba.len() - 1) as f32;
                let stops = row
                    .rgba
                    .into_iter()
                    .enumerate()
                    .map(|(i, rgba)| GradientStop::new(i as f32 / denominator, rgba))
                    .collect();
                Row {
                    label: row.label,
                    paint: Arc::new(Gradient::linear(90., stops).unwrap()).into(),
                    outside: row.outside,
                }
            })
            .collect();
    }
}
fn button(id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0x334155))
        .hover(|s| s.bg(rgb(0x475569)))
        .cursor_pointer()
        .child(label.into())
}
impl Render for ColorDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().id("color-demo").size_full().overflow_y_scroll().p_4().bg(rgb(0x0f172a)).text_color(rgb(0xe2e8f0)).font_family("IBM Plex Sans").flex().flex_col().gap_4()
            .child(div().text_xl().child("Color / Gamut · WebAssembly"))
            .child("Compare interpolation and mapping with the same Display P3 endpoints.")
            .child(div().text_sm().text_color(rgb(0x94a3b8)).child("Mapped sRGB preview · native wide-gamut presentation is not enabled."))
            .child(div().flex().flex_wrap().gap_3()
                .child(button("color-preset",format!("Colors: {}",PRESETS[self.preset])).on_click(cx.listener(|this,_,_,cx| {this.preset=(this.preset+1)%PRESETS.len();this.rebuild();cx.notify();})))
                .child(button("color-mapping",format!("Mapping: {}",if self.policy==GamutMapping::Clip {"Clip"} else {"Reduce chroma"})).on_click(cx.listener(|this,_,_,cx| {this.policy=if this.policy==GamutMapping::Clip {GamutMapping::ReduceChroma} else {GamutMapping::Clip};this.rebuild();cx.notify();})))
                .child(button("color-alpha",if self.translucent {"End alpha: 25%"} else {"End alpha: 100%"}).on_click(cx.listener(|this,_,_,cx| {this.translucent=!this.translucent;this.rebuild();cx.notify();}))))
            .children(self.rows.iter().map(|row| {
                let outside=row.outside.iter().filter(|v|**v).count();
                div().flex().flex_col().gap_2().p_3().rounded_lg().bg(rgb(0x1e293b))
                    .child(div().flex().justify_between().child(row.label).child(format!("{}% outside sRGB before mapping",outside*100/row.outside.len())))
                    .child(painted().w_full().h(px(76.)).rounded_md().bg(rgb(0x334155)).bg_paint(row.paint.clone()))
                    .child(div().flex().w_full().h(px(5.)).children(row.outside.iter().map(|outside|div().flex_1().h_full().bg(rgb(if *outside {0xfbbf24} else {0x475569})))) )
            }))
            .child(div().text_sm().text_color(rgb(0x94a3b8)).child("Amber marks samples outside sRGB before mapping. 32 CPU samples per row; shader paints interpolate between the mapped samples. Alpha is interpolated with premultiplied components; OkLCh hue uses the shorter arc."))
            .child(div().text_sm().child("xgpui-color supplies the math; xgpui-shader displays ordinary sRGB paints. Display calibration and ICC profiles are separate capabilities."))
    }
}
