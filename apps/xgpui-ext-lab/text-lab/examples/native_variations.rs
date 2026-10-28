//! Isolated macOS investigation; does not change GPUI's native backend.
#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    use core_foundation::{
        attributed_string::CFMutableAttributedString,
        base::{CFRange, TCFType},
        dictionary::CFDictionary,
        number::CFNumber,
        string::CFString,
    };
    use core_text::{font::CTFont, font_descriptor, line::CTLine, string_attributes};
    use gpui::{
        FontFallbacks, FontFeatures, FontRun, FontWeight, RenderGlyphParams, font, point, px,
    };
    use std::borrow::Cow;

    const DATA: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../extensions/xgpui-ext/crates/xgpui-parley/tests/fonts/InterVariable.ttf"
    ));
    const TEXT: &str = "Hamburgefonts 012345";
    fn width(font: &CTFont) -> f64 {
        let mut text = CFMutableAttributedString::new();
        text.replace_str(&CFString::new(TEXT), CFRange::init(0, 0));
        unsafe {
            text.set_attribute(
                CFRange::init(0, text.char_len()),
                string_attributes::kCTFontAttributeName,
                font,
            );
        }
        CTLine::new_with_attributed_string(text.as_concrete_TypeRef())
            .get_typographic_bounds()
            .width
    }

    let platform = gpui_platform::current_platform(true);
    let native = platform.text_system();
    native.add_fonts(vec![Cow::Borrowed(DATA)])?;
    let base = core_text::font::new_from_buffer(DATA).map_err(|_| anyhow::anyhow!("load Inter"))?;
    let mut widths = Vec::new();
    let mut native_ids = Vec::new();
    let mut native_widths = Vec::new();
    let mut masks = Vec::new();
    for weight in [100, 400, 900] {
        let mut request = font("Inter Variable");
        request.weight = FontWeight(weight as f32);
        let id = native.font_id(&request)?;
        let layout = native.layout_line(
            TEXT,
            px(28.0),
            &[FontRun {
                len: TEXT.len(),
                font_id: id,
            }],
        );

        anyhow::ensure!(native.font_id(&request)? == id, "unstable requested ID");
        anyhow::ensure!(
            layout.runs.iter().all(|run| run.font_id == id),
            "shaping lost instance identity"
        );
        native_ids.push(id);
        native_widths.push(f32::from(layout.width));
        let params = RenderGlyphParams {
            font_id: id,
            glyph_id: native.glyph_for_char(id, 'H').unwrap(),
            font_size: px(28.0),
            scale_factor: 1.0,
            subpixel_variant: point(0, 0),
            is_emoji: false,
            subpixel_rendering: false,
            dilation: 0,
        };
        masks.push(native.rasterize_glyph(&params, native.glyph_raster_bounds(&params)?)?);
        // OpenType axis tags are numeric keys; values are design coordinates.
        // Pin optical size to Parley's default location for comparison only.
        let axes = CFDictionary::from_CFType_pairs(&[
            (
                CFNumber::from(i32::from_be_bytes(*b"wght")),
                CFNumber::from(weight),
            ),
            (
                CFNumber::from(i32::from_be_bytes(*b"opsz")),
                CFNumber::from(14),
            ),
        ]);
        let key =
            unsafe { CFString::wrap_under_get_rule(font_descriptor::kCTFontVariationAttribute) };
        let attrs = CFDictionary::from_CFType_pairs(&[(key, axes)]);
        let descriptor = base
            .copy_descriptor()
            .create_copy_with_attributes(attrs.to_untyped())
            .unwrap();
        let instance = core_text::font::new_from_descriptor(&descriptor, 28.0);
        let resized = instance.clone_with_font_size(28.0);
        let roundtrip = core_text::font::new_from_CGFont(&instance.copy_to_CGFont(), 28.0);
        anyhow::ensure!(
            (width(&instance) - width(&resized)).abs() < 0.001,
            "resizing must preserve the selected instance"
        );
        widths.push(width(&instance));
        println!(
            "wght={weight}: GPUI requested={id:?} shaped={:?} width={:.2}; CoreText={:.2} resized={:.2} CG roundtrip={:.2} PS={}",
            layout
                .runs
                .iter()
                .map(|run| run.font_id)
                .collect::<Vec<_>>(),
            f32::from(layout.width),
            width(&instance),
            width(&resized),
            width(&roundtrip),
            instance.postscript_name()
        );
    }
    anyhow::ensure!(
        widths
            .windows(2)
            .all(|pair| (pair[0] - pair[1]).abs() > 1.0),
        "explicit weights must change this fixture's advances"
    );
    anyhow::ensure!(
        native_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == 3,
        "native weights must have distinct IDs"
    );
    anyhow::ensure!(
        native_widths.windows(2).all(|w| w[0] < w[1]),
        "native weights must change advances"
    );
    anyhow::ensure!(
        masks.windows(2).all(|m| m[0] != m[1]),
        "native weights must change raster masks"
    );
    let mut configured_ids = Vec::new();
    for (fallbacks, features) in [(true, false), (false, true), (true, true)] {
        let mut configured = font("Inter Variable");
        configured.weight = FontWeight(900.0);
        if fallbacks {
            configured.fallbacks = Some(FontFallbacks::from_fonts(vec!["Times".into()]));
        }
        if features {
            configured.features = FontFeatures::disable_ligatures();
        }
        let id = native.font_id(&configured)?;
        anyhow::ensure!(
            !native_ids.contains(&id) && !configured_ids.contains(&id),
            "fallback/features identity collapsed"
        );
        configured_ids.push(id);
        for size in [16.0, 28.0, 40.0, 28.0] {
            let line = native.layout_line(
                TEXT,
                px(size),
                &[FontRun {
                    len: TEXT.len(),
                    font_id: id,
                }],
            );
            anyhow::ensure!(
                line.runs.iter().all(|r| r.font_id == id),
                "configured instance lost in shaping"
            );
        }
        anyhow::ensure!(native.font_id(&configured)? == id, "configured ID changed");
    }
    for (weight, expected) in [(0.0, native_ids[0]), (1000.0, native_ids[2])] {
        let mut request = font("Inter Variable");
        request.weight = FontWeight(weight);
        anyhow::ensure!(
            native.font_id(&request)? == expected,
            "weight axis was not clamped"
        );
    }
    println!("PASS: distinct weights, raster masks, stable IDs and feature/fallback identity");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This probe uses macOS CoreText.");
}
