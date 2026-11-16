//! Left-to-right paragraphs built from flat line layout, for text backends
//! without their own paragraph engine ([`crate::ParagraphSupport::LeftToRight`]).
//!
//! Each hard line (split at U+2028) is shaped once with the backend's
//! `layout_line`. Reflow only rewraps those shaped lines: greedy breaks at
//! line-break opportunities, trailing spaces hang at the line end, and a word
//! wider than the line breaks at a cluster boundary. Visual order is logical
//! order; base direction and language are ignored.
use super::LineWrapper;
use crate::{
    Bounds, FontRun, LineLayout, ParagraphPosition, ParagraphRequest, Pixels,
    PlatformParagraphLayout, PlatformParagraphShape, Point, Result, ShapedGlyph, ShapedRun, Size,
    TextAffinity, VisualDirection, point, px, size,
};
use anyhow::{bail, ensure};
use std::{ops::Range, sync::Arc};

const HARD_BREAK: char = '\u{2028}';

/// Shape a paragraph from flat lines produced by `layout_line`.
pub(crate) fn shape_paragraph(
    request: ParagraphRequest<'_>,
    layout_line: impl Fn(&str, Pixels, &[FontRun]) -> LineLayout,
) -> Result<Arc<dyn PlatformParagraphShape>> {
    let ParagraphRequest {
        text,
        font_size,
        runs,
        ..
    } = request;
    ensure!(
        f32::from(font_size).is_finite() && font_size > px(0.0),
        "invalid font size"
    );
    ensure!(
        !text.contains(['\n', '\r', '\u{2029}']),
        "expected one logical paragraph"
    );
    let mut segments = Vec::new();
    let mut start = 0;
    for (end, _) in text.match_indices(HARD_BREAK).chain([(text.len(), "")]) {
        segments.push(Segment::new(
            text,
            start..end,
            font_size,
            runs,
            &layout_line,
        ));
        start = end + HARD_BREAK.len_utf8();
    }
    Ok(Arc::new(FallbackShape {
        source: text.into(),
        font_size,
        segments: segments.into(),
    }))
}

/// One hard line, shaped once. Offsets are relative to `range.start`.
struct Segment {
    range: Range<usize>,
    layout: LineLayout,
    ascent: Pixels,
    descent: Pixels,
    /// Cluster boundaries and their x positions, ascending, including `0` and the length.
    stops: Vec<(usize, Pixels)>,
    /// Soft break opportunities strictly inside the segment, on cluster boundaries.
    breaks: Vec<usize>,
}

