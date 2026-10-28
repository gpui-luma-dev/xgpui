//! Parley paragraph shaping, visual glyph conversion and interaction geometry.
use anyhow::{Result, ensure};
use gpui::{Bounds, FontId, GlyphId, Pixels, Point, ShapedGlyph, ShapedRun, point, px, size};
pub use gpui::{ParagraphPosition as TextPosition, VisualDirection};
use parley::{
    Alignment, AlignmentOptions, BoundingBox, Layout,
    editing::{Cursor, Selection},
    layout::Affinity,
};
use std::{ops::Range, sync::Arc};

/// Immutable shaped data. Caller retains it across width changes; no cache here.
pub struct ParagraphShape {
    source: Arc<str>,
    layout: Layout<()>,
    faces: Arc<[FontId]>,
    face_runs: Arc<[(Range<usize>, FontId)]>,
    font_size: Pixels,
}

/// One immutable reflow, used for every geometry query in this text revision.
pub struct ParagraphLayout {
    source: Arc<str>,
    layout: Layout<()>,
    faces: Arc<[FontId]>,
    font_size: Pixels,
    width: Pixels,
    runs: Vec<ShapedRun>,
}

impl ParagraphShape {
    pub(crate) fn new(
        source: Arc<str>,
        layout: Layout<()>,
        faces: Vec<FontId>,
        face_runs: Vec<(Range<usize>, FontId)>,
        font_size: Pixels,
    ) -> Self {
        Self {
            source,
            layout,
            faces: faces.into(),
            face_runs: face_runs.into(),
            font_size,
        }
    }

    /// Clone Parley's layout and rebreak, without rebuilding/shaping the text.
    /// Clone allocation costs are intentionally not hidden behind another cache.
    /// Width must be finite and nonnegative. Zero preserves content with a zero
    /// available width; unbreakable glyphs may overflow, as at other narrow widths.
    pub fn layout(&self, width: Pixels) -> Result<ParagraphLayout> {
        ensure!(
            f32::from(width).is_finite() && width >= px(0.0),
            "invalid paragraph width"
        );
        let mut layout = self.layout.clone();
        layout.break_all_lines(Some(f32::from(width)));
        layout.align(Alignment::Start, AlignmentOptions::default());
        crate::validate_layout_geometry(&layout)?;
        let mut runs = Vec::new();
        for line in layout.lines().filter(|_| !self.source.is_empty()) {
            for item in line.items() {
                let parley::PositionedLayoutItem::GlyphRun(item) = item else {
                    anyhow::bail!("inline boxes are unsupported");
                };
                let range = item.run().text_range();
                let binding = self
                    .face_runs
                    .partition_point(|(original, _)| original.start <= range.start);
                let font_id = binding
                    .checked_sub(1)
                    .and_then(|index| self.face_runs.get(index))
                    .filter(|(original, _)| original.end >= range.end)
                    .map(|(_, id)| *id)
                    .ok_or_else(|| anyhow::anyhow!("unregistered paragraph run"))?;
                let run = item.run();
                let mut indices = run.visual_clusters().flat_map(|cluster| {
                    let index = cluster.text_range().start;
                    let emoji = cluster.is_emoji();
                    cluster.glyphs().map(move |_| (index, emoji))
                });
                let glyph_count = item.glyphs().count();
                let mut glyphs = Vec::with_capacity(glyph_count);
                for (glyph, (index, is_emoji)) in item.positioned_glyphs().zip(indices.by_ref()) {
                    ensure!(
                        glyph.x.is_finite() && glyph.y.is_finite(),
                        "nonfinite glyph position"
                    );
                    glyphs.push(ShapedGlyph {
                        id: GlyphId(glyph.id),
                        position: point(px(glyph.x), px(glyph.y)),
                        index,
                        is_emoji,
                    });
                }
                // zip stops at the shorter iterator. Check both sides so split
                // style clusters still fail instead of silently dropping glyphs.
                ensure!(
                    glyphs.len() == glyph_count && indices.next().is_none(),
                    "split style clusters are unsupported"
                );
                runs.push(ShapedRun { font_id, glyphs });
            }
        }
        Ok(ParagraphLayout {
            source: self.source.clone(),
            layout,
            faces: self.faces.clone(),
            font_size: self.font_size,
            width,
            runs,
        })
    }
}

