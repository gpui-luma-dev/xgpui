//! xgpui-curve lab, shared by the native `curve-lab` binary and the browser
//! `?demo=curve` page: mesh lattices, the spine inspector, node editing,
//! plot series, functional curves, and shapes/strokes/fitting.
use gpui::{prelude::*, *};

mod common;
mod func;
mod mesh;
mod nodes;
mod series;
mod shapes;
mod spine;

/// The lab's panels, in tab order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(target_family = "wasm"), derive(clap::ValueEnum))]
pub enum Panel {
    /// Mesh lattice wireframe over a live mesh gradient.
    #[default]
    Mesh,
    /// Curved field spine: projection, seam rule and GPU field.
    Spine,
    /// Node editor with every continuity mode.
    Nodes,
    /// Plot series curve gallery.
    Series,
    /// Tone curves, LUTs and CSS easing.
    Functions,
    /// Squircles, arcs, variable-width strokes and freehand fitting.
    Shapes,
}

impl Panel {
    const ALL: [Panel; 6] = [
        Panel::Mesh,
        Panel::Spine,
        Panel::Nodes,
        Panel::Series,
        Panel::Functions,
        Panel::Shapes,
    ];

    /// Parse a `panel=` query value (the same names as the native `--panel`).
    #[cfg_attr(not(target_family = "wasm"), allow(dead_code))]
    pub fn from_name(name: &str) -> Option<Panel> {
        Some(match name {
            "mesh" => Panel::Mesh,
            "spine" => Panel::Spine,
            "nodes" => Panel::Nodes,
            "series" => Panel::Series,
            "functions" => Panel::Functions,
            "shapes" => Panel::Shapes,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Panel::Mesh => "Mesh lattice",
            Panel::Spine => "Spine inspector",
            Panel::Nodes => "Node editor",
            Panel::Series => "Series curves",
            Panel::Functions => "Functions & easing",
            Panel::Shapes => "Shapes & strokes",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Panel::Mesh => {
                "Drag nodes, or drag a wire to bend both of its tangents. Wires are the exact patch edges the GPU mesh gradient renders."
            }
            Panel::Spine => {
                "Drag the three control points. The field is drawn by the crate's CURVE_SPINE_WGSL on the GPU, or by the f64 CPU code."
            }
            Panel::Nodes => {
                "Drag anchors, handles or the curve itself. Double-click a segment to insert a node; pick a node to change its mode."
            }
            Panel::Series => {
                "Every series curve on the same samples, with a missing sample, area fills and viewport clipping."
            }
            Panel::Functions => {
                "Drag the tone-curve knots. The strip is the baked 256-entry LUT; the right column animates CSS easing functions."
            }
            Panel::Shapes => {
                "Draw with the pointer to fit a tapered stroke. The squircle compares smoothed and circular corners."
            }
        }
    }
}

/// The whole lab.
pub struct CurveLab {
    panel: Panel,
    mesh: mesh::MeshPanel,
    spine: spine::SpinePanel,
    nodes: nodes::NodesPanel,
    series: series::SeriesPanel,
    func: func::FuncPanel,
    shapes: shapes::ShapesPanel,
}

impl CurveLab {
    /// A lab opened on `panel`.
    pub fn new(panel: Panel) -> Self {
        Self {
            panel,
            mesh: mesh::MeshPanel::new(),
            spine: spine::SpinePanel::new(),
            nodes: nodes::NodesPanel::new(),
            series: series::SeriesPanel::default(),
            func: func::FuncPanel::new(),
            shapes: shapes::ShapesPanel::new(),
        }
    }
}

impl Render for CurveLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = self.panel;
        let body = match panel {
            Panel::Mesh => mesh::render(self, window, cx),
            Panel::Spine => spine::render(self, window, cx),
            Panel::Nodes => nodes::render(self, window, cx),
            Panel::Series => series::render(self, window, cx),
            Panel::Functions => func::render(self, window, cx),
            Panel::Shapes => shapes::render(self, window, cx),
        };
        div()
            .id("curve-lab")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(0x0e1422))
            .text_color(rgb(0xe3eaf6))
            .text_sm()
            // The browser build bundles IBM Plex Sans; native uses the system UI font.
            .when(cfg!(target_family = "wasm"), |el| {
                el.font_family("IBM Plex Sans")
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(Panel::ALL.into_iter().map(|choice| {
                        common::button(choice.label(), choice == panel).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.panel = choice;
                                cx.notify();
                            },
                        ))
                    })),
            )
            .child(div().text_color(rgb(0x9fb0cc)).child(panel.about()))
            .child(div().flex_1().min_h_0().child(body))
    }
}
