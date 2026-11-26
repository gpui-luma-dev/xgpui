use crate::PathCurve;
use anyhow::{Error, bail};
use etagere::euclid::{Point2D, Vector2D};
use lyon::geom::Angle;
use lyon::geom::{CubicBezierSegment, QuadraticBezierSegment};
use lyon::math::{Vector, vector};
use lyon::path::Event;
use lyon::path::traits::SvgPathBuilder;
use lyon::path::{ArcFlags, Polygon};
use lyon::tessellation::{
    BuffersBuilder, FillTessellator, FillVertex, StrokeTessellator, StrokeVertex, VertexBuffers,
};
use lyon::tessellation::{LineCap, LineJoin};

pub use lyon::math::Transform;
pub use lyon::tessellation::{FillOptions, FillRule, StrokeOptions};

use crate::{Path, Pixels, Point, point, px};

/// Style of the PathBuilder
pub enum PathStyle {
    /// Stroke style
    Stroke(StrokeOptions),
    /// Fill style
    Fill(FillOptions),
}

/// A [`Path`] builder.
pub struct PathBuilder {
    raw: lyon::path::builder::WithSvg<lyon::path::BuilderImpl>,
    transform: Option<lyon::math::Transform>,
    /// PathStyle of the PathBuilder
    pub style: PathStyle,
    dash_array: Option<Vec<Pixels>>,
}

impl From<lyon::path::Builder> for PathBuilder {
    fn from(builder: lyon::path::Builder) -> Self {
        Self {
            raw: builder.with_svg(),
            ..Default::default()
        }
    }
}

impl From<lyon::path::builder::WithSvg<lyon::path::BuilderImpl>> for PathBuilder {
    fn from(raw: lyon::path::builder::WithSvg<lyon::path::BuilderImpl>) -> Self {
        Self {
            raw,
            ..Default::default()
        }
    }
}

impl From<lyon::math::Point> for Point<Pixels> {
    fn from(p: lyon::math::Point) -> Self {
        point(px(p.x), px(p.y))
    }
}

impl From<Point<Pixels>> for lyon::math::Point {
    fn from(p: Point<Pixels>) -> Self {
        lyon::math::point(p.x.0, p.y.0)
    }
}

impl From<Point<Pixels>> for Vector {
    fn from(p: Point<Pixels>) -> Self {
        vector(p.x.0, p.y.0)
    }
}

impl From<Point<Pixels>> for Point2D<f32, Pixels> {
    fn from(p: Point<Pixels>) -> Self {
        Point2D::new(p.x.0, p.y.0)
    }
}

impl Default for PathBuilder {
    fn default() -> Self {
        Self {
            raw: lyon::path::Path::builder().with_svg(),
            style: PathStyle::Fill(FillOptions::default()),
            transform: None,
            dash_array: None,
        }
    }
}

impl PathBuilder {
    /// Creates a new [`PathBuilder`] to build a Stroke path.
    pub fn stroke(width: Pixels) -> Self {
        Self {
            style: PathStyle::Stroke(StrokeOptions::default().with_line_width(width.0)),
            ..Self::default()
        }
    }

    /// Creates a new [`PathBuilder`] to build a Fill path.
    pub fn fill() -> Self {
        Self::default()
    }

    /// Sets the style of the [`PathBuilder`].
    pub fn with_style(self, style: PathStyle) -> Self {
        Self { style, ..self }
    }

    /// Sets the dash array of the [`PathBuilder`].
    ///
    /// [MDN](https://developer.mozilla.org/en-US/docs/Web/SVG/Reference/Attribute/stroke-dasharray)
    pub fn dash_array(mut self, dash_array: &[Pixels]) -> Self {
        // If an odd number of values is provided, then the list of values is repeated to yield an even number of values.
        // Thus, 5,3,2 is equivalent to 5,3,2,5,3,2.
        let array = if dash_array.len() % 2 == 1 {
            let mut new_dash_array = dash_array.to_vec();
            new_dash_array.extend_from_slice(dash_array);
            new_dash_array
        } else {
            dash_array.to_vec()
        };

        self.dash_array = Some(array);
        self
    }

