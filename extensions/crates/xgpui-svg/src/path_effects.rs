//! Bounded contour effects before native stroke tessellation.
//! Coordinates, dash lengths, phase and tolerance are logical pixels.
use anyhow::{Result, bail, ensure};
use gpui::{Path, PathBuilder, Pixels, Point, point, px};
use std::ops::Range;

const MAX_SEGMENTS: usize = 8192;
const MAX_SAMPLES: usize = 65536;
const MAX_FRAGMENTS: usize = 16384;

/// Retained vector segment; the start point is the preceding endpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Line(Point<Pixels>),
    Quadratic {
        control: Point<Pixels>,
        to: Point<Pixels>,
    },
    Cubic {
        control_a: Point<Pixels>,
        control_b: Point<Pixels>,
        to: Point<Pixels>,
    },
}
impl Segment {
    fn to(self) -> Point<Pixels> {
        match self {
            Self::Line(to) | Self::Quadratic { to, .. } | Self::Cubic { to, .. } => to,
        }
    }
}
/// One open or closed contour. Input is validated when measuring.
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    start: Point<Pixels>,
    segments: Vec<Segment>,
    closed: bool,
}
impl Contour {
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            start,
            segments: vec![],
            closed: false,
        }
    }
    pub fn line_to(mut self, to: Point<Pixels>) -> Self {
        self.segments.push(Segment::Line(to));
        self
    }
    pub fn quadratic_to(mut self, control: Point<Pixels>, to: Point<Pixels>) -> Self {
        self.segments.push(Segment::Quadratic { control, to });
        self
    }
    pub fn cubic_to(
        mut self,
        control_a: Point<Pixels>,
        control_b: Point<Pixels>,
        to: Point<Pixels>,
    ) -> Self {
        self.segments.push(Segment::Cubic {
            control_a,
            control_b,
            to,
        });
        self
    }
    pub fn close(mut self) -> Self {
        self.closed = true;
        self
    }
    pub fn start(&self) -> Point<Pixels> {
        self.start
    }
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}
/// Trim the total concatenated path length, then dash each retained contour.
/// Dash phase restarts at each retained contour; positive offset advances pattern.
/// Trim is a nonwrapping normalized range, including empty ranges.
#[derive(Clone, Debug)]
pub struct PathEffect {
    trim: Range<f64>,
    dash: Vec<f64>,
    offset: f64,
}
impl Default for PathEffect {
    fn default() -> Self {
        Self {
            trim: 0. ..1.,
            dash: vec![],
            offset: 0.,
        }
    }
}
impl PathEffect {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn trim(mut self, range: Range<f32>) -> Result<Self> {
        ensure!(
            range.start.is_finite()
                && range.end.is_finite()
                && range.start >= 0.
                && range.end <= 1.
                && range.start <= range.end,
            "trim must be an ordered range within 0..1"
        );
        self.trim = range.start as f64..range.end as f64;
        Ok(self)
    }
    /// Empty pattern is solid. Odd patterns repeat. Entries must be strictly
    /// positive and finite (zero-length patterns are rejected to bound work).
    pub fn dash(mut self, pattern: &[Pixels], offset: Pixels) -> Result<Self> {
        ensure!(pattern.len() <= 64, "dash pattern exceeds 64 entries");
        ensure!(f32::from(offset).is_finite(), "nonfinite dash offset");
        ensure!(
            pattern
                .iter()
                .all(|p| f32::from(*p).is_finite() && f32::from(*p) > 0.),
            "dash lengths must be finite and positive"
        );
        self.dash = pattern.iter().map(|p| f32::from(*p) as f64).collect();
        if self.dash.len() % 2 == 1 {
            let copy = self.dash.clone();
            self.dash.extend(copy);
        }
        self.offset = f32::from(offset) as f64;
        Ok(self)
    }
}
#[derive(Clone, Debug)]
struct Curve {
    points: Vec<[f64; 2]>,
}
impl Curve {
    fn split(&self, t: f64) -> (Self, Self) {
        let mut points = self.points.clone();
        let mut left = vec![points[0]];
        let mut right = vec![*points.last().unwrap()];
        for n in (1..points.len()).rev() {
            for i in 0..n {
                points[i] = lerp(points[i], points[i + 1], t);
            }
            left.push(points[0]);
            right.push(points[n - 1]);
        }
        right.reverse();
        (Self { points: left }, Self { points: right })
    }
    fn slice(&self, from: f64, to: f64) -> Self {
        if from == 0. && to == 1. {
            return self.clone();
        }
        let (left, _) = self.split(to);
        if from == 0. {
            left
        } else {
            left.split(from / to).1
        }
    }
    fn segment(&self) -> Segment {
        match self.points.as_slice() {
            [_, to] => Segment::Line(pixel(*to)),
            [_, control, to] => Segment::Quadratic {
                control: pixel(*control),
                to: pixel(*to),
            },
            [_, a, b, to] => Segment::Cubic {
                control_a: pixel(*a),
                control_b: pixel(*b),
                to: pixel(*to),
            },
            _ => unreachable!(),
        }
    }
}
fn raw(p: Point<Pixels>) -> [f64; 2] {
    [f32::from(p.x) as f64, f32::from(p.y) as f64]
}
fn pixel(p: [f64; 2]) -> Point<Pixels> {
    point(px(p[0] as f32), px(p[1] as f32))
}
fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
#[derive(Clone, Debug)]
struct MeasuredCurve {
    curve: Curve,
    samples: Vec<(f64, f64)>,
    length: f64,
    end: f64,
}
impl MeasuredCurve {
    fn t_at(&self, length: f64) -> f64 {
        if length <= 0. {
            return 0.;
        }
        if length >= self.length {
            return 1.;
        }
        let i = self.samples.partition_point(|(_, l)| *l < length).max(1);
        let (ta, la) = self.samples[i - 1];
        let (tb, lb) = self.samples[i];
        if lb == la {
            tb
        } else {
            ta + (tb - ta) * (length - la) / (lb - la)
        }
    }
}
#[derive(Clone, Debug)]
struct MeasuredContour {
    start: Point<Pixels>,
    curves: Vec<MeasuredCurve>,
    length: f64,
    closed: bool,
}
/// Reusable arc-length tables. Curve slicing retains Bézier verbs using de
/// Casteljau subdivision; only length lookup is approximated. Reuse while animating.
#[derive(Clone, Debug)]
pub struct MeasuredPath {
    contours: Vec<MeasuredContour>,
    length: f64,
}
impl MeasuredPath {
    /// Per-leaf positional tolerance for adaptive arc-length lookup. This is not
    /// a global arc-length error bound. Range: 0.001..16 logical pixels.
    pub fn new(contours: &[Contour], tolerance: Pixels) -> Result<Self> {
        let tolerance = f32::from(tolerance) as f64;
        ensure!(
            tolerance.is_finite() && (0.001..=16.).contains(&tolerance),
            "tolerance must be 0.001..16 pixels"
        );
        ensure!(contours.len() <= MAX_SEGMENTS, "too many contours");
        let count = contours
            .iter()
            .try_fold(0usize, |n, c| {
                n.checked_add(c.segments.len() + usize::from(c.closed))
            })
            .unwrap_or(usize::MAX);
        ensure!(
            count <= MAX_SEGMENTS,
            "path exceeds 8192 segments including closure"
        );
        let mut measured = vec![];
        let mut total = 0.;
        let mut samples = 0;
        for contour in contours {
            validate(raw(contour.start))?;
            let mut from = contour.start;
            let mut curves = vec![];
            let mut length = 0.;
            let closure = if contour.closed {
                Some(Segment::Line(contour.start))
            } else {
                None
            };
            for segment in contour.segments.iter().copied().chain(closure) {
                let points = match segment {
                    Segment::Line(to) => vec![raw(from), raw(to)],
                    Segment::Quadratic { control, to } => vec![raw(from), raw(control), raw(to)],
                    Segment::Cubic {
                        control_a,
                        control_b,
                        to,
                    } => vec![raw(from), raw(control_a), raw(control_b), raw(to)],
                };
                for p in &points {
                    validate(*p)?;
                }
                let curve = Curve { points };
                samples += 1;
                ensure!(
                    samples <= MAX_SAMPLES,
                    "arc-length table exceeds 65536 samples"
                );
                let mut table = vec![(0., 0.)];
                measure(&curve, 0., 1., tolerance, 0, &mut table, &mut samples)?;
                let curve_length = table.last().unwrap().1;
                length += curve_length;
                curves.push(MeasuredCurve {
                    curve,
                    samples: table,
                    length: curve_length,
                    end: length,
                });
                from = segment.to();
            }
            total += length;
            measured.push(MeasuredContour {
                start: contour.start,
                curves,
                length,
                closed: contour.closed,
            });
        }
        Ok(Self {
            contours: measured,
            length: total,
        })
    }
    pub fn length(&self) -> Pixels {
        px(self.length as f32)
    }
    /// Retain native curve segments for downstream tessellation or inspection.
    pub fn apply(&self, effect: &PathEffect) -> Result<Vec<Contour>> {
        let begin = self.length * effect.trim.start;
        let end = self.length * effect.trim.end;
        if begin >= end {
            return Ok(vec![]);
        }
        let mut output = vec![];
        let mut base = 0.;
        let mut pieces = 0;
        for contour in &self.contours {
            let from = if effect.trim.start == 0. {
                0.
            } else {
                (begin - base).max(0.)
            };
            let to = if effect.trim.end == 1. {
                contour.length
            } else {
                (end - base).min(contour.length)
            };
            base += contour.length;
            if from >= to {
                continue;
            }
            if effect.dash.is_empty() {
                output.push(slice_contour(contour, from, to, &mut pieces)?);
            } else {
                let period: f64 = effect.dash.iter().sum();
                let mut offset = effect.offset.rem_euclid(period);
                let mut i = 0;
                while offset >= effect.dash[i] {
                    offset -= effect.dash[i];
                    i = (i + 1) % effect.dash.len();
                }
                let mut remaining = effect.dash[i] - offset;
                let mut at = from;
                let start_index = output.len();
                let mut steps = 0;
                while at < to {
                    steps += 1;
                    ensure!(
                        steps <= MAX_FRAGMENTS * 2,
                        "dash pattern exceeds work limit"
                    );
                    let next = (at + remaining).min(to);
                    ensure!(next > at, "dash length is below numeric resolution");
                    if i % 2 == 0 {
                        output.push(slice_contour(contour, at, next, &mut pieces)?);
                    }
                    at = next;
                    i = (i + 1) % effect.dash.len();
                    remaining = effect.dash[i];
                }
                // Join visible pieces across an untrimmed closed seam so the
                // tessellator emits a join there instead of two stroke caps.
                let n = output.len() - start_index;
                if contour.closed && from == 0. && to == contour.length && n > 0 {
                    let seam = contour.start;
                    let first_at_seam = output[start_index].start == seam;
                    let last_at_seam = output
                        .last()
                        .unwrap()
                        .segments
                        .last()
                        .is_some_and(|s| s.to() == seam);
                    if first_at_seam && last_at_seam {
                        if n == 1 {
                            output[start_index].closed = true;
                        } else {
                            let first = output.remove(start_index);
                            output.last_mut().unwrap().segments.extend(first.segments);
                        }
                    }
                }
            }
            ensure!(output.len() <= MAX_FRAGMENTS, "path exceeds fragment limit");
        }
        Ok(output)
    }
    /// Stroke effected contours using the existing GPUI tessellator. Empty trim
    /// produces an empty native path. Width must be finite and positive.
    pub fn stroke(&self, effect: &PathEffect, width: Pixels) -> Result<Path<Pixels>> {
        ensure!(
            f32::from(width).is_finite() && f32::from(width) > 0. && f32::from(width) <= 4096.,
            "stroke width must be 0..4096 pixels"
        );
        let mut builder = PathBuilder::stroke(width);
        for contour in self.apply(effect)? {
            builder.move_to(contour.start);
            for segment in contour.segments {
                match segment {
                    Segment::Line(to) => builder.line_to(to),
                    Segment::Quadratic { control, to } => builder.curve_to(to, control),
                    Segment::Cubic {
                        control_a,
                        control_b,
                        to,
                    } => builder.cubic_bezier_to(to, control_a, control_b),
                }
            }
            if contour.closed {
                builder.close();
            }
        }
        builder.build()
    }
}
fn validate(p: [f64; 2]) -> Result<()> {
    ensure!(
        p.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.),
        "coordinates must be finite within ±1000000 pixels"
    );
    Ok(())
}
fn measure(
    curve: &Curve,
    ta: f64,
    tb: f64,
    tolerance: f64,
    depth: usize,
    table: &mut Vec<(f64, f64)>,
    count: &mut usize,
) -> Result<()> {
    let start = curve.points[0];
    let end = *curve.points.last().unwrap();
    let degree = curve.points.len() - 1;
    // Compare control points with a uniformly parameterized line. Unlike a
    // chord-flatness test alone this also refines collinear nonlinear speed.
    let flat = curve
        .points
        .iter()
        .enumerate()
        .all(|(i, p)| distance(*p, lerp(start, end, i as f64 / degree as f64)) <= tolerance);
    if flat {
        *count += 1;
        ensure!(
            *count <= MAX_SAMPLES,
            "arc-length table exceeds 65536 samples"
        );
        table.push((tb, table.last().unwrap().1 + distance(start, end)));
        return Ok(());
    }
    if depth >= 24 {
        bail!("curve subdivision exceeds depth limit");
    }
    let (left, right) = curve.split(0.5);
    let mid = (ta + tb) * 0.5;
    measure(&left, ta, mid, tolerance, depth + 1, table, count)?;
    measure(&right, mid, tb, tolerance, depth + 1, table, count)
}
fn slice_contour(
    contour: &MeasuredContour,
    from: f64,
    to: f64,
    pieces: &mut usize,
) -> Result<Contour> {
    let mut result = None;
    let start = contour.curves.partition_point(|curve| curve.end <= from);
    let mut base = if start == 0 {
        0.
    } else {
        contour.curves[start - 1].end
    };
    for measured in &contour.curves[start..] {
        if base >= to {
            break;
        }
        let a = (from - base).max(0.);
        let b = (to - base).min(measured.length);
        base += measured.length;
        if a >= b {
            continue;
        }
        *pieces += 1;
        ensure!(
            *pieces <= MAX_FRAGMENTS,
            "effect exceeds 16384 curve pieces"
        );
        let curve = measured.curve.slice(measured.t_at(a), measured.t_at(b));
        let out = result.get_or_insert_with(|| Contour::new(pixel(curve.points[0])));
        out.segments.push(curve.segment());
    }
    let mut result = result.unwrap_or_else(|| Contour::new(contour.start));
    result.closed = contour.closed && from == 0. && to == contour.length;
    Ok(result)
}

#[cfg(test)]
mod tests;
