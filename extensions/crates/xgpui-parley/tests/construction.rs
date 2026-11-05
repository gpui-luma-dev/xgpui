//! Native host smoke check, on the process main thread for platform initialization.
use gpui::{Application, PlatformTextSystem};
use std::{borrow::Cow, sync::Arc};
use xgpui_parley::ParleyTextSystem;

fn main() -> anyhow::Result<()> {
    let mut fonts = parley::FontContext::new();
    let data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../gpui/crates/gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
    ));
    fonts
        .collection
        .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
    let backend = Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans")?);
    let platform = gpui_platform::current_platform(true);
    // Exercise the real host text backend as well as the injected adapter.
    let native = platform.text_system();
    native.add_fonts(vec![Cow::Borrowed(data)])?;
    check_text_backend(native.as_ref())?;
    check_text_backend(backend.as_ref())?;
    let app = Application::with_platform_and_text_system(platform, backend.clone());
    let descriptor = gpui::font("IBM Plex Sans");
    let expected = backend.font_id(&descriptor)?;
    assert_eq!(app.text_system().resolve_font(&descriptor), expected);
    assert_eq!(app.text_system().units_per_em(expected), 1000);
    // GPUI's public add_fonts path reaches the installed backend.
    app.text_system().add_fonts(vec![Cow::Borrowed(data)])?;
    // Re-resolution need not allocate another ID for an unchanged face.
    assert_eq!(
        app.text_system().resolve_font(&descriptor),
        backend.font_id(&descriptor)?
    );
    assert_eq!(app.text_system().units_per_em(expected), 1000);
    // Exercise axes after injection into a real native platform, without a GPU
    // or display-server requirement. Library tests cover variable glyph masks.
    let flex = include_bytes!("fonts/RobotoFlex.ttf");
    assert!(backend.font_id(&gpui::font("Roboto Flex")).is_err());
    app.text_system().add_fonts(vec![Cow::Borrowed(flex)])?;
    let system = app.text_system();
    let id = system.resolve_font(&gpui::font("Roboto Flex"));
    let gpui::FontQuery::Supported(axes) = system.font_axes(id)? else {
        anyhow::bail!("injected Parley backend must expose font axes");
    };
    assert!(axes.iter().any(|axis| axis.tag == gpui::FontTag::SLANT));
    let window_system = gpui::WindowTextSystem::new(system);
    let text = "Hamburgefonts 012345";
    let mut widths = Vec::new();
    for width in [50.0, 150.0] {
        let mut face = gpui::font("Roboto Flex");
        face.variations = gpui::FontVariations::new([
            (gpui::FontTag::WIDTH, width),
            (gpui::FontTag::SLANT, -7.5),
            (gpui::FontTag::OPTICAL_SIZE, 72.0),
        ])?;
        let line = window_system.layout_line(
            text,
            gpui::px(32.0),
            &[gpui::TextRun {
                len: text.len(),
                font: face,
                ..Default::default()
            }],
            None,
        );
        widths.push(line.width);
    }
    assert!(widths[0] > gpui::px(0.0));
    assert!(widths[1] > widths[0]);
    println!(
        "Native + Parley construction/raster smoke passed on {} (no windows opened)",
        std::env::consts::OS
    );
    Ok(())
}

fn check_text_backend(backend: &dyn PlatformTextSystem) -> anyhow::Result<()> {
    let descriptor = gpui::font("IBM Plex Sans");
    let id = backend.font_id(&descriptor)?;
    assert_eq!(backend.font_id(&descriptor)?, id);
    let text = "office café e\u{301}";
    let line = backend.layout_line(
        text,
        gpui::px(24.0),
        &[gpui::FontRun {
            len: text.len(),
            font_id: id,
        }],
    );
    assert_eq!(line.len, text.len());
    assert!(line.width > gpui::px(0.0));
    for run in &line.runs {
        assert!(backend.font_metrics(run.font_id).units_per_em > 0);
        for glyph in &run.glyphs {
            assert!(text.is_char_boundary(glyph.index));
            let params = gpui::RenderGlyphParams {
                font_id: run.font_id,
                glyph_id: glyph.id,
                font_size: gpui::px(24.0),
                scale_factor: 1.0,
                subpixel_variant: gpui::point(0, 0),
                is_emoji: false,
                subpixel_rendering: false,
                dilation: 0,
            };
            let bounds = backend.glyph_raster_bounds(&params)?;
            // Native Windows/Linux reject raster calls for empty outlines;
            // GPUI normally skips them too (e.g. the space in this fixture).
            if bounds.size.width.0 == 0 || bounds.size.height.0 == 0 {
                continue;
            }
            let (size, coverage) = backend.rasterize_glyph(&params, bounds)?;
            assert_eq!(size, bounds.size);
            assert_eq!(
                coverage.len(),
                size.width.0 as usize * size.height.0 as usize
            );
        }
    }
    Ok(())
}
