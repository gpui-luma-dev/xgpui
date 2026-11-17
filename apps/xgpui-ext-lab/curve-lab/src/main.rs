//! xgpui-curve lab: mesh lattices, the spine inspector, node editing, plot
//! series, functional curves and shapes/strokes. The panels are shared with
//! the browser demo (`?demo=curve`).
#![recursion_limit = "256"]
use clap::Parser;
use gpui::{prelude::*, *};
use xgpui_ext_lab::{LabApp, LabWindowTitle};

#[path = "../../extensions-web/src/curve/mod.rs"]
mod curve;

#[derive(Debug, Parser)]
#[command(about = "xgpui-curve lab: mesh lattices, spines, nodes, series, functions and shapes")]
struct Options {
    /// Panel to open first; all panels are available from tabs in the window.
    #[arg(long, value_enum, default_value = "mesh")]
    panel: curve::Panel,
}

fn main() -> anyhow::Result<()> {
    let options = Options::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1280.), px(900.)), cx);
        cx.open_lab_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_lab_title("XGPUI Curve Lab");
                cx.new(|_| curve::CurveLab::new(options.panel))
            },
        )
        .expect("open curve lab");
        cx.activate(true);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn every_panel_is_selectable_from_the_command_line() {
        for name in ["mesh", "spine", "nodes", "series", "functions", "shapes"] {
            assert!(
                Options::try_parse_from(["curve-lab", "--panel", name]).is_ok(),
                "{name}"
            );
        }
        assert!(Options::try_parse_from(["curve-lab", "--panel", "nope"]).is_err());
    }
}