impl Segment {
    fn new(
        text: &str,
        range: Range<usize>,
        font_size: Pixels,
        runs: &[FontRun],
        layout_line: &impl Fn(&str, Pixels, &[FontRun]) -> LineLayout,
    ) -> Self {
        let source = &text[range.clone()];
        let segment_runs = slice_runs(runs, range.clone());
        let layout = layout_line(source, font_size, &segment_runs);
        // An empty line has no glyphs to give it metrics; measure a space in
        // the font at that position instead.
        let (ascent, descent) = if source.is_empty() {
            let font = runs_font_at(runs, range.start);
            let metrics = layout_line(" ", font_size, &font);
            (metrics.ascent, metrics.descent)
        } else {
            (layout.ascent, layout.descent)
        };
        // Backends disagree on the sign of descent; it always extends below the baseline.
        let descent = descent.abs();
        let mut stops: Vec<(usize, Pixels)> = layout
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter())
            .filter(|glyph| glyph.index < source.len() && source.is_char_boundary(glyph.index))
            .map(|glyph| (glyph.index, glyph.position.x))
            .collect();
        stops.sort_by_key(|(index, _)| *index);
        stops.dedup_by_key(|(index, _)| *index);
        if stops.first().is_none_or(|(index, _)| *index != 0) {
            stops.insert(0, (0, px(0.0)));
        }
        stops.push((source.len(), layout.width));
        stops.dedup_by_key(|(index, _)| *index);
        let breaks = LineWrapper::break_indices(source)
            .filter(|&index| index > 0 && index < source.len())
            .filter(|index| stops.binary_search_by_key(index, |(i, _)| *i).is_ok())
            .collect();
        Self {
            range,
            layout,
            ascent,
            descent,
            stops,
            breaks,
        }
    }

    fn len(&self) -> usize {
        self.range.len()
    }

    fn x(&self, offset: usize) -> Option<Pixels> {
        self.stops
            .binary_search_by_key(&offset, |(index, _)| *index)
            .ok()
            .map(|ix| self.stops[ix].1)
    }

    /// End of `start..end` without its trailing whitespace, as a cluster boundary.
    fn content_end(&self, text: &str, start: usize, end: usize) -> usize {
        let piece = &text[self.range.start + start..self.range.start + end];
        let trimmed = start + piece.trim_end().len();
        // Snap up to a cluster boundary so the measured width stays defined.
        self.stops
            .iter()
            .map(|(index, _)| *index)
            .find(|&index| index >= trimmed)
            .unwrap_or(end)
            .min(end)
    }

    fn width_between(&self, start: usize, end: usize) -> Pixels {
        match (self.x(start), self.x(end)) {
            (Some(start), Some(end)) => end - start,
            _ => px(0.0),
        }
    }

    /// Greedy line ends (relative offsets) for `width`; the last is `len()`.
    fn wrap(&self, text: &str, width: Pixels) -> Vec<usize> {
        let mut ends = Vec::new();
        let mut start = 0;
        loop {
            let rest_end = self.content_end(text, start, self.len());
            if self.width_between(start, rest_end) <= width {
                ends.push(self.len());
                return ends;
            }
            let mut end = None;
            for &candidate in self.breaks.iter().filter(|&&b| b > start) {
                let content = self.content_end(text, start, candidate);
                if self.width_between(start, content) <= width {
                    end = Some(candidate);
                } else {
                    break;
                }
            }
            // A word wider than the line: break at the last cluster that fits,
            // always taking at least one cluster so every line makes progress.
            let end = end.unwrap_or_else(|| {
                let first = self
                    .stops
                    .iter()
                    .map(|(index, _)| *index)
                    .find(|&index| index > start)
                    .unwrap_or(self.len());
                self.stops
                    .iter()
                    .map(|(index, _)| *index)
                    .filter(|&index| index > start)
                    .take_while(|&index| self.width_between(start, index) <= width)
                    .last()
                    .unwrap_or(first)
            });
            ends.push(end);
            start = end;
            if start >= self.len() {
                return ends;
            }
        }
    }
}

fn slice_runs(runs: &[FontRun], range: Range<usize>) -> Vec<FontRun> {
    let mut sliced = Vec::new();
    let mut offset = 0;
    for run in runs {
        let run_range = offset..offset + run.len;
        offset = run_range.end;
        let start = run_range.start.max(range.start);
        let end = run_range.end.min(range.end);
        if start < end {
            sliced.push(FontRun {
                len: end - start,
                font_id: run.font_id,
            });
        }
    }
    sliced
}

/// A one-byte run in the font covering `index` (or the last font), for metrics.
fn runs_font_at(runs: &[FontRun], index: usize) -> Vec<FontRun> {
    let mut offset = 0;
    let mut font = None;
    for run in runs {
        font = Some(run.font_id);
        offset += run.len;
        if index < offset {
            break;
        }
    }
    font.map(|font_id| vec![FontRun { len: 1, font_id }])
        .unwrap_or_default()
}

struct FallbackShape {
    source: Arc<str>,
    font_size: Pixels,
    segments: Arc<[Segment]>,
}