    /// Move the current point to the given point.
    #[inline]
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.raw.move_to(to.into());
    }

    /// Draw a straight line from the current point to the given point.
    #[inline]
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.raw.line_to(to.into());
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    #[inline]
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.raw.quadratic_bezier_to(ctrl.into(), to.into());
    }

    /// Adds a cubic Bézier to the [`Path`] given its two control points
    /// and its end point.
    #[inline]
    pub fn cubic_bezier_to(
        &mut self,
        to: Point<Pixels>,
        control_a: Point<Pixels>,
        control_b: Point<Pixels>,
    ) {
        self.raw
            .cubic_bezier_to(control_a.into(), control_b.into(), to.into());
    }

    /// Adds an elliptical arc.
    pub fn arc_to(
        &mut self,
        radii: Point<Pixels>,
        x_rotation: Pixels,
        large_arc: bool,
        sweep: bool,
        to: Point<Pixels>,
    ) {
        self.raw.arc_to(
            radii.into(),
            Angle::degrees(x_rotation.into()),
            ArcFlags { large_arc, sweep },
            to.into(),
        );
    }

    /// Equivalent to `arc_to` in relative coordinates.
    pub fn relative_arc_to(
        &mut self,
        radii: Point<Pixels>,
        x_rotation: Pixels,
        large_arc: bool,
        sweep: bool,
        to: Point<Pixels>,
    ) {
        self.raw.relative_arc_to(
            radii.into(),
            Angle::degrees(x_rotation.into()),
            ArcFlags { large_arc, sweep },
            to.into(),
        );
    }

    /// Adds a polygon.
    pub fn add_polygon(&mut self, points: &[Point<Pixels>], closed: bool) {
        let points = points.iter().copied().map(|p| p.into()).collect::<Vec<_>>();
        self.raw.add_polygon(Polygon {
            points: points.as_ref(),
            closed,
        });
    }

    /// Close the current sub-path.
    #[inline]
    pub fn close(&mut self) {
        self.raw.close();
    }

    /// Applies a transform to the path.
    #[inline]
    pub fn transform(&mut self, transform: Transform) {
        self.transform = Some(transform);
    }

    /// Applies a translation to the path.
    #[inline]
    pub fn translate(&mut self, to: Point<Pixels>) {
        if let Some(transform) = self.transform {
            self.transform = Some(transform.then_translate(Vector2D::new(to.x.0, to.y.0)));
        } else {
            self.transform = Some(Transform::translation(to.x.0, to.y.0))
        }
    }

    /// Applies a scale to the path.
    #[inline]
    pub fn scale(&mut self, scale: f32) {
        if let Some(transform) = self.transform {
            self.transform = Some(transform.then_scale(scale, scale));
        } else {
            self.transform = Some(Transform::scale(scale, scale));
        }
    }

    /// Applies a rotation to the path.
    ///
    /// The `angle` is in degrees value in the range 0.0 to 360.0.
    #[inline]
    pub fn rotate(&mut self, angle: f32) {
        let radians = angle.to_radians();
        if let Some(transform) = self.transform {
            self.transform = Some(transform.then_rotate(Angle::radians(radians)));
        } else {
            self.transform = Some(Transform::rotation(Angle::radians(radians)));
        }
    }

    /// Builds into a [`Path`].
    #[inline]
    pub fn build(self) -> Result<Path<Pixels>, Error> {
        let path = if let Some(transform) = self.transform {
            self.raw.build().transformed(&transform)
        } else {
            self.raw.build()
        };

        match self.style {
            PathStyle::Stroke(options) => Self::tessellate_stroke(self.dash_array, &path, &options),
            PathStyle::Fill(options) => {
                let mut tessellated = Self::tessellate_fill(&path, &options)?;
                if let Ok(curves) = Self::fill_outline(&path) {
                    tessellated.set_winding_curves(curves, options.fill_rule == FillRule::EvenOdd);
                }
                Ok(tessellated)
            }
        }
    }

    fn tessellate_fill(
        path: &lyon::path::Path,
        options: &FillOptions,
    ) -> Result<Path<Pixels>, Error> {
        // Will contain the result of the tessellation.
        let mut buf: VertexBuffers<lyon::math::Point, u16> = VertexBuffers::new();
        let mut tessellator = FillTessellator::new();

        // Compute the tessellation.
        tessellator.tessellate_path(
            path,
            options,
            &mut BuffersBuilder::new(&mut buf, |vertex: FillVertex| vertex.position()),
        )?;

        Ok(Self::build_path(buf))
    }

    fn tessellate_stroke(
        dash_array: Option<Vec<Pixels>>,
        path: &lyon::path::Path,
        options: &StrokeOptions,
    ) -> Result<Path<Pixels>, Error> {
        let path = if let Some(dash_array) = dash_array {
            let measurements = lyon::algorithms::measure::PathMeasurements::from_path(path, 0.01);
            let mut sampler = measurements
                .create_sampler(path, lyon::algorithms::measure::SampleType::Normalized);
            let mut builder = lyon::path::Path::builder();

            let total_length = sampler.length();
            let dash_array_len = dash_array.len();
            let mut pos = 0.;
            let mut dash_index = 0;
            while pos < total_length {
                let dash_length = dash_array[dash_index % dash_array_len].0;
                let next_pos = (pos + dash_length).min(total_length);
                if dash_index % 2 == 0 {
                    let start = pos / total_length;
                    let end = next_pos / total_length;
                    sampler.split_range(start..end, &mut builder);
                }
                pos = next_pos;
                dash_index += 1;
            }

            &builder.build()
        } else {
            path
        };

        // Will contain the result of the tessellation.
        let mut buf: VertexBuffers<lyon::math::Point, u16> = VertexBuffers::new();
        let mut tessellator = StrokeTessellator::new();

        // Compute the tessellation.
        tessellator.tessellate_path(
            path,
            options,
            &mut BuffersBuilder::new(&mut buf, |vertex: StrokeVertex| vertex.position()),
        )?;

        let mut tessellated = Self::build_path(buf);
        if let Ok(curves) = Self::stroke_outline(path, options) {
            tessellated.set_winding_curves(curves, false);
        }
        Ok(tessellated)
    }

    /// Builds a [`Path`] from a [`lyon::tessellation::VertexBuffers`].
    pub fn build_path(buf: VertexBuffers<lyon::math::Point, u16>) -> Path<Pixels> {
        if buf.vertices.is_empty() {
            return Path::new(Point::default());
        }

        let first_point = buf.vertices[0];

        let mut path = Path::new(first_point.into());
        for i in 0..buf.indices.len() / 3 {
            let i0 = buf.indices[i * 3] as usize;
            let i1 = buf.indices[i * 3 + 1] as usize;
            let i2 = buf.indices[i * 3 + 2] as usize;

            let v0 = buf.vertices[i0];
            let v1 = buf.vertices[i1];
            let v2 = buf.vertices[i2];

            path.push_triangle(
                (v0.into(), v1.into(), v2.into()),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        path
    }

    fn stroke_outline(
        path: &lyon::path::Path,
        options: &StrokeOptions,
    ) -> Result<Vec<PathCurve>, Error> {
        // The legacy tessellator supports different caps at each end
        // and per-vertex widths. Keep those strokes on its coverage path.
        if options.start_cap != options.end_cap || options.variable_line_width.is_some() {
            return Ok(Vec::new());
        }
        let outline = skia_path(path).and_then(|skia_path| {
            skia_path.stroke(
                &tiny_skia_path::Stroke {
                    width: options.line_width,
                    miter_limit: options.miter_limit,
                    line_cap: match options.start_cap {
                        LineCap::Butt => tiny_skia_path::LineCap::Butt,
                        LineCap::Square => tiny_skia_path::LineCap::Square,
                        LineCap::Round => tiny_skia_path::LineCap::Round,
                    },
                    line_join: match options.line_join {
                        LineJoin::Miter => tiny_skia_path::LineJoin::Miter,
                        LineJoin::MiterClip => tiny_skia_path::LineJoin::MiterClip,
                        LineJoin::Round => tiny_skia_path::LineJoin::Round,
                        LineJoin::Bevel => tiny_skia_path::LineJoin::Bevel,
                    },
                    dash: None,
                },
                STROKE_RESOLUTION_SCALE,
            )
        });
        let Some(outline) = outline else {
            return Ok(Vec::new());
        };

        let mut output = Outline::default();
        let at = |p: tiny_skia_path::Point| point(p.x, p.y);
        for segment in outline.segments() {
            match segment {
                tiny_skia_path::PathSegment::MoveTo(p) => output.move_to(at(p)),
                tiny_skia_path::PathSegment::LineTo(p) => output.line_to(at(p)),
                tiny_skia_path::PathSegment::QuadTo(ctrl, p) => output.curve_to(at(p), at(ctrl)),
                tiny_skia_path::PathSegment::CubicTo(ctrl1, ctrl2, p) => {
                    output.cubic_to(at(p), at(ctrl1), at(ctrl2))
                }
                tiny_skia_path::PathSegment::Close => output.close(),
            }
        }
        output.into_curves()
    }

    /// Convert a lyon path into a fill, closing every subpath.
    fn fill_outline(path: &lyon::path::Path) -> Result<Vec<PathCurve>, Error> {
        let mut output = Outline::default();
        let at = |p: lyon::path::math::Point| point(p.x, p.y);
        for event in path.iter() {
            match event {
                Event::Begin { at: p } => output.move_to(at(p)),
                Event::Line { to, .. } => output.line_to(at(to)),
                Event::Quadratic { ctrl, to, .. } => output.curve_to(at(to), at(ctrl)),
                Event::Cubic {
                    ctrl1, ctrl2, to, ..
                } => output.cubic_to(at(to), at(ctrl1), at(ctrl2)),
                Event::End { .. } => output.close(),
            }
        }
        output.into_curves()
    }
}

/// A path's contours as monotone curves, collected by [`PathBuilder`].
#[derive(Default)]
struct Outline {
    curves: Vec<PathCurve>,
    start: Point<f32>,
    current: Point<f32>,
    non_finite: bool,
}

impl Outline {
    /// Whether segments through these points can be added.
    fn accepts(&mut self, points: &[Point<f32>]) -> bool {
        self.non_finite |= !points
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite());
        !self.non_finite
    }

    /// Close the current contour and start a new one at the given point.
    fn move_to(&mut self, to: Point<f32>) {
        if !self.accepts(&[to]) {
            return;
        }
        self.close();
        self.start = to;
        self.current = to;
    }

    fn line_to(&mut self, to: Point<f32>) {
        if self.accepts(&[to]) && to != self.current {
            self.curves.push(PathCurve::line(self.current, to));
            self.current = to;
        }
    }

    fn curve_to(&mut self, to: Point<f32>, ctrl: Point<f32>) {
        if !self.accepts(&[to, ctrl]) || (to == self.current && ctrl == self.current) {
            return;
        }
        PathCurve::push_monotone(&mut self.curves, self.current, ctrl, to);
        self.current = to;
    }

    fn cubic_to(&mut self, to: Point<f32>, ctrl1: Point<f32>, ctrl2: Point<f32>) {
        /// Maximum error, in logical pixels.
        const TOLERANCE: f32 = 0.25;
        if !self.accepts(&[to, ctrl1, ctrl2]) {
            return;
        }
        let lyon_point = |p: Point<f32>| lyon::path::math::point(p.x, p.y);
        let cubic = CubicBezierSegment {
            from: lyon_point(self.current),
            ctrl1: lyon_point(ctrl1),
            ctrl2: lyon_point(ctrl2),
            to: lyon_point(to),
        };
        cubic.for_each_quadratic_bezier(TOLERANCE, &mut |quadratic| {
            self.curve_to(
                point(quadratic.to.x, quadratic.to.y),
                point(quadratic.ctrl.x, quadratic.ctrl.y),
            );
        });
    }

    /// Close the current contour with a straight line back to its start.
    fn close(&mut self) {
        let start = self.start;
        self.line_to(start);
    }

    /// The curves, with every contour closed.
    fn into_curves(mut self) -> Result<Vec<PathCurve>, Error> {
        if self.non_finite {
            bail!("path has a non-finite point");
        }
        self.close();
        Ok(self.curves)
    }
}

