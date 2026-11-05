use xgpui_ext_lab::{LabApp, LabWindowTitle};
#[cfg(all(target_family = "wasm", target_feature = "atomics"))]
compile_error!("shader examples keep browser GPU handles on a single non-atomic WASM thread");
mod examples;
mod shadows;
mod typography;
use gpui::{prelude::*, *};
use std::borrow::Cow;
const FONT: &[u8] = include_bytes!(
    "../../../../gpui/crates/gpui-web/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
);

#[cfg(target_family = "wasm")]
thread_local! { static APP: std::cell::RefCell<Option<ApplicationHandle>> = const { std::cell::RefCell::new(None) }; }

#[cfg(target_family = "wasm")]
fn main() {
    use gpui_web::{WebBackendPreference, WebPlatform};
    use std::{rc::Rc, sync::Arc};
    console_error_panic_hook::set_once();
    gpui_web::init_logging();
    let search = web_sys::window()
        .unwrap()
        .location()
        .search()
        .unwrap_or_default();
    let params: Vec<_> = search.trim_start_matches('?').split('&').collect();
    let backend = if params.contains(&"backend=webgl") {
        WebBackendPreference::WebGl
    } else if params.contains(&"backend=webgpu") {
        WebBackendPreference::WebGpu
    } else {
        WebBackendPreference::Auto
    };
    let mode = examples::Mode::parse(&search);
    let platform = Rc::new(WebPlatform::new_with_backend(false, backend));
    let http = Arc::new(platform.fetch_http_client());
    let app = Application::with_platform(platform).with_http_client(http);
    app.text_system()
        .add_fonts(vec![Cow::Borrowed(FONT)])
        .unwrap();
    let handle = app.run_embedded(move |cx| open(mode, cx));
    APP.with(|cell| *cell.borrow_mut() = Some(handle));
}
#[cfg(not(target_family = "wasm"))]
fn main() {
    use clap::Parser;

    #[derive(Parser)]
    #[command(about = "Native shader examples and selectable typography")]
    struct Options {
        /// Example to open (defaults to minimal); also selectable in the window.
        #[arg(value_enum, conflicts_with = "example")]
        mode: Option<examples::Mode>,
        /// Select an example using a named option instead of a positional value.
        #[arg(long, value_enum)]
        example: Option<examples::Mode>,
    }

    let options = Options::parse();
    let mode = options
        .example
        .or(options.mode)
        .unwrap_or(examples::Mode::Minimal);
    let app = gpui_platform::application();
    app.text_system()
        .add_fonts(vec![Cow::Borrowed(FONT)])
        .unwrap();
    app.run(move |cx| open(mode, cx));
}
fn open(mode: examples::Mode, cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1120.), px(800.)),
            cx,
        ))),
        ..Default::default()
    };
    if mode == examples::Mode::Typography {
        cx.open_lab_window(options, |window, cx| {
            window.set_lab_title("Selectable Typography · xgpui-shader");
            cx.new(|cx| typography::TypographyDemo::new(window, cx))
        })
        .expect("open typography demo");
        cx.activate(true);
        return;
    }
    cx.open_lab_window(options, |window, cx| {
        window.set_lab_title("Instanced WGSL · Shader Examples");
        cx.new(|_| examples::Examples::new(mode))
    })
    .expect("open shader example");
    cx.activate(true);
}