impl PlatformParagraphShape for FallbackShape {
    fn layout(&self, width: Pixels) -> Result<Arc<dyn PlatformParagraphLayout>> {
        ensure!(
            f32::from(width).is_finite() && width >= px(0.0),
            "invalid paragraph width"
        );
        let mut lines = Vec::new();
        let mut top = px(0.0);
        for (segment_ix, segment) in self.segments.iter().enumerate() {
            let mut start = 0;
            let ends = segment.wrap(&self.source, width);
            let count = ends.len();
            for (ix, end) in ends.into_iter().enumerate() {
                let height = segment.ascent + segment.descent;
                lines.push(Line {
                    segment: segment_ix,
                    range: start..end,
                    x0: segment.x(start).unwrap_or_default(),
                    top,
                    soft_end: ix + 1 < count,
                });
                top += height;
                start = end;
            }
        }
        let mut runs = Vec::new();
        for line in &lines {
            let segment = &self.segments[line.segment];
            let baseline = line.top + segment.ascent;
            for run in &segment.layout.runs {
                let glyphs: Vec<ShapedGlyph> = run
                    .glyphs
                    .iter()
                    .filter(|glyph| line.range.contains(&glyph.index))
                    .map(|glyph| ShapedGlyph {
                        id: glyph.id,
                        position: point(glyph.position.x - line.x0, baseline + glyph.position.y),
                        index: segment.range.start + glyph.index,
                        is_emoji: glyph.is_emoji,
                    })
                    .collect();
                if !glyphs.is_empty() {
                    runs.push(ShapedRun {
                        font_id: run.font_id,
                        glyphs,
                    });
                }
            }
        }
        Ok(Arc::new(FallbackLayout {
            source: self.source.clone(),
            font_size: self.font_size,
            segments: self.segments.clone(),
            size: size(width, top),
            lines,
            runs,
        }))
    }

    fn content_widths(&self) -> Result<(Pixels, Pixels)> {
        let mut min = px(0.0);
        let mut max = px(0.0);
        for segment in self.segments.iter() {
            let end = segment.content_end(&self.source, 0, segment.len());
            max = max.max(segment.width_between(0, end));
            let mut start = 0;
            for piece_end in segment.breaks.iter().copied().chain([segment.len()]) {
                let content = segment.content_end(&self.source, start, piece_end);
                min = min.max(segment.width_between(start, content));
                start = piece_end;
            }
        }
        Ok((min, max))
    }
}

/// One visual line: a relative range within its segment.
struct Line {
    segment: usize,
    range: Range<usize>,
    /// Segment x of the line start; subtracted so every line starts at x = 0.
    x0: Pixels,
    top: Pixels,
    /// Ends at a soft wrap, so its end is also the next line's start.
    soft_end: bool,
}

struct FallbackLayout {
    source: Arc<str>,
    font_size: Pixels,
    segments: Arc<[Segment]>,
    size: Size<Pixels>,
    lines: Vec<Line>,
    runs: Vec<ShapedRun>,
}

impl FallbackLayout {
    fn height(&self, line: &Line) -> Pixels {
        let segment = &self.segments[line.segment];
        segment.ascent + segment.descent
    }

    fn absolute(&self, line: &Line, offset: usize) -> usize {
        self.segments[line.segment].range.start + offset
    }

    /// The visual line showing `position`, and its offset within the segment.
    fn locate(&self, position: ParagraphPosition) -> Result<(usize, usize)> {
        let byte = position.byte_index;
        let Some(segment_ix) = self
            .segments
            .iter()
            .position(|segment| segment.range.start <= byte && byte <= segment.range.end)
        else {
            bail!("paragraph position {byte} is outside the text or inside a line break");
        };
        let segment = &self.segments[segment_ix];
        let offset = byte - segment.range.start;
        ensure!(
            segment.x(offset).is_some(),
            "paragraph position {byte} is not a cluster boundary"
        );
        let mut found = None;
        for (ix, line) in self.lines.iter().enumerate() {
            if line.segment != segment_ix {
                continue;
            }
            if line.range.start <= offset && offset < line.range.end {
                found = Some(ix);
                break;
            }
            if offset == line.range.end {
                // At a soft wrap the boundary is shared; affinity picks the line.
                found = Some(ix);
                if !(line.soft_end && position.affinity == TextAffinity::Downstream) {
                    break;
                }
            }
        }
        match found {
            Some(ix) => Ok((ix, offset)),
            None => bail!("paragraph position {byte} has no line"),
        }
    }