impl PathCurve {
    fn new(p0: Point<f32>, ctrl: Point<f32>, p1: Point<f32>, sign: f32) -> Self {
        Self {
            p0,
            p1,
            a: point(p0.x - 2.0 * ctrl.x + p1.x, p0.y - 2.0 * ctrl.y + p1.y),
            b: point(2.0 * (ctrl.x - p0.x), 2.0 * (ctrl.y - p0.y)),
            sign,
            pad: 0,
        }
    }

    /// A line, with exactly zero quadratic terms.
    fn line(from: Point<f32>, to: Point<f32>) -> Self {
        let (p0, p1, sign) = downward(from, to);
        Self {
            p0,
            p1,
            a: point(0.0, 0.0),
            b: point(p1.x - p0.x, p1.y - p0.y),
            sign,
            pad: 0,
        }
    }

    /// Split a quadratic segment at its extrema into xy-monotone curves.
    fn push_monotone(curves: &mut Vec<Self>, p0: Point<f32>, ctrl: Point<f32>, p1: Point<f32>) {
        let curve = QuadraticBezierSegment {
            from: lyon::path::math::point(p0.x, p0.y),
            ctrl: lyon::path::math::point(ctrl.x, ctrl.y),
            to: lyon::path::math::point(p1.x, p1.y),
        };
        // Unlike `split_range`, this also clamps each piece's control point
        // back into its endpoint box when rounding pushes it out.
        curve.for_each_monotonic(&mut |monotone| {
            let from = point(monotone.from.x, monotone.from.y);
            let to = point(monotone.to.x, monotone.to.y);
            if from == to {
                return;
            }
            // Horizontal curves are kept: upward rays cross them.
            let (p0, p1, sign) = downward(from, to);
            let ctrl = point(
                monotone.ctrl.x.clamp(p0.x.min(p1.x), p0.x.max(p1.x)),
                monotone.ctrl.y.clamp(p0.y, p1.y),
            );
            curves.push(Self::new(p0, ctrl, p1, sign));
        });
    }
}

