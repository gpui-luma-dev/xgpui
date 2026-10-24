use anyhow::{Context, Result, ensure};
use gpui::{Bounds, FontId, FontMetrics, GlyphId, Size, point, size};
use parley::{FontData, Run};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, NormalizedCoord, Size as FontSize},
    metrics::BoundingBox,
};
use std::collections::HashMap;

#[derive(Hash, PartialEq, Eq)]
struct FaceKey {
    blob: u64,
    index: u32,
    coords: Vec<NormalizedCoord>,
}

/// Registry of resolved font instances. IDs are stable for this registry's
/// lifetime and must not be shared with another registry or reused after reset.
/// Retain one registry per text backend, not per line or frame.
#[derive(Default)]
pub struct FaceRegistry {
    ids: HashMap<FaceKey, FontId>,
    faces: Vec<RegisteredFace>,
}

impl FaceRegistry {
    /// Retain the actual resolved face and its normalized variation location.
    /// Faux bold/italic are rejected until metrics and rasterization can apply
    /// identical synthesis. Variable-font weight/slant coordinates are supported.
    pub fn register(&mut self, run: &Run<'_, ()>) -> Result<FontId> {
        let synthesis = run.synthesis();
        ensure!(
            !synthesis.embolden() && synthesis.skew().is_none(),
            "synthetic bold/italic is not supported yet"
        );
        let coords = run
            .normalized_coords()
            .iter()
            .map(|coord| NormalizedCoord::from_bits(coord.to_bits()))
            .collect();
        self.insert(run.font(), coords)
    }

    fn insert(&mut self, font: &FontData, coords: Vec<NormalizedCoord>) -> Result<FontId> {
        // Parley can omit trailing default axes. Canonicalize equivalent
        // locations without changing any non-default coordinate bits.
        let mut coords = coords;
        while coords.last() == Some(&NormalizedCoord::default()) {
            coords.pop();
        }
        let key = FaceKey {
            blob: font.data.id(),
            index: font.index,
            coords,
        };
        if let Some(id) = self.ids.get(&key) {
            return Ok(*id);
        }
        let parsed = FontRef::from_index(font.data.data(), font.index)?;
        ensure!(
            key.coords.len() <= parsed.axes().len(),
            "too many variation coordinates: {} for {} axes",
            key.coords.len(),
            parsed.axes().len()
        );
        let metrics = parsed.metrics(FontSize::unscaled(), LocationRef::new(&key.coords));
        ensure!(
            metrics.units_per_em > 0 && metrics.glyph_count > 0,
            "invalid font metrics"
        );
        let id = FontId(self.faces.len());
        self.faces.push(RegisteredFace {
            font: font.clone(),
            coords: key.coords.clone(),
        });
        self.ids.insert(key, id);
        Ok(id)
    }

    /// Look up the immutable face instance retained under a GPUI font ID.
    pub fn get(&self, id: FontId) -> Result<&RegisteredFace> {
        self.faces.get(id.0).context("unknown registered font ID")
    }

    // Requested styles need distinct IDs even when they resolve to the same
    // face: their features/fallbacks may differ. Resolved runs keep canonical IDs.
    pub(crate) fn alias(&mut self, id: FontId) -> Result<FontId> {
        let face = self.get(id)?.clone();
        let id = FontId(self.faces.len());
        self.faces.push(face);
        Ok(id)
    }
}

/// Font bytes, collection index and variation location used by shaping.
/// Rasterization uses this instance rather than resolving a family again.
#[derive(Clone)]
pub struct RegisteredFace {
    font: FontData,
    coords: Vec<NormalizedCoord>,
}

impl RegisteredFace {
    /// Shared font bytes and collection index; cloning the handle does not copy bytes.
    pub fn data(&self) -> &FontData {
        &self.font
    }

    /// Normalized coordinates with trailing zero axes omitted. Non-default
    /// values preserve Parley's fixed-point bits; omitted axes remain at default.
    pub fn normalized_coords(&self) -> &[NormalizedCoord] {
        &self.coords
    }

    fn font(&self) -> Result<FontRef<'_>> {
        Ok(FontRef::from_index(self.font.data.data(), self.font.index)?)
    }

    /// Real font metrics in unscaled, y-up font units. Missing optional heights
    /// are measured from H/x where possible; unavailable decorations/heights use zero.
    /// The global bounding box follows GPUI's head-table contract, not a per-instance
    /// union of all variable glyph outlines.
    pub fn metrics(&self) -> Result<FontMetrics> {
        let font = self.font()?;
        let location = LocationRef::new(&self.coords);
        let metrics = font.metrics(FontSize::unscaled(), location);
        let glyphs = font.glyph_metrics(FontSize::unscaled(), location);
        let height = |ch| {
            font.charmap()
                .map(ch)
                .and_then(|id| glyphs.bounds(id))
                .map(|b| b.y_max)
        };
        let underline = metrics.underline.unwrap_or_default();
        Ok(FontMetrics {
            units_per_em: metrics.units_per_em.into(),
            ascent: metrics.ascent,
            descent: metrics.descent,
            line_gap: metrics.leading,
            underline_position: underline.offset,
            underline_thickness: underline.thickness,
            cap_height: metrics.cap_height.or_else(|| height('H')).unwrap_or(0.0),
            x_height: metrics.x_height.or_else(|| height('x')).unwrap_or(0.0),
            bounding_box: bounds(metrics.bounds.context("missing font bounding box")?),
        })
    }

    /// Nominal character mapping on this exact face; shaping may choose another glyph.
    pub fn glyph_for_char(&self, ch: char) -> Option<GlyphId> {
        let id = self.font().ok()?.charmap().map(ch)?;
        (id.to_u32() != 0).then_some(GlyphId(id.to_u32()))
    }

    /// Horizontal advance in unscaled font units at the retained variation location.
    pub fn advance(&self, glyph: GlyphId) -> Result<Size<f32>> {
        let font = self.font()?;
        let metrics = font.glyph_metrics(FontSize::unscaled(), LocationRef::new(&self.coords));
        let width = metrics
            .advance_width(skrifa::GlyphId::new(glyph.0))
            .context("missing glyph advance")?;
        Ok(size(width, 0.0))
    }

    /// Glyph bounds in unscaled, y-up font units at the retained variation location.
    pub fn typographic_bounds(&self, glyph: GlyphId) -> Result<Bounds<f32>> {
        let font = self.font()?;
        let metrics = font.glyph_metrics(FontSize::unscaled(), LocationRef::new(&self.coords));
        Ok(bounds(
            metrics
                .bounds(skrifa::GlyphId::new(glyph.0))
                .context("missing glyph bounds")?,
        ))
    }
}

fn bounds(b: BoundingBox) -> Bounds<f32> {
    Bounds {
        origin: point(b.x_min, b.y_min),
        size: size(b.x_max - b.x_min, b.y_max - b.y_min),
    }
}

#[cfg(test)]
mod tests;
