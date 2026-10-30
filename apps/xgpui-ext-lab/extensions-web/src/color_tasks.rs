//! Task-focused demos: source metadata and gamut simulation in an sRGB canvas.
use crate::color_tasks_model as model;
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_color::{GamutMapping, LinearRgb};

pub struct ColorTasks {
    metadata: bool,
    source: usize,
    patch: usize,
    simulated: usize,
    luminance: usize,
    reduce: bool,
    rec: bool,
    translucent: bool,
    fill: bool,
    boundary: Arc<Vec<[f64; 2]>>,
    cells: Arc<Vec<model::Cell>>,
    sample: Option<model::Sample>,
    error: Option<String>,
}
impl ColorTasks {
    pub fn new(metadata: bool) -> Self {
        let mut demo = Self {
            metadata,
            source: 0,
            patch: if metadata { 2 } else { 0 },
            simulated: 1,
            luminance: 1,
            reduce: false,
            rec: true,
            translucent: false,
            fill: true,
            boundary: Arc::new(model::locus()),
            cells: Arc::new(vec![]),
            sample: None,
            error: None,
        };
        demo.rebuild();
        demo
    }
    fn policy(&self) -> GamutMapping {
        if self.reduce {
            GamutMapping::ReduceChroma
        } else {
            GamutMapping::Clip
        }
    }
    fn rebuild(&mut self) {
        self.error = None;
        if self.metadata {
            match model::sample(
                self.source,
                model::PATCHES[self.patch].1,
                if self.translucent { 0.5 } else { 1. },
                self.rec,
                self.policy(),
            ) {
                Ok(s) => self.sample = Some(s),
                Err(e) => {
                    self.sample = None;
                    self.error = Some(e.to_string());
                }
            }
        } else {
            match model::cells(
                model::SPACES[self.simulated],
                model::LUMINANCE[self.luminance],
                self.policy(),
                &self.boundary,
            ) {
                Ok(c) => self.cells = Arc::new(c),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }
    fn button(&self, label: String, action: usize, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(ElementId::Name(format!("task-{action}").into()))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x334155))
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                match action {
                    0 => this.source = (this.source + 1) % if this.metadata { 8 } else { 4 },
                    1 => this.patch = (this.patch + 1) % model::PATCHES.len(),
                    2 => this.reduce = !this.reduce,
                    3 => {
                        if this.metadata {
                            this.rec = !this.rec
                        } else {
                            this.simulated = (this.simulated + 1) % 4
                        }
                    }
                    4 => {
                        if this.metadata {
                            this.translucent = !this.translucent
                        } else {
                            this.luminance = (this.luminance + 1) % 3
                        }
                    }
                    _ => this.fill = !this.fill,
                }
                this.rebuild();
                cx.notify();
            }))
            .into_any_element()
    }
    fn chart(&self) -> impl IntoElement {
        let boundary = self.boundary.clone();
        let cells = self.cells.clone();
        let fill = self.fill;
        let selected = model::source_xy(model::SPACES[self.source], model::PATCHES[self.patch].1);
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let scale = ((f32::from(bounds.size.width) - 48.) / 0.8)
                    .min((f32::from(bounds.size.height) - 48.) / 0.9)
                    .max(1.);
                let left = bounds.left() + px((f32::from(bounds.size.width) - scale * 0.8) / 2.);
                let bottom = bounds.bottom() - px(24.);
                let pos = |p: [f64; 2]| {
                    point(
                        left + px(p[0] as f32 * scale),
                        bottom - px(p[1] as f32 * scale),
                    )
                };
                if fill {
                    for cell in cells.iter() {
                        window.paint_quad(gpui::fill(
                            Bounds::new(
                                pos([cell.xy[0], cell.xy[1] + 0.9 / 54.]),
                                size(px(scale * 0.8 / 48. + 0.2), px(scale * 0.9 / 54. + 0.2)),
                            ),
                            Rgba {
                                r: cell.rgb[0],
                                g: cell.rgb[1],
                                b: cell.rgb[2],
                                a: 1.,
                            },
                        ));
                        if cell.outside {
                            let mut p = PathBuilder::stroke(px(1.));
                            p.move_to(pos(cell.xy));
                            p.line_to(pos([cell.xy[0] + 0.8 / 48., cell.xy[1] + 0.9 / 54.]));
                            if let Ok(p) = p.build() {
                                window.paint_path(p, rgba(0x10182388));
                            }
                        }
                    }
                }
                let stroke =
                    |points: Vec<[f64; 2]>, color: u32, width: f32, window: &mut Window| {
                        let mut p = PathBuilder::stroke(px(width));
                        for (i, v) in points.into_iter().enumerate() {
                            if i == 0 {
                                p.move_to(pos(v));
                            } else {
                                p.line_to(pos(v));
                            }
                        }
                        if let Ok(p) = p.build() {
                            window.paint_path(p, rgb(color));
                        }
                    };
                for i in 0..=8 {
                    let t = i as f64 / 8.;
                    stroke(vec![[t * 0.8, 0.], [t * 0.8, 0.9]], 0x334155, 1., window);
                    stroke(vec![[0., t * 0.9], [0.8, t * 0.9]], 0x334155, 1., window);
                }
                let mut locus = boundary.as_ref().clone();
                locus.push(locus[0]);
                stroke(locus, 0xa8b4c4, 1.5, window);
                for (i, space) in model::SPACES.into_iter().enumerate() {
                    let mut points: Vec<_> = (0..3)
                        .map(|c| {
                            model::xy(
                                LinearRgb::new(
                                    space,
                                    std::array::from_fn(|j| if j == c { 1. } else { 0. }),
                                )
                                .unwrap()
                                .to_xyz()
                                .unwrap()
                                .components(),
                            )
                            .unwrap()
                        })
                        .collect();
                    points.push(points[0]);
                    stroke(points, model::COLORS[i], 2., window);
                }
                for (p, color) in [(Some([0.3127, 0.3290]), 0xffffff), (selected, 0xffdf62)] {
                    if let Some(p) = p {
                        let delta = 5. / f64::from(scale);
                        stroke(
                            vec![[p[0] - delta, p[1]], [p[0] + delta, p[1]]],
                            color,
                            2.,
                            window,
                        );
                        stroke(
                            vec![[p[0], p[1] - delta], [p[0], p[1] + delta]],
                            color,
                            2.,
                            window,
                        );
                    }
                }
            },
        )
        .w_full()
        .h(px(420.))
        .flex_shrink_0()
    }
}
impl Render for ColorTasks {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let source_label = if self.source == 3 {
            "Rec.2020 (linear)"
        } else {
            model::NAMES[self.source]
        };
        let mut body=div().id("color-tasks").size_full().overflow_y_scroll().p_4().bg(rgb(0x101823)).text_color(rgb(0xe2eaf3)).font_family("IBM Plex Sans").flex().flex_col().gap_3()
            .child(div().text_xl().child(if self.metadata {"Same pixels, different metadata"}else{"Chromaticity & gamut explorer"}))
            .child(if self.metadata {"Change the interpretation without changing the supplied RGB values. Inspect actual CPU-packed FP16 bytes and their sRGB preview."}else{"Compare color-space boundaries and see which colors need mapping. This chart is a simulation, not a measurement of your monitor."})
            .child(div().text_sm().child("Wide gamut is a display capability. Display P3 and Adobe RGB are specific color spaces. This demo renders an sRGB preview."))
            .child(div().flex().flex_wrap().gap_2()
                .child(self.button(format!("Interpretation: {source_label}"),0,cx))
                .child(self.button(format!("Pixels: {}",model::PATCHES[self.patch].0),1,cx))
                .child(self.button(format!("Mapping: {}",if self.reduce {"reduce chroma"}else{"clip"}),2,cx))
                .child(self.button(if self.metadata {format!("Storage: {}",if self.rec {"Rec.2020"}else{"P3"})}else{format!("Simulated gamut: {}",model::NAMES[self.simulated])},3,cx))
                .child(self.button(if self.metadata {format!("Alpha: {}",if self.translucent {0.5}else{1.})}else{format!("Fill Y: {}",model::LUMINANCE[self.luminance])},4,cx)));
        if self.metadata {
            if let Some(s) = &self.sample {
                body=body.child(div().h(px(130.)).flex_shrink_0().bg(Rgba{r:s.preview[0],g:s.preview[1],b:s.preview[2],a:1.}))
                    .child("sRGB preview over white in linear light; source pixels are unchanged by the selector.")
                    .child(format!("Supplied RGB {:?} • source XYZ D65 {:.10?}",model::PATCHES[self.patch].1,s.xyz))
                    .child(format!("Actual stored associated RGBA16F {:.10?} • bits {:04x?}",s.stored,s.bits))
                    .child("ICC modes parse bundled, pinned RGB matrix/TRC profiles with xgpui-color/icc. Curves and colorants come from the bytes, not the profile name. sRGB2014's small neutral offset is retained. No image file is decoded.")
                    .child("These are CPU storage bytes, not a GPU or monitor readback. Browser rendering and native wide-gamut composition are separate checks.");
            }
        } else {
            let mapped = self.cells.iter().filter(|c| c.outside).count();
            body=body.child(self.button(format!("Fill: {}",self.fill),5,cx)).child(self.chart())
                .child(div().flex().flex_wrap().gap_3().children((0..4).map(|i|div().text_color(rgb(model::COLORS[i])).child(model::NAMES[i]))))
                .child(format!("CIE xy • equal axis scale • x 0–0.8, y 0–0.9 • white cross D65, gold cross selected pixel. {mapped}/{} spectral-locus cells need mapping into the simulated gamut at this Y.",self.cells.len()))
                .child("Hatching marks those simulated-gamut cells. Colors are mapped into the selected gamut, then clipped to sRGB for this canvas. A P3-capable display cannot recover colors already clipped by this preview.")
                .child("CIE 1931 2° data: CIE 2019, CC BY-SA 4.0. Spectral locus plus purple boundary; neither outlines nor fill percentages describe display coverage.");
        }
        body.child(self.error.clone().unwrap_or_default())
    }
}
