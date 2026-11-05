//! Color swatches and the integrated CIE chromaticity workspace.
#![recursion_limit = "256"]
use xgpui_ext_lab::{LabApp, LabWindowTitle};
mod chromaticity;
mod diagnostics;
use clap::{Parser, ValueEnum};
#[path = "../../extensions-web/src/color_samples.rs"]
mod color_samples;
use gpui::{prelude::*, *};
use xgpui_color::{EncodedRgb, GamutMapping, RgbSpace};
use xgpui_gamut::{WideColor, instanced::WidePainter};

struct ColorLab {
    preset: usize,
    policy: GamutMapping,
    translucent: bool,
    painter: Option<WidePainter>,
}
impl Render for ColorLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = window.color_status();
        let wide = status.effective() == WindowColorMode::WideGamutSdr && self.painter.is_some();
        let srgb = color_samples::rows(self.preset, self.policy, self.translucent).unwrap();
        let p3 = color_samples::rows_in_space(
            self.preset,
            self.policy,
            self.translucent,
            RgbSpace::DisplayP3,
        )
        .unwrap();
        div().id("native-color").size_full().overflow_y_scroll().p_6().flex().flex_col().gap_4().bg(rgb(0x0f172a)).text_color(rgb(0xe2e8f0))
            .child(div().text_xl().child("Color swatches · sRGB / optional native P3"))
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
                    .when(wide, |row| row.child("Native Display P3")
                    .child(div().flex().w_full().h(px(46.)).children(p3.rgba.into_iter().map(|[r,g,b,a]| {
                        let rgb=EncodedRgb::new(RgbSpace::DisplayP3,[r as f64,g as f64,b as f64]).unwrap().decode().unwrap().components();
                        let color=WideColor::linear_p3([rgb[0] as f32,rgb[1] as f32,rgb[2] as f32,a]).unwrap();
                        let painter=self.painter.clone().unwrap();
                        div().flex_1().h_full().child(canvas(|_,_,_|(),move |bounds,_,window,_| {painter.paint_color(window,bounds,px(0.),&color).expect("native P3 swatch");}).size_full())
                    }))))
            }))
            .child("32 discrete reference samples per row, shared with the Wasm demo. P3 samples map to P3; sRGB samples map to sRGB. Alpha composites over the same background. Native P3 requires effective WideGamutSdr; display reporting does not verify calibration.")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
enum Mode {
    #[default]
    Swatches,
    Chromaticity,
}

#[derive(Clone, Debug, Default, Parser)]
#[command(
    about = "Color lab: swatches, CIE xy/u′v′, FP16/ICC inspection and native-wide comparisons"
)]
struct Options {
    /// Initial workspace; both are available from tabs in the window.
    #[arg(long, value_enum)]
    mode: Option<Mode>,
    /// Start in the chromaticity workspace.
    #[arg(long, conflicts_with = "mode")]
    chromaticity: bool,
    /// Start the chromaticity diagram in CIE u′v′ instead of xy.
    #[arg(long)]
    uv: bool,
    /// Start with native-wide chromaticity fill (falls back to sRGB if unavailable).
    #[arg(long)]
    native_fill: bool,
    /// Start the chromaticity workspace with the WebKit Italy photo comparison.
    #[arg(long)]
    photo: bool,
    /// Select the synthetic ICC parametric fixture.
    #[arg(long, conflicts_with = "icc_sampled")]
    icc_parametric: bool,
    /// Select the synthetic ICC sampled fixture.
    #[arg(long)]
    icc_sampled: bool,
    /// Enable nested wide captures/blur in chromaticity (macOS Metal only).
    #[arg(long)]
    effects: bool,
}
impl Options {
    fn initial_mode(&self) -> anyhow::Result<Mode> {
        let chromaticity_options = self.uv
            || self.native_fill
            || self.photo
            || self.icc_parametric
            || self.icc_sampled
            || self.effects;
        anyhow::ensure!(
            self.mode != Some(Mode::Swatches) || !chromaticity_options,
            "Chromaticity options cannot be used with --mode swatches"
        );
        anyhow::ensure!(
            !self.effects || cfg!(target_os = "macos"),
            "--effects requires macOS Metal; omit it for the Windows/Linux color workspace"
        );
        Ok(self
            .mode
            .unwrap_or(if self.chromaticity || chromaticity_options {
                Mode::Chromaticity
            } else {
                Mode::Swatches
            }))
    }
}

