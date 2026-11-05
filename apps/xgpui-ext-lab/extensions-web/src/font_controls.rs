//! The native axis inspector, running through Parley and GPUI in Wasm.
use crate::{axes, samples};
use anyhow::{Context as _, Result};
use gpui::{prelude::*, *};

pub struct Prepared {
    controls: axes::Controls,
    ranges: String,
    features: String,
    samples: Vec<samples::Sample>,
}

impl Prepared {
    pub fn new(backend: &dyn PlatformTextSystem) -> Result<Self> {
        let id = backend.font_id(&font("Roboto Flex"))?;
        let FontQuery::Supported(axes) = backend.font_axes(id)? else {
            anyhow::bail!("Parley axis inspection unavailable");
        };
        let FontQuery::Supported(features) = backend.opentype_features(id)? else {
            anyhow::bail!("Parley feature inspection unavailable");
        };
        Ok(Self {
            controls: axes::Controls::for_backend(backend)?.context("Parley axes unavailable")?,
            ranges: axes
                .iter()
                .map(|axis| {
                    format!(
                        "{}: {} .. {} (default {}){}",
                        axis.tag,
                        axis.min_value,
                        axis.max_value,
                        axis.default_value,
                        if axis.hidden { " · hidden" } else { "" },
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            features: features
                .iter()
                .map(|feature| {
                    format!(
                        "{} [{}{}]",
                        feature.tag,
                        if feature.substitution { "GSUB" } else { "" },
                        if feature.positioning {
                            if feature.substitution {
                                "+GPOS"
                            } else {
                                "GPOS"
                            }
                        } else {
                            ""
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join(" · "),
            samples: samples::axis_samples()?,
        })
    }
}

pub struct FontDemo {
    axes: Entity<axes::AxisLab>,
    ranges: String,
    features: String,
    samples: Vec<samples::Sample>,
}

impl FontDemo {
    pub fn new(prepared: Prepared, cx: &mut Context<Self>) -> Self {
        Self {
            axes: cx.new(|cx| axes::AxisLab::new("Parley / Wasm".into(), prepared.controls, cx)),
            ranges: prepared.ranges,
            features: prepared.features,
            samples: prepared.samples,
        }
    }
}

impl Render for FontDemo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("font-controls-demo")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(0x0f172a))
            .text_color(rgb(0xe5e7eb))
            .font_family("IBM Plex Sans")
            .text_size(px(14.0))
            .child(div().w_full().max_w(px(1060.0)).mx_auto().p_4().flex().flex_col().gap_3()
                .child(div().text_size(px(25.0)).child("Variable fonts · Rust / Wasm"))
                .child("Drag weight, width, slant and optical size. These are the desktop text lab’s controls, shaped and rasterized by Parley in Wasm; CSS font rendering is not involved.")
                .child(self.axes.clone())
                .child(div().text_size(px(20.0)).child("Resolved Roboto Flex axis ranges"))
                .child(self.ranges.clone())
                .child(div().text_size(px(20.0)).child("OpenType feature tags"))
                .child(self.features.clone())
                .child("Tags describe this face’s GSUB/GPOS tables, not guaranteed effects on every string. Static samples below retain precise coordinates independently of the preview sliders.")
                .children(self.samples.iter().map(|sample| div().flex().flex_col().gap_1().p_3().rounded_md().bg(rgb(0x1f2937))
                    .child(sample.label.clone())
                    .child(div().text_size(px(28.0)).font(sample.font.clone()).child(sample.text.clone()))))
                .child("Preview policy: 512 combinations per session; exhaustion holds the last font. Previously admitted settings and Default remain available. Reload for a fresh session. This is not global memory budgeting or eviction.")
                .child("Text engine: Parley / HarfRust / Skrifa in Wasm. WebGPU / WebGL2 selects only the GPU renderer. Compare native CoreText / DirectWrite with Parley using the desktop text lab."))
    }
}
