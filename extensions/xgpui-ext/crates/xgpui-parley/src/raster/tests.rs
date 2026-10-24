use super::*;
use crate::test_support::{fonts, shape};
use gpui::{FontId, GlyphId, px};

fn params(font_id: FontId, glyph_id: GlyphId) -> RenderGlyphParams {
    RenderGlyphParams {
        font_id,
        glyph_id,
        font_size: px(24.0),
        scale_factor: 1.0,
        subpixel_variant: point(0, 0),
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    }
}

#[test]
fn masks_match_bounds_baselines_scale_and_subpixel_position() -> Result<()> {
    let layout = shape(&mut fonts(), "Hg ", "IBM Plex Sans", 400.0);
    let mut faces = FaceRegistry::default();
    let line = crate::line_layout(&layout, px(16.0), |run| faces.register(run))?;
    let id = line.runs[0].font_id;
    let face = faces.get(id)?;
    let mut raster = GlyphRasterizer::default();
    let h = params(id, face.glyph_for_char('H').unwrap());
    let bounds = raster.raster_bounds(&faces, &h)?;
    let (extent, bytes) = raster.rasterize_glyph(&faces, &h, bounds)?;
    assert!(bounds.origin.y.0 < 0, "capital extends above baseline");
    assert_eq!(extent, bounds.size);
    assert_eq!(
        bytes.len(),
        extent.width.0 as usize * extent.height.0 as usize
    );
    assert!(bytes.iter().any(|&alpha| alpha > 0));
    assert!(
        bytes.iter().any(|&alpha| alpha > 0 && alpha < 255),
        "antialiased edges"
    );
    assert_eq!(raster.rasterize_glyph(&faces, &h, bounds)?.1, bytes);

    let g = params(id, face.glyph_for_char('g').unwrap());
    let descender = raster.raster_bounds(&faces, &g)?;
    assert!(
        descender.origin.y.0 + descender.size.height.0 > 0,
        "descender below baseline"
    );
    assert_eq!(
        raster.hints.len(),
        1,
        "hinting reused across glyphs and calls"
    );
    assert_eq!(raster.hint_builds, 1);
    let shifted = RenderGlyphParams {
        subpixel_variant: point(1, 0),
        ..h.clone()
    };
    let shifted_bounds = raster.raster_bounds(&faces, &shifted)?;
    assert_ne!(
        raster.rasterize_glyph(&faces, &shifted, shifted_bounds)?.1,
        bytes
    );
    assert_eq!(
        raster.hints.len(),
        1,
        "subpixel shift does not rebuild hinting"
    );
    let scaled = RenderGlyphParams {
        scale_factor: 2.0,
        ..h.clone()
    };
    let larger = raster.raster_bounds(&faces, &scaled)?;
    assert!(larger.size.height > bounds.size.height);
    assert_eq!(raster.hints.len(), 2);
    assert_eq!(raster.hint_builds, 2);
    let space = params(id, face.glyph_for_char(' ').unwrap());
    let empty = raster.raster_bounds(&faces, &space)?;
    assert_eq!(empty, Bounds::default());
    assert!(raster.rasterize_glyph(&faces, &space, empty)?.1.is_empty());
    assert!(
        raster
            .rasterize_glyph(&faces, &h, Bounds::default())
            .is_err()
    );
    Ok(())
}

#[test]
fn variations_and_registry_identity_do_not_reuse_wrong_hints() -> Result<()> {
    let mut fonts = fonts();
    let mut faces = FaceRegistry::default();
    let mut raster = GlyphRasterizer::default();
    let mut masks = Vec::new();
    for weight in [100.0, 900.0] {
        let layout = shape(&mut fonts, "H", "Inter Variable", weight);
        let run = layout.lines().next().unwrap().runs().next().unwrap();
        let id = faces.register(&run)?;
        let params = params(id, faces.get(id)?.glyph_for_char('H').unwrap());
        let bounds = raster.raster_bounds(&faces, &params)?;
        masks.push(raster.rasterize_glyph(&faces, &params, bounds)?);
    }
    assert_ne!(
        masks[0], masks[1],
        "retained variation affects rasterization"
    );
    assert_eq!(raster.hints.len(), 2);
    // A different registry starts at FontId(0), but its blob/index/location differ.
    let layout = shape(&mut fonts, "H", "IBM Plex Sans", 400.0);
    let run = layout.lines().next().unwrap().runs().next().unwrap();
    let mut other = FaceRegistry::default();
    let id = other.register(&run)?;
    assert_eq!(id, FontId(0));
    let p = params(id, other.get(id)?.glyph_for_char('H').unwrap());
    raster.raster_bounds(&other, &p)?;
    assert_eq!(raster.hints.len(), 3);
    for size in 1..=70 {
        raster.raster_bounds(
            &other,
            &RenderGlyphParams {
                font_size: px(size as f32),
                ..p.clone()
            },
        )?;
    }
    assert_eq!(raster.hints.len(), HINT_LIMIT, "hint setup stays bounded");
    Ok(())
}

