#[cfg(target_feature = "atomics")]
compile_error!(
    "extensions-web must be built without atomics; browser GPU handles stay on one thread"
);

use gpui::{prelude::*, *};
use gpui_web::{WebBackendPreference, WebPlatform};
use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc};

// Share the native lab's editor, including its UTF-16/IME bridge and BiDi geometry.
mod brushes;
mod color;
mod color_samples;
mod hermite;
mod media;
#[path = "../../text-lab/src/paragraph.rs"]
mod paragraph;
mod shader;
mod svg;
#[path = "../../web-support/tiled_demo.rs"]
mod tiled_demo;
#[path = "../../shader-examples/src/typography.rs"]
mod typography;

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
        "media-tiles" => "72 MP tiled canvas: progressive preview • bounded immutable pages • viewport selection",
        "media-float" => "RGBA16F: supplied linear P3 pixels • explicit clipped sRGB preview",
        "media-filters" => {
            "Media adjustments: linear exposure and vignette • encoded filter comparison • shared source"
        }
        "mipmaps" => "Mipmapping: native atlas versus dedicated trilinear source texture",
        "path-effects" => "Path effects: contour trim and dash before native tessellation",
        "typography" => {
            "Typography: gradient, glow and shadow • kinetic glyphs with an unchanged source"
        }
        "svg" => "SVG: independent views • sharp / texture zoom • no worker",
        "parley" => "Parley: editable paragraphs • selection and BiDi",
        "brushes" => "Brushes: universal fills, noise, glyphs and paths",
        "hermite" => "Hermite: live 2×2 / 3×3 mesh editor • points and tangent vectors",
        "layers" => "Layers: subtree filters, blending and blur",
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
    let typography = parameters.contains(&"demo=typography");
    let parley = parameters.contains(&"demo=parley");
    let svg = parameters.contains(&"demo=svg");
    let color = parameters.contains(&"demo=color");
    let brushes = parameters.contains(&"demo=brushes");
    let layers = parameters.contains(&"demo=layers");
    let tiled = parameters.contains(&"demo=media-tiles");
    let hermite = parameters.contains(&"demo=hermite");
    let media_mode = if parameters.contains(&"demo=media-float") {
        Some(media::Mode::FloatImages)
    } else if parameters.contains(&"demo=media-filters") {
        Some(media::Mode::Filters)
    } else if parameters.contains(&"demo=mipmaps") {
        Some(media::Mode::Mipmaps)
    } else if parameters.contains(&"demo=path-effects") {
        Some(media::Mode::Paths)
    } else {
        None
    };
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

    let reduced_motion = web_sys::window()
        .and_then(|window| {
            window
                .match_media("(prefers-reduced-motion: reduce)")
                .ok()
                .flatten()
        })
        .is_some_and(|query| query.matches());
    let handle = app.run_embedded(move |cx| {
        cx.set_reduce_motion(reduced_motion);
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
        } else if tiled {
            cx.open_window(options, |_, cx| cx.new(|_| tiled_demo::TiledDemo::new()))
                .expect("open tiled canvas");
        } else if let Some(mode) = media_mode {
            cx.open_window(options, |_, cx| {
                cx.new(|_| media::MediaDemo::new(mode).expect("generated media fixture"))
            })
            .expect("open media demo");
        } else if typography {
            cx.open_window(options, |_, cx| cx.new(typography::TypographyDemo::new))
                .expect("open typography demo");
        } else if hermite {
            cx.open_window(options, |_, cx| cx.new(|_| hermite::HermiteDemo::new()))
                .expect("open Hermite editor");
        } else if brushes || layers {
            cx.open_window(options, |_, cx| cx.new(|_| brushes::BrushDemo::new(layers)))
                .expect("open brush gallery");
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
