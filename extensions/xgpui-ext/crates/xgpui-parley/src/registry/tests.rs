use super::*;
use crate::test_support::{INTER, PLEX, fonts, shape};
use std::sync::Arc;

#[test]
fn variable_instances_keep_ids_coordinates_and_real_measurements() -> Result<()> {
    let mut fonts = fonts();
    let mut registry = FaceRegistry::default();
    let mut instances = Vec::new();
    for weight in [100.0, 900.0] {
        let layout = shape(&mut fonts, "H", "Inter Variable", weight);
        let line = layout.lines().next().unwrap();
        let run = line.runs().next().unwrap();
        assert!(
            !run.normalized_coords().is_empty(),
            "fixture must resolve to variable font"
        );
        let id = registry.register(&run)?;
        assert_eq!(registry.register(&run)?, id);
        let face = registry.get(id)?;
        assert_eq!(face.data().data.id(), run.font().data.id());
        assert_eq!(face.data().index, run.font().index);
        assert_eq!(
            face.normalized_coords()
                .iter()
                .map(|c| c.to_bits())
                .chain(std::iter::repeat(0))
                .take(run.normalized_coords().len())
                .collect::<Vec<_>>(),
            run.normalized_coords()
                .iter()
                .map(|c| c.to_bits())
                .collect::<Vec<_>>()
        );
        let metrics = face.metrics()?;
        let h = face.glyph_for_char('H').unwrap();
        let advance = face.advance(h)?.width;
        // Parley shapes with the variation instance; unit conversion must agree.
        assert!((advance * 16.0 / metrics.units_per_em as f32 - layout.width()).abs() < 0.02);
        assert!(metrics.ascent > 0.0 && metrics.descent < 0.0);
        let period = face.glyph_for_char('.').unwrap();
        assert_ne!(
            face.typographic_bounds(h)?,
            face.typographic_bounds(period)?
        );
        assert!(face.advance(GlyphId(u32::MAX)).is_err());
        instances.push((id, advance, face.typographic_bounds(h)?));
    }
    assert_ne!(instances[0].0, instances[1].0);
    assert_ne!(instances[0].1, instances[1].1, "HVAR must affect advance");
    assert_ne!(
        instances[0].2, instances[1].2,
        "gvar must affect glyph bounds"
    );
    assert_eq!(registry.faces.len(), 2);
    assert!(registry.get(FontId(999)).is_err());
    let font = registry.get(instances[0].0)?.data().clone();
    let axes = FontRef::from_index(font.data.data(), font.index)?
        .axes()
        .len();
    let default = registry.insert(&font, vec![])?;
    assert_eq!(
        registry.insert(&font, vec![NormalizedCoord::default(); axes])?,
        default
    );
    let invalid = FontData::new(font.data.clone(), u32::MAX);
    assert!(registry.insert(&invalid, vec![]).is_err());
    Ok(())
}

#[test]
fn rejects_unimplemented_synthetic_style() {
    let mut fonts = fonts();
    let layout = shape(&mut fonts, "H", "IBM Plex Sans", 900.0);
    let line = layout.lines().next().unwrap();
    let run = line.runs().next().unwrap();
    assert!(run.synthesis().embolden());
    let mut registry = FaceRegistry::default();
    assert!(registry.register(&run).is_err());
    assert!(registry.faces.is_empty());
}