    /// Every cluster boundary in the paragraph, ascending, as absolute bytes.
    fn boundaries(&self) -> impl Iterator<Item = usize> + '_ {
        self.segments.iter().flat_map(|segment| {
            segment
                .stops
                .iter()
                .map(move |(index, _)| segment.range.start + index)
        })
    }
}

impl PlatformParagraphLayout for FallbackLayout {
    fn source(&self) -> &str {
        &self.source
    }

    fn font_size(&self) -> Pixels {
        self.font_size
    }

    fn size(&self) -> Size<Pixels> {
        self.size
    }

    fn runs(&self) -> &[ShapedRun] {
        &self.runs
    }

    fn hit_test(&self, point: Point<Pixels>) -> Result<ParagraphPosition> {
        let line = self
            .lines
            .iter()
            .find(|line| point.y < line.top + self.height(line))
            .or(self.lines.last())
            .ok_or_else(|| anyhow::anyhow!("paragraph has no lines"))?;
        let segment = &self.segments[line.segment];
        let x = point.x + line.x0;
        let (offset, _) = segment
            .stops
            .iter()
            .filter(|(index, _)| line.range.contains(index) || *index == line.range.end)
            .min_by(|(_, a), (_, b)| {
                f32::from((*a - x).abs()).total_cmp(&f32::from((*b - x).abs()))
            })
            .copied()
            .unwrap_or((line.range.start, line.x0));
        let affinity = if line.soft_end && offset == line.range.end {
            TextAffinity::Upstream
        } else {
            TextAffinity::Downstream
        };
        Ok(ParagraphPosition {
            byte_index: self.absolute(line, offset),
            affinity,
        })
    }

    fn caret_bounds(&self, position: ParagraphPosition) -> Result<Bounds<Pixels>> {
        let (line_ix, offset) = self.locate(position)?;
        let line = &self.lines[line_ix];
        let x = self.segments[line.segment].x(offset).unwrap_or_default() - line.x0;
        Ok(Bounds::new(
            point(x, line.top),
            size(px(1.0), self.height(line)),
        ))
    }

    fn move_visual(
        &self,
        position: ParagraphPosition,
        direction: VisualDirection,
    ) -> Result<ParagraphPosition> {
        self.locate(position)?;
        let byte = position.byte_index;
        let target = match direction {
            VisualDirection::Right => self.boundaries().find(|&b| b > byte),
            VisualDirection::Left => self.boundaries().filter(|&b| b < byte).last(),
        };
        Ok(match target {
            Some(byte_index) => ParagraphPosition {
                byte_index,
                affinity: TextAffinity::Downstream,
            },
            None => position,
        })
    }

    fn selection_bounds(
        &self,
        anchor: ParagraphPosition,
        focus: ParagraphPosition,
    ) -> Result<Vec<Bounds<Pixels>>> {
        self.locate(anchor)?;
        self.locate(focus)?;
        let start = anchor.byte_index.min(focus.byte_index);
        let end = anchor.byte_index.max(focus.byte_index);
        let mut rects = Vec::new();
        for line in &self.lines {
            let segment = &self.segments[line.segment];
            let from = start
                .saturating_sub(segment.range.start)
                .max(line.range.start);
            let to = end.saturating_sub(segment.range.start).min(line.range.end);
            if end <= segment.range.start || from >= to {
                continue;
            }
            let (Some(left), Some(right)) = (segment.x(from), segment.x(to)) else {
                continue;
            };
            rects.push(Bounds::new(
                point(left - line.x0, line.top),
                size(right - left, self.height(line)),
            ));
        }
        Ok(rects)
    }
}

#[cfg(test)]
#[path = "paragraph_fallback/tests.rs"]
mod tests;
