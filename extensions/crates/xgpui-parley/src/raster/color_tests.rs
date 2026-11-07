use super::*;
use crate::ParleyTextSystem;
use gpui::{FontRun, PlatformParagraphLayout as _, PlatformTextSystem as _, RenderGlyphParams, px};
use std::sync::Arc;

/// The platform's color emoji font, when present: sbix on macOS, COLR on
/// Windows, CBDT on most Linux distributions.
fn system_emoji_font() -> Option<(Vec<u8>, &'static str)> {
    [
        (
            "/System/Library/Fonts/Apple Color Emoji.ttc",
            "Apple Color Emoji",
        ),
        (r"C:\Windows\Fonts\seguiemj.ttf", "Segoe UI Emoji"),
        (
            "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
            "Noto Color Emoji",
        ),
        (
            "/usr/share/fonts/noto/NotoColorEmoji.ttf",
            "Noto Color Emoji",
        ),
        (
            "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
            "Noto Color Emoji",
        ),
    ]
    .into_iter()
    .find_map(|(path, family)| Some((std::fs::read(path).ok()?, family)))
}

fn colorful(raster: &ColorRaster) -> bool {
    raster.bgra.chunks_exact(4).any(|p| {
        let (b, g, r, a) = (p[0] as i32, p[1] as i32, p[2] as i32, p[3]);
        a > 200 && r.max(g).max(b) - r.min(g).min(b) > 60
    })
}

#[test]
fn system_emoji_font_renders_a_color_glyph_at_its_size() -> Result<()> {
    let Some((bytes, family)) = system_emoji_font() else {
        eprintln!("no system color emoji font; skipping");
        return Ok(());
    };
    let font = FontRef::from_index(&bytes, 0)?;
    let glyph = font.charmap().map('😀').context("emoji glyph")?.to_u32();
    let probe = ColorProbe::new(&bytes, 0).context("color tables")?;
    assert!(probe.has(glyph), "{family} has color data for 😀");
    for ppem in [16.0, 32.0, 64.0] {
        let raster = render(&font, &[], glyph, ppem)?.context("color raster")?;
        let (w, h) = (raster.width as f32, raster.height as f32);
        assert!(
            w > ppem * 0.6 && w < ppem * 1.6,
            "{family} {ppem}: width {w}"
        );
        assert!(
            h > ppem * 0.6 && h < ppem * 1.6,
            "{family} {ppem}: height {h}"
        );
        assert!(raster.top < 0, "rises above the baseline");
        assert!(
            raster.top + (h as i32) > -(ppem as i32) / 2,
            "reaches near the baseline"
        );
        assert_eq!(
            raster.bgra.len(),
            (raster.width * raster.height * 4) as usize
        );
        assert!(
            colorful(&raster),
            "{family} {ppem}: has color, not just gray"
        );
    }
    Ok(())
}

