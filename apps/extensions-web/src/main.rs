#[cfg(target_feature = "atomics")]
compile_error!(
    "extensions-web must be built without atomics; browser GPU handles stay on one thread"
);

use gpui::{prelude::*, *};
use gpui_web::{WebBackendPreference, WebPlatform};
use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc};

// Share the native lab's editor, including its UTF-16/IME bridge and BiDi geometry.
mod color;
mod color_samples;
#[path = "../../text-lab/src/paragraph.rs"]
mod paragraph;
mod shader;
mod svg;

const PLEX: &[u8] = include_bytes!(
    "../../../gpui/crates/gpui-web/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
);

thread_local! {
    static APPLICATION: RefCell<Option<ApplicationHandle>> = const { RefCell::new(None) };
}

/// Identifies the compiled module independently of its HTML loader.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn demo_build_id() -> String {
    option_env!("XGPUI_WEB_BUILD_ID")
        .unwrap_or("unversioned")
        .into()
}

/// Keep the visible behavior label tied to the running module.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn demo_behavior(demo: &str) -> String {
    match demo {
        "svg" => "SVG: independent views • sharp / texture zoom • no worker",
        "parley" => "Parley: editable paragraphs • selection and BiDi",
        "shader" => "Shader: gradients, paths and animation",
        "color" => "Color / Gamut: interpolation and mapping • sRGB preview",
        _ => "Unknown demo",
    }
    .into()
}

fn main() {
    console_error_panic_hook::set_once();
    gpui_web::init_logging();
    if let Err(error) = launch() {
        web_sys::console::error_1(&format!("Extension demo: {error:#}").into());
        panic!("Extension demo: {error:#}");
    }
}

fn launch() -> anyhow::Result<()> {
    let search = web_sys::window()
        .unwrap()
        .location()
        .search()
        .map_err(|error| anyhow::anyhow!("read URL: {error:?}"))?;
    let parameters: Vec<_> = search.trim_start_matches('?').split('&').collect();
    let backend = if parameters.contains(&"backend=webgpu") {
        WebBackendPreference::WebGpu
    } else if parameters.contains(&"backend=webgl") {
        WebBackendPreference::WebGl
    } else {
        WebBackendPreference::Auto
    };
    let parley = parameters.contains(&"demo=parley");
    let svg = parameters.contains(&"demo=svg");
    let color = parameters.contains(&"demo=color");
    // These demos need no worker pool or shared Wasm memory.
    let platform = Rc::new(WebPlatform::new_with_backend(false, backend));
    let http = Arc::new(platform.fetch_http_client());
    let app = if parley {
        let mut fonts = parley::FontContext::new();
        for data in [PLEX, paragraph::FONTS[0], paragraph::FONTS[1]] {
            fonts
                .collection
                .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
        }
        Application::with_platform_and_text_system(
            platform,
            Arc::new(xgpui_parley::ParleyTextSystem::new(fonts, "IBM Plex Sans")?),
        )
    } else {
        let app = Application::with_platform(platform);
        app.text_system().add_fonts(vec![Cow::Borrowed(PLEX)])?;
        app
    }
    .with_http_client(http);

    let handle = app.run_embedded(move |cx| {
        let options = WindowOptions::default();
        if parley {
            let timings = paragraph::measure(cx.text_system().clone(), 5)
                .expect("paragraph layout diagnostics");
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| paragraph::ParagraphLab::new(timings, cx));
                window.focus(&view.focus_handle(cx), cx);
                view
            })
            .expect("open Parley demo");
        } else if color {
            cx.open_window(options, |_, cx| cx.new(|_| color::ColorDemo::new()))
                .expect("open color demo");
        } else if svg {
            cx.open_window(options, |_, cx| {
                cx.new(|cx| svg::SvgDemo::new(cx).expect("bundled SVG samples"))
            })
            .expect("open SVG demo");
        } else {
            cx.open_window(options, |_, cx| cx.new(|_| shader::ShaderDemo::new()))
                .expect("open shader demo");
        }
        cx.activate(true);
    });
    APPLICATION.with(|cell| *cell.borrow_mut() = Some(handle));
    Ok(())
}
