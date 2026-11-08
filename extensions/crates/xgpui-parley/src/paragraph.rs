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
        self.layout_with(Some(width), Alignment::Start)
    }

    /// Narrowest width (every soft break taken) and widest single-line width.
    /// These are the min-content and max-content sizes used by flex layout.
    pub fn content_widths(&self) -> (Pixels, Pixels) {
        let widths = self.layout.calculate_content_widths();
        (px(widths.min), px(widths.max))
    }

    /// Whether the resolved base direction is right-to-left.
    pub fn is_rtl(&self) -> bool {
        self.layout.is_rtl()
    }

    /// Original unmodified source.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Clusters in logical order with their advances, from the unbroken layout.
    pub(crate) fn logical_clusters(&self) -> Vec<(Range<usize>, f32)> {
        let mut clusters: Vec<_> = self
            .layout
            .lines()
            .flat_map(|line| {
                line.runs()
                    .flat_map(|run| {
                        run.clusters()
                            .map(|cluster| (cluster.text_range(), cluster.advance()))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        clusters.sort_by_key(|(range, _)| range.start);
        clusters
    }

    /// Reflow with an optional break width. Without one, lines break only at hard
    /// breaks and Parley aligns each line within the widest line; callers then
    /// place that block inside their own bounds.
    pub(crate) fn layout_with(
        &self,
        break_width: Option<Pixels>,
        alignment: Alignment,
    ) -> Result<ParagraphLayout> {
        if let Some(width) = break_width {
            ensure!(
                f32::from(width).is_finite() && width >= px(0.0),
                "invalid paragraph width"
            );
        }
        let mut layout = self.layout.clone();
        layout.break_all_lines(break_width.map(f32::from));
        layout.align(alignment, AlignmentOptions::default());
        let width = break_width.unwrap_or(px(layout.width()));
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
                // Emoji are glyphs the resolved face has color data for, as in native
                // backends; Unicode emoji presentation alone does not imply color.
                let color =
                    crate::raster::color::ColorProbe::new(run.font().data.data(), run.font().index);
                let mut indices = run.visual_clusters().flat_map(|cluster| {
                    let index = cluster.text_range().start;
                    cluster.glyphs().map(move |_| index)
                });
                let glyph_count = item.glyphs().count();
                let mut glyphs = Vec::with_capacity(glyph_count);
                for (glyph, index) in item.positioned_glyphs().zip(indices.by_ref()) {
                    let is_emoji = color.as_ref().is_some_and(|color| color.has(glyph.id));
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
    /// Widest line, excluding trailing whitespace.
    pub fn content_width(&self) -> Pixels {
        px(self.layout.width())
    }
    /// Number of laid-out lines.
    pub fn line_count(&self) -> usize {
        self.layout.len()
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