#[test]
fn line_and_paragraph_paths_flag_and_rasterize_emoji() -> Result<()> {
    let Some((bytes, family)) = system_emoji_font() else {
        eprintln!("no system color emoji font; skipping");
        return Ok(());
    };
    let mut fonts = crate::test_support::fonts();
    fonts
        .collection
        .register_fonts(parley::fontique::Blob::new(Arc::new(bytes)), None);
    let backend = ParleyTextSystem::new(fonts, "IBM Plex Sans")?;
    let mut font = gpui::font("IBM Plex Sans");
    font.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![family.into()]));
    let font_id = backend.font_id(&font)?;
    let text = "a😀";
    let runs = [FontRun {
        len: text.len(),
        font_id,
    }];

    // Plain GPUI text (single-line path): only the emoji glyph is flagged.
    let line = backend.layout_line(text, px(24.0), &runs);
    let glyphs: Vec<_> = line
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(move |glyph| (run.font_id, glyph)))
        .collect();
    let (emoji_font, emoji) = glyphs
        .iter()
        .find(|(_, glyph)| glyph.index == 1)
        .copied()
        .context("emoji glyph")?;
    assert!(emoji.is_emoji, "{family} glyph is painted as color");
    assert!(
        glyphs
            .iter()
            .all(|(_, glyph)| glyph.index != 0 || !glyph.is_emoji)
    );

    // GPUI's emoji request produces BGRA pixels matching the measured bounds.
    let params = RenderGlyphParams {
        font_id: emoji_font,
        glyph_id: emoji.id,
        font_size: px(24.0),
        scale_factor: 2.0,
        subpixel_variant: Default::default(),
        is_emoji: true,
        subpixel_rendering: false,
        dilation: 0,
    };
    let bounds = backend.glyph_raster_bounds(&params)?;
    let (extent, pixels) = backend.rasterize_glyph(&params, bounds)?;
    assert_eq!(extent, bounds.size);
    assert_eq!(
        pixels.len(),
        extent.width.0 as usize * extent.height.0 as usize * 4
    );
    assert!(
        extent.width.0 > 30 && extent.height.0 > 30,
        "48 device px per em"
    );

    // Paragraphs (and typeset()) flag the same glyph.
    let paragraph = backend
        .prepare_paragraph(
            "😀 ok",
            px(24.0),
            &[FontRun {
                len: "😀 ok".len(),
                font_id,
            }],
        )?
        .layout(px(400.0))?;
    let flagged: Vec<_> = paragraph
        .runs()
        .iter()
        .flat_map(|run| &run.glyphs)
        .map(|glyph| (glyph.index, glyph.is_emoji))
        .collect();
    assert!(flagged.contains(&(0, true)), "{flagged:?}");
    assert!(flagged.iter().all(|&(index, emoji)| index == 0 || !emoji));
    Ok(())
}

#[test]
fn registered_emoji_fonts_are_automatic_fallbacks() -> Result<()> {
    let Some((bytes, family)) = system_emoji_font() else {
        eprintln!("no system color emoji font; skipping");
        return Ok(());
    };
    let emoji_flagged = |backend: &ParleyTextSystem| -> Result<bool> {
        // No fallback list: the emoji family is never named.
        let font_id = backend.font_id(&gpui::font("IBM Plex Sans"))?;
        let line = backend.layout_line(
            "a😀",
            px(24.0),
            &[FontRun {
                len: "a😀".len(),
                font_id,
            }],
        );
        Ok(line
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .any(|glyph| glyph.index == 1 && glyph.is_emoji && glyph.id.0 != 0))
    };
    // Registered before construction.
    let mut fonts = crate::test_support::fonts();
    fonts
        .collection
        .register_fonts(parley::fontique::Blob::new(Arc::new(bytes.clone())), None);
    assert!(
        emoji_flagged(&ParleyTextSystem::new(fonts, "IBM Plex Sans")?)?,
        "{family}"
    );
    // Added later through GPUI's add_fonts.
    let backend = ParleyTextSystem::new(crate::test_support::fonts(), "IBM Plex Sans")?;
    assert!(!emoji_flagged(&backend)?, "no emoji font registered yet");
    backend.add_fonts(vec![std::borrow::Cow::Owned(bytes)])?;
    assert!(emoji_flagged(&backend)?, "{family} after add_fonts");
    Ok(())
}

mod synthetic {
    //! COLR fonts built from Inter's outlines, so every platform exercises the
    //! COLR painter without bundling an emoji font.
    use super::*;
    use write_fonts::{
        FontBuilder,
        tables::{
            colr::{
                BaseGlyph, BaseGlyphList, BaseGlyphPaint, ColorLine, ColorStop as Stop, Colr,
                Extend as LineExtend, Layer, Paint, PaintGlyph, PaintLinearGradient,
            },
            cpal::{ColorRecord, Cpal},
        },
        types::{F2Dot14, FWord, GlyphId16},
    };

    const INTER: &[u8] = crate::test_support::INTER;
    const RED: usize = 0;
    const BLUE: usize = 1;

    fn glyph(c: char) -> GlyphId16 {
        let font = FontRef::new(INTER).unwrap();
        GlyphId16::new(font.charmap().map(c).unwrap().to_u32() as u16)
    }