fn downward(from: Point<f32>, to: Point<f32>) -> (Point<f32>, Point<f32>, f32) {
    if from.y > to.y {
        (to, from, -1.0)
    } else {
        (from, to, 1.0)
    }
}

/// Strokes are expanded in logical pixels. tiny-skia keeps offset curves
/// within `0.25 / STROKE_RESOLUTION_SCALE` of exact, a quarter device pixel at
/// 2x, and flattens curves to within `0.5 / STROKE_RESOLUTION_SCALE` to measure
/// dashes. Round joins and caps ignore it: their arcs are approximated to 0.25
/// logical pixels.
const STROKE_RESOLUTION_SCALE: f32 = 2.0;

/// Returns [`None`] for empty geometry.
fn skia_path(path: &lyon::path::Path) -> Option<tiny_skia_path::Path> {
    let mut builder = tiny_skia_path::PathBuilder::new();
    for event in path.iter() {
        match event {
            Event::Begin { at } => builder.move_to(at.x, at.y),
            Event::Line { to, .. } => builder.line_to(to.x, to.y),
            Event::Quadratic { ctrl, to, .. } => builder.quad_to(ctrl.x, ctrl.y, to.x, to.y),
            Event::Cubic {
                ctrl1, ctrl2, to, ..
            } => builder.cubic_to(ctrl1.x, ctrl1.y, ctrl2.x, ctrl2.y, to.x, to.y),
            Event::End { close, .. } => {
                if close {
                    builder.close();
                }
            }
        }
    }
    builder.finish()
}