struct Workspace {
    mode: Mode,
    notice: Option<String>,
    options: Options,
    swatches: Option<Entity<ColorLab>>,
    chromaticity: Option<Entity<chromaticity::Lab>>,
}
impl Workspace {
    fn swatch_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.swatches.is_none() {
            let painter = if self.notice.is_none() { WidePainter::new().ok() } else { None };
            if painter.is_none() && self.notice.is_none() {
                self.notice = Some("Native P3 output isn’t available with the current display and graphics setup. Showing mapped sRGB swatches.".into());
            }
            self.swatches = Some(cx.new(|_| ColorLab {
                preset: 1, policy: GamutMapping::Clip, translucent: false, painter,
            }));
        }
        self.swatches.as_ref().unwrap().clone().into_any_element()
    }
}
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.mode {
            Mode::Swatches => self.swatch_view(cx),
            Mode::Chromaticity => {
                if self.chromaticity.is_none() {
                    self.chromaticity =
                        Some(cx.new(|_| chromaticity::Lab::new(window, &self.options)));
                }
                self.chromaticity
                    .as_ref()
                    .unwrap()
                    .clone()
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0f172a))
            .text_color(rgb(0xe2e8f0))
            .child(
                div().flex_none().flex().gap_3().p_3().children(
                    [
                        (Mode::Swatches, "Swatches"),
                        (Mode::Chromaticity, "Chromaticity"),
                    ]
                    .into_iter()
                    .map(|(mode, label)| {
                        div()
                            .id(label)
                            .cursor_pointer()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(if self.mode == mode {
                                0x334155
                            } else {
                                0x1e293b
                            }))
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.mode = mode;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .children(self.notice.iter().cloned().map(|notice| div().px_3().py_2().text_color(rgb(0xfbbf24)).child(notice)))
            .child(div().flex_1().min_h_0().overflow_hidden().child(content))
            .into_any_element()
    }
}

fn main() -> anyhow::Result<()> {
    let options = Options::parse();
    let mode = options.initial_mode()?;
    diagnostics::init();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1220.), px(960.)), cx);
        cx.open_lab_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let notice = xgpui_ext_lab::negotiate_wide(window);
                if let Some(notice) = &notice { eprintln!("{notice}"); }
                window.set_lab_title("XGPUI Color Lab — Swatches & Chromaticity");
                cx.new(|_| Workspace {
                    mode,
                    notice,
                    options,
                    swatches: None,
                    chromaticity: None,
                })
            },
        )
        .expect("open color lab");
        cx.activate(true);
    });
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn existing_swatches_remain_default_and_chromaticity_flags_select_the_workspace() {
        assert_eq!(
            Options::parse_from(["color-lab"]).initial_mode().unwrap(),
            Mode::Swatches
        );
        for flag in [
            "--chromaticity",
            "--uv",
            "--native-fill",
            "--photo",
            "--icc-parametric",
            "--icc-sampled",
        ] {
            assert_eq!(
                Options::parse_from(["color-lab", flag])
                    .initial_mode()
                    .unwrap(),
                Mode::Chromaticity
            );
        }
        assert_eq!(
            Options::parse_from(["color-lab", "--mode", "chromaticity"])
                .initial_mode()
                .unwrap(),
            Mode::Chromaticity
        );
    }
    #[::core::prelude::v1::test]
    fn incompatible_launch_modes_fail_before_opening_a_window() {
        assert!(
            Options::try_parse_from(["color-lab", "--icc-parametric", "--icc-sampled"]).is_err()
        );
        assert!(
            Options::parse_from(["color-lab", "--mode", "swatches", "--uv"])
                .initial_mode()
                .is_err()
        );
    }
}
