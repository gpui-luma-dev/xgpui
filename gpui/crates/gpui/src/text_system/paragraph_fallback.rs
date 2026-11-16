//! Left-to-right paragraphs built from flat line layout, for text backends
//! without their own paragraph engine ([`crate::ParagraphSupport::LeftToRight`]).
//!
//! Each hard line (split at U+2028) is shaped once with the backend's
//! `layout_line`. Reflow only rewraps those shaped lines: greedy breaks at
//! line-break opportunities, trailing spaces hang at the line end, and a word
//! wider than the line breaks at a cluster boundary.
//!
//! Lines follow logical order. The paragraph's base direction (explicit, or its
//! first strong character for `Auto`) sets each line's start edge: right for a
//! right-to-left base. Language is ignored, and there is no reordering beyond
//! the backend's own line shaping. Within a line, glyphs keep its visual order:
//! native shapers (CoreText, DirectWrite) already reorder right-to-left runs, so
//! every measurement uses where glyphs are, never byte order. A cluster is in a
//! right-to-left run when byte indices fall from left to right around it; its
//! logical start is then its right edge.
use super::LineWrapper;
use crate::{
    Bounds, FontRun, LineLayout, ParagraphPosition, ParagraphRequest, Pixels,
    PlatformParagraphLayout, PlatformParagraphShape, Point, Result, ShapedGlyph, ShapedRun, Size,
    TextAffinity, TextDirection, VisualDirection, point, px, size,
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
        options,
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
    let rtl = match options.direction {
        TextDirection::LeftToRight => false,
        TextDirection::RightToLeft => true,
        // The first strong character; none (digits, empty text) is left to right.
        _ => unicode_bidi::get_base_direction(text) == unicode_bidi::Direction::Rtl,
    };
    Ok(Arc::new(FallbackShape {
        source: text.into(),
        font_size,
        rtl,
        segments: segments.into(),
    }))
}