impl ParagraphLayout {
    /// Read-only inspection for the prototype, not an export into GPUI core.
    pub fn parley_layout(&self) -> &Layout<()> {
        &self.layout
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Actual instances retained in the backend's existing metrics/raster registry.
    pub fn resolved_faces(&self) -> &[FontId] {
        &self.faces
    }

    fn cursor(&self, position: TextPosition) -> Result<Cursor> {
        ensure!(
            self.source.is_char_boundary(position.byte_index),
            "invalid UTF-8 position"
        );
        // Parley snaps a valid scalar boundary inside a cluster to its start.
        Ok(Cursor::from_byte_index(
            &self.layout,
            position.byte_index,
            to_parley(position.affinity),
        ))
    }

    pub fn hit_test(&self, point: Point<Pixels>) -> Result<TextPosition> {
        ensure!(
            f32::from(point.x).is_finite() && f32::from(point.y).is_finite(),
            "invalid hit point"
        );
        Ok(position(Cursor::from_point(
            &self.layout,
            point.x.into(),
            point.y.into(),
        )))
    }

    /// One logical pixel wide; caller applies element origin and scrolling.
    pub fn caret_bounds(&self, position: TextPosition) -> Result<Bounds<Pixels>> {
        Ok(bounds(self.cursor(position)?.geometry(&self.layout, 1.0)))
    }

    pub fn move_visual(
        &self,
        from: TextPosition,
        direction: VisualDirection,
    ) -> Result<TextPosition> {
        let cursor = self.cursor(from)?;
        Ok(position(match direction {
            VisualDirection::Left => cursor.previous_visual(&self.layout),
            VisualDirection::Right => cursor.next_visual(&self.layout),
        }))
    }

    pub fn selection_bounds(
        &self,
        anchor: TextPosition,
        focus: TextPosition,
    ) -> Result<Vec<Bounds<Pixels>>> {
        let selection = Selection::new(self.cursor(anchor)?, self.cursor(focus)?);
        Ok(selection
            .geometry(&self.layout)
            .into_iter()
            .map(|(rect, _line)| bounds(rect))
            .collect())
    }
}

fn position(cursor: Cursor) -> TextPosition {
    TextPosition {
        byte_index: cursor.index(),
        affinity: match cursor.affinity() {
            Affinity::Upstream => gpui::TextAffinity::Upstream,
            Affinity::Downstream => gpui::TextAffinity::Downstream,
        },
    }
}
fn bounds(rect: BoundingBox) -> Bounds<Pixels> {
    Bounds::new(
        point(px(rect.x0 as f32), px(rect.y0 as f32)),
        size(px(rect.width() as f32), px(rect.height() as f32)),
    )
}

fn to_parley(affinity: gpui::TextAffinity) -> Affinity {
    match affinity {
        gpui::TextAffinity::Upstream => Affinity::Upstream,
        gpui::TextAffinity::Downstream => Affinity::Downstream,
    }
}

impl gpui::PlatformParagraphShape for ParagraphShape {
    fn layout(&self, width: Pixels) -> Result<Arc<dyn gpui::PlatformParagraphLayout>> {
        Ok(Arc::new(self.layout(width)?))
    }
}
impl gpui::PlatformParagraphLayout for ParagraphLayout {
    fn source(&self) -> &str {
        self.source()
    }
    fn font_size(&self) -> Pixels {
        self.font_size
    }
    fn size(&self) -> gpui::Size<Pixels> {
        size(self.width, px(self.layout.height()))
    }
    fn runs(&self) -> &[ShapedRun] {
        &self.runs
    }
    fn hit_test(&self, point: Point<Pixels>) -> Result<TextPosition> {
        self.hit_test(point)
    }
    fn caret_bounds(&self, position: TextPosition) -> Result<Bounds<Pixels>> {
        self.caret_bounds(position)
    }
    fn move_visual(
        &self,
        position: TextPosition,
        direction: VisualDirection,
    ) -> Result<TextPosition> {
        self.move_visual(position, direction)
    }
    fn selection_bounds(
        &self,
        anchor: TextPosition,
        focus: TextPosition,
    ) -> Result<Vec<Bounds<Pixels>>> {
        self.selection_bounds(anchor, focus)
    }
}