#[test]
fn fallback_glyphs_register_the_actual_face() -> Result<()> {
    let primary = FontRef::new(INTER)?;
    let fallback = FontRef::new(PLEX)?;
    // Find a Latin character in Plex that Inter lacks; no system-font dependency.
    let ch = (0x00c0..0x0250)
        .filter_map(char::from_u32)
        .find(|&ch| primary.charmap().map(ch).is_none() && fallback.charmap().map(ch).is_some())
        .expect("fixtures need a fallback-only Latin character");
    let mut fonts = fonts();
    let layout = shape(
        &mut fonts,
        &format!("H{ch}"),
        "Inter Variable, IBM Plex Sans",
        400.0,
    );
    let mut registry = FaceRegistry::default();
    let converted = crate::line_layout(&layout, gpui::px(16.0), |run| registry.register(run))?;
    assert_eq!(converted.runs.len(), 2, "text must use both fixture faces");
    assert_ne!(converted.runs[0].font_id, converted.runs[1].font_id);
    for run in &converted.runs {
        let face = registry.get(run.font_id)?;
        let source = if run.glyphs[0].index == 0 { 'H' } else { ch };
        assert_eq!(run.glyphs[0].id, face.glyph_for_char(source).unwrap());
    }
    let face = registry.get(converted.runs[1].font_id)?;
    assert_eq!(face.data().data.data(), PLEX);
    assert_eq!(
        face.metrics()?.units_per_em,
        fallback
            .metrics(FontSize::unscaled(), LocationRef::default())
            .units_per_em as u32
    );
    Ok(())
}

#[test]
fn collection_index_is_part_of_identity() -> Result<()> {
    // Wrap two existing faces as a TTC, relocating absolute table offsets.
    let mut ttc = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
    ttc.resize(20, 0);
    for (index, source) in [PLEX, INTER].into_iter().enumerate() {
        let offset = ttc.len();
        ttc[12 + index * 4..16 + index * 4].copy_from_slice(&(offset as u32).to_be_bytes());
        let mut face = source.to_vec();
        let tables = u16::from_be_bytes([face[4], face[5]]) as usize;
        for table in 0..tables {
            let pos = 12 + table * 16 + 8;
            let old = u32::from_be_bytes(face[pos..pos + 4].try_into().unwrap());
            face[pos..pos + 4].copy_from_slice(&(old + offset as u32).to_be_bytes());
        }
        ttc.extend(face);
        ttc.resize(ttc.len().next_multiple_of(4), 0);
    }
    let blob = parley::fontique::Blob::new(Arc::new(ttc));
    let mut registry = FaceRegistry::default();
    let first = registry.insert(&FontData::new(blob.clone(), 0), vec![])?;
    let second_font = FontData::new(blob, 1);
    let axes = FontRef::from_index(second_font.data.data(), 1)?
        .axes()
        .len();
    let second = registry.insert(&second_font, vec![NormalizedCoord::default(); axes])?;
    assert_ne!(first, second);
    assert_eq!(registry.get(first)?.data().index, 0);
    assert_eq!(registry.get(second)?.data().index, 1);
    assert_ne!(
        registry.get(first)?.metrics()?.units_per_em,
        registry.get(second)?.metrics()?.units_per_em
    );
    Ok(())
}

#[test]
fn repeated_variable_face_registration_reuses_query_storage() -> Result<()> {
    let mut fonts = fonts();
    let light = shape(&mut fonts, "H", "Inter Variable", 100.0);
    let heavy = shape(&mut fonts, "H", "Inter Variable", 900.0);
    let light = light.lines().next().unwrap().runs().next().unwrap();
    let heavy = heavy.lines().next().unwrap().runs().next().unwrap();
    let mut registry = FaceRegistry::default();
    let light_id = registry.register(&light)?;
    let heavy_id = registry.register(&heavy)?;
    assert_ne!(light_id, heavy_id);
    let pointer = registry.query.coords.as_ptr();
    let capacity = registry.query.coords.capacity();
    assert!(capacity > 0, "fixture must exercise coordinate storage");
    for _ in 0..100 {
        assert_eq!(registry.register(&light)?, light_id);
        assert_eq!(registry.register(&heavy)?, heavy_id);
        assert_eq!(registry.query.coords.as_ptr(), pointer);
        assert_eq!(registry.query.coords.capacity(), capacity);
    }
    assert_eq!(registry.faces.len(), 2);
    Ok(())
}
