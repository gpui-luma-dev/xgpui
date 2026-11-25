//! Bounded contour effects before native stroke tessellation.
//! Coordinates, dash lengths, phase and tolerance are logical pixels.
//!
//! Deprecated: use [`xgpui_curve::measure`] directly. Build a
//! `xgpui_curve::geom::BezPath`, measure it with
//! `xgpui_curve::measure::MeasuredPath`, apply an
//! `xgpui_curve::measure::PathEffect`, and stroke the result with
//! `xgpui_curve::gpui_paint::append` into a `PathBuilder::stroke`. This module
//! now delegates to it and remains for one release so callers can migrate.
#![allow(deprecated)]
use anyhow::{Result, ensure};
use gpui::{Path, PathBuilder, Pixels, Point, point, px};
use std::ops::Range;
use xgpui_curve::geom::{BezPath, PathEl, Point as CurvePoint};
use xgpui_curve::measure;

const MAX_SEGMENTS: usize = 8192;

/// Retained vector segment; the start point is the preceding endpoint.
#[deprecated(
    note = "use xgpui_curve::measure::{MeasuredPath, PathEffect} on a xgpui_curve::geom::BezPath, and xgpui_curve::gpui_paint to stroke the result"
)]
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
    #[cfg_attr(not(test), allow(dead_code))]
    fn to(self) -> Point<Pixels> {
        match self {
            Self::Line(to) | Self::Quadratic { to, .. } | Self::Cubic { to, .. } => to,
        }
    }
}
/// One open or closed contour. Input is validated when measuring.
#[deprecated(
    note = "use xgpui_curve::measure::{MeasuredPath, PathEffect} on a xgpui_curve::geom::BezPath, and xgpui_curve::gpui_paint to stroke the result"
)]
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
#[deprecated(
    note = "use xgpui_curve::measure::{MeasuredPath, PathEffect} on a xgpui_curve::geom::BezPath, and xgpui_curve::gpui_paint to stroke the result"
)]
#[derive(Clone, Debug, Default)]
pub struct PathEffect(measure::PathEffect);
impl PathEffect {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn trim(self, range: Range<f32>) -> Result<Self> {
        ensure!(
            range.start.is_finite()
                && range.end.is_finite()
                && range.start >= 0.
                && range.end <= 1.
                && range.start <= range.end,
            "trim must be an ordered range within 0..1"
        );
        Ok(Self(self.0.trim(range.start as f64..range.end as f64)?))
    }
    /// Empty pattern is solid. Odd patterns repeat. Entries must be strictly
    /// positive and finite (zero-length patterns are rejected to bound work).
    pub fn dash(self, pattern: &[Pixels], offset: Pixels) -> Result<Self> {
        let pattern: Vec<f64> = pattern.iter().map(|p| f32::from(*p) as f64).collect();
        Ok(Self(self.0.dash(&pattern, f32::from(offset) as f64)?))
    }
}
/// Reusable arc-length tables. Curve slicing retains Bézier verbs using de
/// Casteljau subdivision. Reuse while animating.
#[deprecated(
    note = "use xgpui_curve::measure::{MeasuredPath, PathEffect} on a xgpui_curve::geom::BezPath, and xgpui_curve::gpui_paint to stroke the result"
)]
#[derive(Clone, Debug)]
pub struct MeasuredPath(measure::MeasuredPath);
impl MeasuredPath {
    /// Absolute arc-length tolerance per measured interval. Range: 0.001..16
    /// logical pixels.
    pub fn new(contours: &[Contour], tolerance: Pixels) -> Result<Self> {
        let tolerance = f32::from(tolerance) as f64;
        ensure!(
            tolerance.is_finite() && (0.001..=16.).contains(&tolerance),
            "tolerance must be 0.001..16 pixels"
        );
        ensure!(contours.len() <= MAX_SEGMENTS, "too many contours");
        let mut path = BezPath::new();
        for contour in contours {
            validate(contour.start)?;
            path.move_to(raw(contour.start));
            for segment in &contour.segments {
                match *segment {
                    Segment::Line(to) => {
                        validate(to)?;
                        path.line_to(raw(to));
                    }
                    Segment::Quadratic { control, to } => {
                        validate(control)?;
                        validate(to)?;
                        path.quad_to(raw(control), raw(to));
                    }
                    Segment::Cubic {
                        control_a,
                        control_b,
                        to,
                    } => {
                        validate(control_a)?;
                        validate(control_b)?;
                        validate(to)?;
                        path.curve_to(raw(control_a), raw(control_b), raw(to));
                    }
                }
            }
            if contour.closed {
                path.close_path();
            }
        }
        Ok(Self(measure::MeasuredPath::new(&path, tolerance)?))
    }
    pub fn length(&self) -> Pixels {
        px(self.0.length() as f32)
    }
    /// Retain native curve segments for downstream tessellation or inspection.
    pub fn apply(&self, effect: &PathEffect) -> Result<Vec<Contour>> {
        Ok(contours(&self.0.apply(&effect.0)?))
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
fn validate(p: Point<Pixels>) -> Result<()> {
    let p = [f32::from(p.x), f32::from(p.y)];
    ensure!(
        p.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.),
        "coordinates must be finite within ±1000000 pixels"
    );
    Ok(())
}
fn raw(p: Point<Pixels>) -> CurvePoint {
    CurvePoint::new(f32::from(p.x) as f64, f32::from(p.y) as f64)
}
fn pixel(p: CurvePoint) -> Point<Pixels> {
    point(px(p.x as f32), px(p.y as f32))
}
/// The effect output as GPUI-pixel contours, one per subpath.
fn contours(path: &BezPath) -> Vec<Contour> {
    let mut out: Vec<Contour> = Vec::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => out.push(Contour::new(pixel(p))),
            PathEl::LineTo(p) => {
                if let Some(c) = out.last_mut() {
                    c.segments.push(Segment::Line(pixel(p)));
                }
            }
            PathEl::QuadTo(c1, p) => {
                if let Some(c) = out.last_mut() {
                    c.segments.push(Segment::Quadratic {
                        control: pixel(c1),
                        to: pixel(p),
                    });
                }
            }
            PathEl::CurveTo(c1, c2, p) => {
                if let Some(c) = out.last_mut() {
                    c.segments.push(Segment::Cubic {
                        control_a: pixel(c1),
                        control_b: pixel(c2),
                        to: pixel(p),
                    });
                }
            }
            PathEl::ClosePath => {
                if let Some(c) = out.last_mut() {
                    c.closed = true;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