/// One hard line, shaped once. Offsets are relative to `range.start`.
struct Segment {
    range: Range<usize>,
    layout: LineLayout,
    ascent: Pixels,
    descent: Pixels,
    /// Cluster starts ascending, plus the segment length.
    boundaries: Vec<usize>,
    /// Each cluster's horizontal extent in the unwrapped line, by start ascending.
    clusters: Vec<Cluster>,
    /// The same clusters in visual (left-to-right) order, as indices into `clusters`.
    visual: Vec<usize>,
    /// Soft break opportunities strictly inside the segment, on cluster boundaries.
    breaks: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Cluster {
    start: usize,
    left: Pixels,
    right: Pixels,
    /// In a right-to-left run: its logical start is its right edge.
    rtl: bool,
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

        // Glyph boxes in visual order: each extends to the next glyph to its right.
        let mut glyphs: Vec<(Pixels, usize)> = layout
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter())
            .filter(|glyph| glyph.index < source.len() && source.is_char_boundary(glyph.index))
            .map(|glyph| (glyph.position.x, glyph.index))
            .collect();
        glyphs.sort_by(|a, b| f32::from(a.0).total_cmp(&f32::from(b.0)));
        let mut sequence: Vec<(usize, Pixels, Pixels)> = Vec::new();
        for (ix, &(left, start)) in glyphs.iter().enumerate() {
            let right = glyphs[ix + 1..]
                .iter()
                .map(|(x, _)| *x)
                .find(|&x| x > left)
                .unwrap_or(layout.width)
                .max(left);
            match sequence.last_mut() {
                Some(last) if last.0 == start => last.2 = last.2.max(right),
                _ => sequence.push((start, left, right)),
            }
        }
        let mut clusters: Vec<Cluster> = Vec::new();
        for &(start, left, right) in &sequence {
            match clusters.iter_mut().find(|cluster| cluster.start == start) {
                Some(cluster) => {
                    cluster.left = cluster.left.min(left);
                    cluster.right = cluster.right.max(right);
                }
                None => clusters.push(Cluster {
                    start,
                    left,
                    right,
                    rtl: false,
                }),
            }
        }
        // Byte indices falling left to right mark a right-to-left run.
        for (ix, &(start, ..)) in sequence.iter().enumerate() {
            let falls_after = sequence.get(ix + 1).is_some_and(|next| next.0 < start);
            let falls_before = ix > 0 && sequence[ix - 1].0 > start;
            if (falls_after || falls_before)
                && let Some(cluster) = clusters.iter_mut().find(|cluster| cluster.start == start)
            {
                cluster.rtl = true;
            }
        }
        clusters.sort_by_key(|cluster| cluster.start);
        let mut visual: Vec<usize> = (0..clusters.len()).collect();
        visual
            .sort_by(|&a, &b| f32::from(clusters[a].left).total_cmp(&f32::from(clusters[b].left)));
        let mut boundaries: Vec<usize> = clusters.iter().map(|cluster| cluster.start).collect();
        boundaries.push(source.len());
        boundaries.dedup();
        let breaks = LineWrapper::break_indices(source)
            .filter(|&index| index > 0 && index < source.len())
            .filter(|index| boundaries.binary_search(index).is_ok())
            .collect();
        Self {
            range,
            layout,
            ascent,
            descent,
            boundaries,
            clusters,
            visual,
            breaks,
        }
    }

    fn len(&self) -> usize {
        self.range.len()
    }

    fn is_boundary(&self, offset: usize) -> bool {
        self.boundaries.binary_search(&offset).is_ok()
    }

    /// Clusters starting in `start..end`.
    fn clusters_in(&self, start: usize, end: usize) -> &[Cluster] {
        let from = self
            .clusters
            .partition_point(|cluster| cluster.start < start);
        let to = self.clusters.partition_point(|cluster| cluster.start < end);
        &self.clusters[from..to.max(from)]
    }

    /// Visual extent of the clusters in `start..end`.
    fn extent(&self, start: usize, end: usize) -> Option<(Pixels, Pixels)> {
        self.clusters_in(start, end)
            .iter()
            .fold(None, |extent, cluster| {
                Some(match extent {
                    None => (cluster.left, cluster.right),
                    Some((left, right)) => (left.min(cluster.left), right.max(cluster.right)),
                })
            })
    }

    fn width(&self, start: usize, end: usize) -> Pixels {
        self.extent(start, end)
            .map_or(px(0.0), |(left, right)| right - left)
    }

    /// End of `start..end` without its trailing whitespace, as a cluster boundary.
    fn content_end(&self, text: &str, start: usize, end: usize) -> usize {
        let piece = &text[self.range.start + start..self.range.start + end];
        let trimmed = start + piece.trim_end().len();
        // Snap up to a cluster boundary so the measured width stays defined.
        self.boundaries
            .iter()
            .copied()
            .find(|&index| index >= trimmed)
            .unwrap_or(end)
            .min(end)
    }

    /// Segment x of the caret at `offset` on a line covering `line`. Inside the
    /// line it sits at the logical start of the cluster there; at the line's end,
    /// after its last cluster.
    fn caret_x(&self, offset: usize, line: &Range<usize>) -> Option<Pixels> {
        if offset < line.end
            && let Ok(ix) = self
                .clusters
                .binary_search_by_key(&offset, |cluster| cluster.start)
        {
            let cluster = self.clusters[ix];
            return Some(if cluster.rtl {
                cluster.right
            } else {
                cluster.left
            });
        }
        let last = self.clusters_in(line.start, offset).last()?;
        Some(if last.rtl { last.left } else { last.right })
    }

    /// Greedy line ends (relative offsets) for `width`; the last is `len()`.
    fn wrap(&self, text: &str, width: Pixels) -> Vec<usize> {
        let mut ends = Vec::new();
        let mut start = 0;
        loop {
            let rest_end = self.content_end(text, start, self.len());
            if self.width(start, rest_end) <= width {
                ends.push(self.len());
                return ends;
            }
            let mut end = None;
            for &candidate in self.breaks.iter().filter(|&&b| b > start) {
                let content = self.content_end(text, start, candidate);
                if self.width(start, content) <= width {
                    end = Some(candidate);
                } else {
                    break;
                }
            }
            // A word wider than the line: break at the last cluster that fits,
            // always taking at least one cluster so every line makes progress.
            let end = end.unwrap_or_else(|| {
                let first = self
                    .boundaries
                    .iter()
                    .copied()
                    .find(|&index| index > start)
                    .unwrap_or(self.len());
                self.boundaries
                    .iter()
                    .copied()
                    .filter(|&index| index > start)
                    .take_while(|&index| self.width(start, index) <= width)
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
    /// Right-to-left base direction: lines start at the right edge.
    rtl: bool,
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
                // A line's visible text is flush with the start edge; trailing
                // spaces hang past its end.
                let content = segment.content_end(&self.source, start, end);
                let x0 = match segment
                    .extent(start, content)
                    .or_else(|| segment.extent(start, end))
                {
                    Some((_, right)) if self.rtl => right - width,
                    Some((left, _)) => left,
                    None => px(0.0),
                };
                lines.push(Line {
                    segment: segment_ix,
                    range: start..end,
                    x0,
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
            rtl: self.rtl,
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
            max = max.max(segment.width(0, end));
            let mut start = 0;
            for piece_end in segment.breaks.iter().copied().chain([segment.len()]) {
                let content = segment.content_end(&self.source, start, piece_end);
                min = min.max(segment.width(start, content));
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
    rtl: bool,
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
            segment.is_boundary(offset),
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

    /// Line-local caret x (never left of the line) for a boundary on `line`.
    /// An empty line's caret sits at its start edge.
    fn caret_x(&self, line: &Line, offset: usize) -> Pixels {
        match self.segments[line.segment].caret_x(offset, &line.range) {
            Some(x) => (x - line.x0).max(px(0.0)),
            None if self.rtl => (self.size.width - px(1.0)).max(px(0.0)),
            None => px(0.0),
        }
    }

    /// The position for `offset` shown on `line`.
    fn position_on(&self, line: &Line, offset: usize) -> ParagraphPosition {
        let affinity = if line.soft_end && offset == line.range.end {
            TextAffinity::Upstream
        } else {
            TextAffinity::Downstream
        };
        ParagraphPosition {
            byte_index: self.absolute(line, offset),
            affinity,
        }
    }

    /// Every boundary shown on `line` with its line-local caret x.
    fn carets(&self, line: &Line) -> Vec<(usize, Pixels)> {
        let segment = &self.segments[line.segment];
        segment
            .boundaries
            .iter()
            .copied()
            .filter(|&offset| line.range.start <= offset && offset <= line.range.end)
            .map(|offset| (offset, self.caret_x(line, offset)))
            .collect()
    }

    /// The nearest caret strictly right (or left) of `x` on line `ix`.
    fn step(&self, ix: usize, x: Pixels, direction: VisualDirection) -> Option<usize> {
        let carets = self.carets(&self.lines[ix]);
        let epsilon = px(0.01);
        match direction {
            VisualDirection::Right => carets
                .into_iter()
                .filter(|(_, caret)| *caret > x + epsilon)
                .min_by(|a, b| f32::from(a.1).total_cmp(&f32::from(b.1))),
            VisualDirection::Left => carets
                .into_iter()
                .filter(|(_, caret)| *caret < x - epsilon)
                .max_by(|a, b| f32::from(a.1).total_cmp(&f32::from(b.1))),
        }
        .map(|(offset, _)| offset)
    }

    /// The leftmost (or rightmost) caret on line `ix`.
    fn edge(&self, ix: usize, direction: VisualDirection) -> Option<(usize, Pixels)> {
        let carets = self.carets(&self.lines[ix]).into_iter();
        match direction {
            VisualDirection::Right => {
                carets.min_by(|a, b| f32::from(a.1).total_cmp(&f32::from(b.1)))
            }
            VisualDirection::Left => {
                carets.max_by(|a, b| f32::from(a.1).total_cmp(&f32::from(b.1)))
            }
        }
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
        let offset = self
            .carets(line)
            .into_iter()
            .min_by(|(_, a), (_, b)| {
                f32::from((*a - point.x).abs()).total_cmp(&f32::from((*b - point.x).abs()))
            })
            .map_or(line.range.start, |(offset, _)| offset);
        Ok(self.position_on(line, offset))
    }

    fn caret_bounds(&self, position: ParagraphPosition) -> Result<Bounds<Pixels>> {
        let (line_ix, offset) = self.locate(position)?;
        let line = &self.lines[line_ix];
        Ok(Bounds::new(
            point(self.caret_x(line, offset), line.top),
            size(px(1.0), self.height(line)),
        ))
    }

    fn move_visual(
        &self,
        position: ParagraphPosition,
        direction: VisualDirection,
    ) -> Result<ParagraphPosition> {
        let (line_ix, offset) = self.locate(position)?;
        let x = self.caret_x(&self.lines[line_ix], offset);
        if let Some(next) = self.step(line_ix, x, direction) {
            return Ok(self.position_on(&self.lines[line_ix], next));
        }
        // Off the line's edge: continue onto the neighbouring line, from its
        // near edge. A soft wrap shares one byte between both lines, so a step
        // that would land on the same byte moves one caret further.
        let neighbour = match direction {
            VisualDirection::Right => line_ix + 1,
            VisualDirection::Left => match line_ix.checked_sub(1) {
                Some(ix) => ix,
                None => return Ok(position),
            },
        };
        let Some((edge, edge_x)) = (neighbour < self.lines.len())
            .then(|| self.edge(neighbour, direction))
            .flatten()
        else {
            return Ok(position);
        };
        let line = &self.lines[neighbour];
        if self.absolute(line, edge) == position.byte_index
            && let Some(next) = self.step(neighbour, edge_x, direction)
        {
            return Ok(self.position_on(line, next));
        }
        Ok(self.position_on(line, edge))
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
            // Selected clusters, in visual order, merged where they touch.
            let mut span: Option<(Pixels, Pixels)> = None;
            for &cluster in &segment.visual {
                let cluster = segment.clusters[cluster];
                if !line.range.contains(&cluster.start) {
                    continue;
                }
                if (from..to).contains(&cluster.start) {
                    span = Some(match span {
                        Some((left, _)) => (left, cluster.right),
                        None => (cluster.left, cluster.right),
                    });
                } else if let Some((left, right)) = span.take() {
                    rects.push((line, left, right));
                }
            }
            if let Some((left, right)) = span {
                rects.push((line, left, right));
            }
        }
        Ok(rects
            .into_iter()
            .map(|(line, left, right)| {
                let left = (left - line.x0).max(px(0.0));
                let right = (right - line.x0).max(left);
                Bounds::new(point(left, line.top), size(right - left, self.height(line)))
            })
            .collect())
    }
}

#[cfg(test)]
#[path = "paragraph_fallback/tests.rs"]
mod tests;
