//! Experimental Parley interop: flat LTR lines, resolved-face metrics and
//! grayscale rasterization, with an opt-in `PlatformTextSystem` backend.

use anyhow::{Result, ensure};
use gpui::{FontId, GlyphId, LineLayout, Pixels, ShapedGlyph, ShapedRun, point, px};
use parley::{Alignment, Layout, PositionedLayoutItem, Run};

mod registry;
pub use registry::{FaceRegistry, RegisteredFace};
mod raster;
pub use raster::GlyphRasterizer;
mod backend;
pub use backend::{LayoutErrorHandler, ParleyTextSystem};

#[cfg(test)]
mod test_support;

/// Convert a scale-1, start-aligned Parley line into GPUI's baseline-relative
/// glyph records. The caller owns/reuses Parley's font and layout contexts.
///
/// `resolve_face` must register the actual `run.font()` (including collection
/// index), normalized variation coordinates and synthesis for later metrics and
/// rasterization. Returning the originally requested family ID is insufficient
/// when shaping selected a fallback face.
///
/// This initial bridge rejects paragraphs, inline boxes, RTL and mixed sizes.
/// It adds no layout or raster cache: those remain owned by GPUI's `TextSystem`.
pub fn line_layout(
    layout: &Layout<()>,
    font_size: Pixels,
    mut resolve_face: impl FnMut(&Run<'_, ()>) -> Result<FontId>,
) -> Result<LineLayout> {
    ensure!(
        f32::from(font_size).is_finite() && font_size > px(0.0),
        "invalid font size"
    );
    ensure!(layout.scale() == 1.0, "expected logical-pixel layout");
    ensure!(layout.len() == 1, "expected one unwrapped line");
    ensure!(
        layout.alignment() == Some(Alignment::Start),
        "expected start alignment"
    );
    validate_layout_geometry(layout)?;
    let line = layout.lines().next().unwrap();
    ensure!(
        layout.text_len() == 0 || line.text_range() == (0..layout.text_len()),
        "incomplete line: {:?} for {} source bytes",
        line.text_range(),
        layout.text_len()
    );
    ensure!(
        line.items()
            .all(|item| matches!(item, PositionedLayoutItem::GlyphRun(_))),
        "inline boxes are unsupported"
    );
    for item in line.items() {
        if let PositionedLayoutItem::GlyphRun(item) = item {
            ensure!(
                item.baseline() == line.metrics().baseline,
                "vertical alignment is unsupported"
            );
            ensure!(
                item.glyphs().count()
                    == item
                        .run()
                        .visual_clusters()
                        .map(|cluster| cluster.glyphs().count())
                        .sum::<usize>(),
                "split style runs are unsupported"
            );
        }
    }
    // Validate before allowing the caller to register any faces.
    for run in line.runs() {
        ensure!(!run.is_rtl(), "RTL interaction is not supported yet");
        ensure!(
            run.font_size() == f32::from(font_size),
            "mixed font sizes are unsupported"
        );
    }

    let metrics = line.metrics();
    let mut result = LineLayout {
        font_size,
        width: px(metrics.advance),
        ascent: px(metrics.baseline - metrics.content_block_min_coord),
        descent: px(metrics.content_block_max_coord - metrics.baseline),
        runs: Vec::new(),
        len: layout.text_len(),
    };
    // Parley uses a synthetic 0..1 range for its empty-line strut. Keep its real
    // baseline metrics, but never expose that synthetic character to GPUI.
    if result.len == 0 {
        return Ok(result);
    }
    let mut x = metrics.offset;
    for run in line.runs() {
        let font_id = resolve_face(&run)?;
        let mut glyphs = Vec::new();
        for cluster in run.visual_clusters() {
            for glyph in cluster.glyphs() {
                let position = point(px(x + glyph.x), px(glyph.y));
                ensure!(
                    f32::from(position.x).is_finite() && f32::from(position.y).is_finite(),
                    "nonfinite glyph position"
                );
                glyphs.push(ShapedGlyph {
                    id: GlyphId(glyph.id),
                    position,
                    index: cluster.text_range().start,
                    is_emoji: cluster.is_emoji(),
                });
                x += glyph.advance;
            }
        }
        result.runs.push(ShapedRun { font_id, glyphs });
    }
    Ok(result)
}

// A finite requested font size can still overflow shaping or line breaking.
// Reject invalid geometry before exposing it to GPUI or registering its faces.
fn validate_layout_geometry(layout: &Layout<()>) -> Result<()> {
    ensure!(
        [layout.width(), layout.full_width(), layout.height()]
            .into_iter()
            .all(f32::is_finite),
        "nonfinite paragraph geometry"
    );
    for line in layout.lines() {
        let m = line.metrics();
        ensure!(
            [
                m.line_height,
                m.baseline,
                m.offset,
                m.advance,
                m.block_min_coord,
                m.block_max_coord,
                m.content_block_min_coord,
                m.content_block_max_coord,
                m.baseline - m.content_block_min_coord,
                m.content_block_max_coord - m.baseline
            ]
            .into_iter()
            .all(f32::is_finite),
            "nonfinite line geometry"
        );
    }
    Ok(())
}

mod paragraph;
pub use paragraph::{ParagraphLayout, ParagraphShape, TextPosition, VisualDirection};