#[test]
fn rejects_unsupported_modes_and_invalid_requests() -> Result<()> {
    let layout = shape(&mut fonts(), "H", "IBM Plex Sans", 400.0);
    let mut faces = FaceRegistry::default();
    let run = layout.lines().next().unwrap().runs().next().unwrap();
    let id = faces.register(&run)?;
    let p = params(id, faces.get(id)?.glyph_for_char('H').unwrap());
    let mut raster = GlyphRasterizer::default();
    for bad in [
        RenderGlyphParams {
            is_emoji: true,
            ..p.clone()
        },
        RenderGlyphParams {
            subpixel_rendering: true,
            ..p.clone()
        },
        RenderGlyphParams {
            dilation: 1,
            ..p.clone()
        },
        RenderGlyphParams {
            subpixel_variant: point(SUBPIXEL_VARIANTS_X, 0),
            ..p.clone()
        },
        RenderGlyphParams {
            scale_factor: f32::NAN,
            ..p.clone()
        },
        RenderGlyphParams {
            font_size: px(-1.0),
            ..p.clone()
        },
        RenderGlyphParams {
            font_size: px(1e9),
            ..p.clone()
        },
        RenderGlyphParams {
            font_id: FontId(999),
            ..p.clone()
        },
        RenderGlyphParams {
            glyph_id: GlyphId(u32::MAX),
            ..p
        },
    ] {
        assert!(raster.raster_bounds(&faces, &bad).is_err());
    }
    assert!(raster.hints.is_empty());
    Ok(())
}

#[test]
fn bounds_then_raster_reuses_outline_and_interleaving_remains_correct() -> Result<()> {
    let layout = shape(&mut fonts(), "Hg", "IBM Plex Sans", 400.0);
    let mut faces = FaceRegistry::default();
    let run = layout.lines().next().unwrap().runs().next().unwrap();
    let id = faces.register(&run)?;
    let face = faces.get(id)?;
    let h = params(id, face.glyph_for_char('H').unwrap());
    let g = params(id, face.glyph_for_char('g').unwrap());
    let mut raster = GlyphRasterizer::default();
    let h_bounds = raster.raster_bounds(&faces, &h)?;
    assert_eq!(raster.outline_builds, 1);
    assert_eq!(raster.raster_bounds(&faces, &h)?, h_bounds);
    let h_mask = raster.rasterize_glyph(&faces, &h, h_bounds)?;
    assert_eq!(
        raster.outline_builds, 1,
        "bounds and bitmap share one outline"
    );
    assert!(
        raster.prepared.is_none(),
        "raster consumes the staged outline"
    );

    // Bounds calls may be interleaved or cached by GPUI; raster must still work.
    raster.raster_bounds(&faces, &h)?;
    let g_bounds = raster.raster_bounds(&faces, &g)?;
    assert_eq!(raster.rasterize_glyph(&faces, &h, h_bounds)?, h_mask);
    let direct = GlyphRasterizer::default().rasterize_glyph(&faces, &g, g_bounds)?;
    assert_eq!(raster.rasterize_glyph(&faces, &g, g_bounds)?, direct);
    Ok(())
}

#[test]
fn hint_cache_hits_refresh_recency_and_evict_only_the_oldest() -> Result<()> {
    let layout = shape(&mut fonts(), "H", "Inter Variable", 900.0);
    let mut faces = FaceRegistry::default();
    let run = layout.lines().next().unwrap().runs().next().unwrap();
    let id = faces.register(&run)?;
    let face = faces.get(id)?;
    assert!(!face.normalized_coords().is_empty());
    let first = face.coordinate_handle();
    let second = face.coordinate_handle();
    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "lookup keys share coordinate storage"
    );
    let base = params(id, face.glyph_for_char('H').unwrap());
    let sized = |size| RenderGlyphParams {
        font_size: px(size as f32),
        ..base.clone()
    };
    let mut raster = GlyphRasterizer::default();
    for size in 1..=HINT_LIMIT {
        raster.raster_bounds(&faces, &sized(size))?;
    }
    assert_eq!(raster.hint_builds, HINT_LIMIT);
    // Refresh the oldest entry, then exceed capacity: size 2 must be evicted.
    raster.raster_bounds(&faces, &sized(1))?;
    assert_eq!(raster.hint_builds, HINT_LIMIT);
    raster.raster_bounds(&faces, &sized(HINT_LIMIT + 1))?;
    raster.raster_bounds(&faces, &sized(1))?;
    assert_eq!(raster.hint_builds, HINT_LIMIT + 1);
    raster.raster_bounds(&faces, &sized(2))?;
    assert_eq!(raster.hint_builds, HINT_LIMIT + 2);
    assert_eq!(raster.hints.len(), HINT_LIMIT);
    let bounds = raster.raster_bounds(&faces, &base)?;
    let actual = raster.rasterize_glyph(&faces, &base, bounds)?;
    let expected = GlyphRasterizer::default().rasterize_glyph(&faces, &base, bounds)?;
    assert_eq!(actual, expected, "eviction and reuse preserve glyph output");
    Ok(())
}