    fn with_colr(colr: Colr) -> Vec<u8> {
        let palette = Cpal::new(
            2,
            1,
            2,
            Some(vec![
                ColorRecord::new(0, 0, 255, 255), // red (BGRA)
                ColorRecord::new(255, 0, 0, 255), // blue
            ]),
            vec![0],
        );
        FontBuilder::new()
            .add_table(&colr)
            .unwrap()
            .add_table(&palette)
            .unwrap()
            .copy_missing_tables(write_fonts::read::FontRef::new(INTER).unwrap())
            .build()
    }

    /// Fraction of opaque pixels that are mostly red and mostly blue.
    fn red_and_blue(raster: &ColorRaster) -> (usize, usize) {
        raster.bgra.chunks_exact(4).fold((0, 0), |(red, blue), p| {
            let (b, r, a) = (p[0], p[2], p[3]);
            (
                red + (a > 200 && r > 200 && b < 60) as usize,
                blue + (a > 200 && b > 200 && r < 60) as usize,
            )
        })
    }

    #[test]
    fn colr_v0_layers_paint_in_palette_colors() -> Result<()> {
        let (h, o, i) = (glyph('H'), glyph('O'), glyph('I'));
        let bytes = with_colr(Colr::new(
            1,
            Some(vec![BaseGlyph::new(h, 0, 2)]),
            Some(vec![Layer::new(o, RED as u16), Layer::new(i, BLUE as u16)]),
            2,
        ));
        let probe = ColorProbe::new(&bytes, 0).context("COLR detected")?;
        assert!(probe.has(h.to_u32()));
        assert!(
            !probe.has(glyph('A').to_u32()),
            "plain glyphs stay outlines"
        );

        let font = FontRef::new(&bytes)?;
        let raster = render(&font, &[], h.to_u32(), 48.0)?.context("color raster")?;
        let (red, blue) = red_and_blue(&raster);
        assert!(red > 50, "red O layer: {red}");
        assert!(blue > 20, "blue I layer on top: {blue}");
        assert!(
            raster.top < 0 && raster.top > -48,
            "above the baseline: {}",
            raster.top
        );
        assert!(render(&font, &[], glyph('A').to_u32(), 48.0)?.is_none());
        Ok(())
    }

    #[test]
    fn colr_v1_linear_gradient_runs_across_the_glyph() -> Result<()> {
        let o = glyph('O');
        let line = ColorLine::new(
            LineExtend::Pad,
            2,
            vec![
                Stop::new(F2Dot14::from_f32(0.0), RED as u16, F2Dot14::from_f32(1.0)),
                Stop::new(F2Dot14::from_f32(1.0), BLUE as u16, F2Dot14::from_f32(1.0)),
            ],
        );
        let gradient = PaintLinearGradient::new(
            line,
            FWord::new(0),
            FWord::new(0),
            FWord::new(1400),
            FWord::new(0),
            FWord::new(0),
            FWord::new(1400),
        );
        let paint = Paint::Glyph(PaintGlyph::new(Paint::LinearGradient(gradient), o));
        let mut colr = Colr::new(0, None, None, 0);
        colr.base_glyph_list =
            Some(BaseGlyphList::new(1, vec![BaseGlyphPaint::new(o, paint)])).into();
        let bytes = with_colr(colr);
        let font = FontRef::new(&bytes)?;
        assert!(
            ColorProbe::new(&bytes, 0)
                .context("COLR detected")?
                .has(o.to_u32())
        );
        let raster = render(&font, &[], o.to_u32(), 64.0)?.context("color raster")?;
        // Average red and blue over the opaque pixels of each half of the glyph.
        let width = raster.width as usize;
        let mut sums = [[0u64; 3]; 2];
        for (index, p) in raster.bgra.chunks_exact(4).enumerate() {
            if p[3] > 200 {
                let half = ((index % width) * 2 / width).min(1);
                sums[half][0] += p[2] as u64;
                sums[half][1] += p[0] as u64;
                sums[half][2] += 1;
            }
        }
        let [left, right] = sums;
        assert!(left[2] > 0 && right[2] > 0);
        assert!(
            left[0] * right[2] > right[0] * left[2],
            "redder on the left"
        );
        assert!(
            right[1] * left[2] > left[1] * right[2],
            "bluer on the right"
        );
        Ok(())
    }
}
