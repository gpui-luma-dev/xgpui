//! Owned geometry values and private adapters to the numerical backend.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Range, Sub};

use kurbo::{ParamCurve as _, ParamCurveArclen as _, ParamCurveNearest as _, Shape as _};

/// A position in two-dimensional space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal component.
    pub x: f64,
    /// Vertical component.
    pub y: f64,
}

impl Point {
    /// Constructs a value from its coordinates.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub(crate) fn to_backend(self) -> kurbo::Point {
        kurbo::Point::new(self.x, self.y)
    }

    pub(crate) fn from_backend(value: kurbo::Point) -> Self {
        Self::new(value.x, value.y)
    }
}

/// A two-dimensional displacement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    /// Horizontal component.
    pub x: f64,
    /// Vertical component.
    pub y: f64,
}

impl Vec2 {
    /// Constructs a value from its coordinates.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn to_backend(self) -> kurbo::Vec2 {
        kurbo::Vec2::new(self.x, self.y)
    }

    fn from_backend(value: kurbo::Vec2) -> Self {
        Self::new(value.x, value.y)
    }
}

/// An axis-aligned rectangle represented by two corners.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// First corner's x coordinate.
    pub x0: f64,
    /// First corner's y coordinate.
    pub y0: f64,
    /// Second corner's x coordinate.
    pub x1: f64,
    /// Second corner's y coordinate.
    pub y1: f64,
}

impl Rect {
    /// Constructs a value from its coordinates.
    pub const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub(crate) fn to_backend(self) -> kurbo::Rect {
        kurbo::Rect::new(self.x0, self.y0, self.x1, self.y1)
    }

    fn from_backend(value: kurbo::Rect) -> Self {
        Self::new(value.x0, value.y0, value.x1, value.y1)
    }
}

/// A straight segment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Line {
    /// Start point.
    pub p0: Point,
    /// End point.
    pub p1: Point,
}

impl Line {
    /// Constructs a value from its points.
    pub fn new(p0: impl Into<Point>, p1: impl Into<Point>) -> Self {
        Self {
            p0: p0.into(),
            p1: p1.into(),
        }
    }

    fn to_backend(self) -> kurbo::Line {
        kurbo::Line::new(self.p0.to_backend(), self.p1.to_backend())
    }

    fn from_backend(value: kurbo::Line) -> Self {
        Self::new(Point::from_backend(value.p0), Point::from_backend(value.p1))
    }
}

/// A quadratic Bézier segment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadBez {
    /// Start point.
    pub p0: Point,
    /// Control point.
    pub p1: Point,
    /// End point.
    pub p2: Point,
}

impl QuadBez {
    /// Constructs a value from its points.
    pub fn new(p0: impl Into<Point>, p1: impl Into<Point>, p2: impl Into<Point>) -> Self {
        Self {
            p0: p0.into(),
            p1: p1.into(),
            p2: p2.into(),
        }
    }

    fn to_backend(self) -> kurbo::QuadBez {
        kurbo::QuadBez::new(
            self.p0.to_backend(),
            self.p1.to_backend(),
            self.p2.to_backend(),
        )
    }

    fn from_backend(value: kurbo::QuadBez) -> Self {
        Self::new(
            Point::from_backend(value.p0),
            Point::from_backend(value.p1),
            Point::from_backend(value.p2),
        )
    }
}

/// A cubic Bézier segment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CubicBez {
    /// Start point.
    pub p0: Point,
    /// Control point.
    pub p1: Point,
    /// Control point.
    pub p2: Point,
    /// End point.
    pub p3: Point,
}

impl CubicBez {
    /// Constructs a value from its points.
    pub fn new(
        p0: impl Into<Point>,
        p1: impl Into<Point>,
        p2: impl Into<Point>,
        p3: impl Into<Point>,
    ) -> Self {
        Self {
            p0: p0.into(),
            p1: p1.into(),
            p2: p2.into(),
            p3: p3.into(),
        }
    }

    fn to_backend(self) -> kurbo::CubicBez {
        kurbo::CubicBez::new(
            self.p0.to_backend(),
            self.p1.to_backend(),
            self.p2.to_backend(),
            self.p3.to_backend(),
        )
    }

    fn from_backend(value: kurbo::CubicBez) -> Self {
        Self::new(
            Point::from_backend(value.p0),
            Point::from_backend(value.p1),
            Point::from_backend(value.p2),
            Point::from_backend(value.p3),
        )
    }
}

impl Point {
    /// The zero point.
    pub const ZERO: Self = Self::new(0., 0.);

    /// Linearly interpolates toward another point.
    pub fn lerp(self, other: Self, t: f64) -> Self {
        Self::from_backend(self.to_backend().lerp(other.to_backend(), t))
    }

    /// Returns the Euclidean distance to another point.
    pub fn distance(self, other: Self) -> f64 {
        self.to_backend().distance(other.to_backend())
    }

    /// Returns the midpoint between two points.
    pub fn midpoint(self, other: Self) -> Self {
        Self::from_backend(self.to_backend().midpoint(other.to_backend()))
    }

    /// Returns the displacement from the origin.
    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0., 0.);

    /// Returns the unit vector at the given angle in radians.
    pub fn from_angle(angle: f64) -> Self {
        Self::from_backend(kurbo::Vec2::from_angle(angle))
    }

    /// Returns the vector length.
    pub fn hypot(self) -> f64 {
        self.to_backend().hypot()
    }

    /// Returns the squared vector length.
    pub fn hypot2(self) -> f64 {
        self.to_backend().hypot2()
    }

    /// Returns the angle from the positive x axis, in radians.
    pub fn atan2(self) -> f64 {
        self.to_backend().atan2()
    }

    /// Scales the vector to unit length.
    pub fn normalize(self) -> Self {
        Self::from_backend(self.to_backend().normalize())
    }

    /// Returns the point displaced from the origin by this vector.
    pub fn to_point(self) -> Point {
        Point::new(self.x, self.y)
    }

    /// Returns the dot product.
    pub fn dot(self, other: Self) -> f64 {
        self.to_backend().dot(other.to_backend())
    }

    /// Returns the signed scalar cross product.
    pub fn cross(self, other: Self) -> f64 {
        self.to_backend().cross(other.to_backend())
    }
}

impl Rect {
    /// Normalizes the corner order so both dimensions are nonnegative.
    pub fn abs(self) -> Self {
        Self::from_backend(self.to_backend().abs())
    }

    /// Returns the bounding rectangle of two points.
    pub fn from_points(a: impl Into<Point>, b: impl Into<Point>) -> Self {
        Self::from_backend(kurbo::Rect::from_points(
            a.into().to_backend(),
            b.into().to_backend(),
        ))
    }

    /// Constructs a rectangle from an origin and (width, height).
    pub fn from_origin_size(origin: impl Into<Point>, size: (f64, f64)) -> Self {
        Self::from_backend(kurbo::Rect::from_origin_size(
            origin.into().to_backend(),
            size,
        ))
    }

    /// Returns the rectangle width.
    pub fn width(self) -> f64 {
        self.to_backend().width()
    }

    /// Returns the rectangle height.
    pub fn height(self) -> f64 {
        self.to_backend().height()
    }

    /// Returns the first corner.
    pub fn origin(self) -> Point {
        Point::from_backend(self.to_backend().origin())
    }

    /// Returns the rectangle center.
    pub fn center(self) -> Point {
        Point::from_backend(self.to_backend().center())
    }

    /// Expands each edge by the given distance.
    pub fn inflate(self, x: f64, y: f64) -> Self {
        Self::from_backend(self.to_backend().inflate(x, y))
    }

    /// Expands the rectangle to include a point.
    pub fn union_pt(self, point: Point) -> Self {
        Self::from_backend(self.to_backend().union_pt(point.to_backend()))
    }

    /// Returns the overlap of two rectangles, clamping each dimension to zero.
    pub fn intersect(self, other: Self) -> Self {
        Self::from_backend(self.to_backend().intersect(other.to_backend()))
    }

    /// Checks containment, including the minimum edges and excluding the maximum edges.
    pub fn contains(self, point: Point) -> bool {
        self.to_backend().contains(point.to_backend())
    }
}

impl QuadBez {
    /// Elevates the quadratic to an exact cubic.
    pub fn raise(self) -> CubicBez {
        CubicBez::from_backend(self.to_backend().raise())
    }
}

impl From<(f64, f64)> for Point {
    fn from((x, y): (f64, f64)) -> Self {
        Self::new(x, y)
    }
}

impl From<(f64, f64)> for Vec2 {
    fn from((x, y): (f64, f64)) -> Self {
        Self::new(x, y)
    }
}

impl Add<Vec2> for Point {
    type Output = Point;

    fn add(self, rhs: Vec2) -> Self::Output {
        Point::from_backend(self.to_backend() + rhs.to_backend())
    }
}

impl Sub<Vec2> for Point {
    type Output = Point;

    fn sub(self, rhs: Vec2) -> Self::Output {
        Point::from_backend(self.to_backend() - rhs.to_backend())
    }
}

impl Sub<Point> for Point {
    type Output = Vec2;

    fn sub(self, rhs: Point) -> Self::Output {
        Vec2::from_backend(self.to_backend() - rhs.to_backend())
    }
}

impl Add<Vec2> for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Vec2) -> Self::Output {
        Vec2::from_backend(self.to_backend() + rhs.to_backend())
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub<Vec2> for Vec2 {
    type Output = Vec2;

    fn sub(self, rhs: Vec2) -> Self::Output {
        Vec2::from_backend(self.to_backend() - rhs.to_backend())
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: f64) -> Self::Output {
        Vec2::from_backend(self.to_backend() * rhs)
    }
}

impl Div<f64> for Vec2 {
    type Output = Vec2;

    fn div(self, rhs: f64) -> Self::Output {
        Vec2::from_backend(self.to_backend() / rhs)
    }
}

impl Mul<Vec2> for f64 {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Self::Output {
        rhs * self
    }
}

impl Neg for Vec2 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::from_backend(-self.to_backend())
    }
}

/// A two-dimensional affine transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine([f64; 6]);

impl Affine {
    /// The identity transform.
    pub const IDENTITY: Self = Self::new([1., 0., 0., 1., 0., 0.]);

    /// Constructs a transform from `[a, b, c, d, e, f]`, mapping
    /// `(x, y)` to `(a*x + c*y + e, b*x + d*y + f)`.
    pub const fn new(coefficients: [f64; 6]) -> Self {
        Self(coefficients)
    }

    /// Returns the coefficients in the order accepted by [`Self::new`].
    pub const fn as_coeffs(self) -> [f64; 6] {
        self.0
    }

    /// Returns the inverse. A singular transform produces non-finite coefficients.
    pub fn inverse(self) -> Self {
        Self(self.to_backend().inverse().as_coeffs())
    }

    /// Scales both axes by the same factor.
    pub fn scale(factor: f64) -> Self {
        Self::scale_non_uniform(factor, factor)
    }

    /// Scales the axes independently.
    pub fn scale_non_uniform(x: f64, y: f64) -> Self {
        Self::new([x, 0., 0., y, 0., 0.])
    }

    /// Translates by the given displacement.
    pub fn translate(offset: impl Into<Vec2>) -> Self {
        let offset = offset.into();
        Self::new([1., 0., 0., 1., offset.x, offset.y])
    }

    fn to_backend(self) -> kurbo::Affine {
        kurbo::Affine::new(self.0)
    }
}

/// Multiplies transforms in application order from right to left.
impl Mul for Affine {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self((self.to_backend() * rhs.to_backend()).as_coeffs())
    }
}

impl Mul<Point> for Affine {
    type Output = Point;

    fn mul(self, rhs: Point) -> Self::Output {
        Point::from_backend(self.to_backend() * rhs.to_backend())
    }
}

/// A line, quadratic or cubic Bézier segment with its start point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSeg {
    /// A straight segment.
    Line(Line),
    /// A quadratic segment.
    Quad(QuadBez),
    /// A cubic segment.
    Cubic(CubicBez),
}

impl PathSeg {
    fn to_backend(self) -> kurbo::PathSeg {
        match self {
            Self::Line(line) => kurbo::PathSeg::Line(line.to_backend()),
            Self::Quad(quad) => kurbo::PathSeg::Quad(quad.to_backend()),
            Self::Cubic(cubic) => kurbo::PathSeg::Cubic(cubic.to_backend()),
        }
    }

    fn from_backend(segment: kurbo::PathSeg) -> Self {
        match segment {
            kurbo::PathSeg::Line(line) => Self::Line(Line::from_backend(line)),
            kurbo::PathSeg::Quad(quad) => Self::Quad(QuadBez::from_backend(quad)),
            kurbo::PathSeg::Cubic(cubic) => Self::Cubic(CubicBez::from_backend(cubic)),
        }
    }

    pub(crate) fn nearest(self, point: Point, accuracy: f64) -> kurbo::Nearest {
        self.to_backend().nearest(point.to_backend(), accuracy)
    }

    pub(crate) fn intersect_line(
        self,
        line: Line,
    ) -> impl IntoIterator<Item = kurbo::LineIntersection> {
        self.to_backend().intersect_line(line.to_backend())
    }

    /// Returns the same segment with the direction reversed.
    pub fn reverse(self) -> Self {
        Self::from_backend(self.to_backend().reverse())
    }

    /// Returns the segment's tight axis-aligned bounds.
    pub fn bounding_box(self) -> Rect {
        Rect::from_backend(self.to_backend().bounding_box())
    }
}

impl From<Line> for PathSeg {
    fn from(segment: Line) -> Self {
        Self::Line(segment)
    }
}

impl Line {
    /// Returns the segment length.
    pub fn length(self) -> f64 {
        self.to_backend().length()
    }

    /// Evaluates the segment at parameter `t`.
    pub fn eval(self, t: f64) -> Point {
        Point::from_backend(self.to_backend().eval(t))
    }
}

impl From<QuadBez> for PathSeg {
    fn from(segment: QuadBez) -> Self {
        Self::Quad(segment)
    }
}

impl QuadBez {
    /// Evaluates the segment at parameter `t`.
    pub fn eval(self, t: f64) -> Point {
        Point::from_backend(self.to_backend().eval(t))
    }

    /// Reparameterizes a portion of the quadratic to the unit interval.
    pub fn subsegment(self, range: Range<f64>) -> Self {
        Self::from_backend(self.to_backend().subsegment(range))
    }
}

impl From<CubicBez> for PathSeg {
    fn from(segment: CubicBez) -> Self {
        Self::Cubic(segment)
    }
}

impl CubicBez {
    /// Evaluates the segment at parameter `t`.
    pub fn eval(self, t: f64) -> Point {
        Point::from_backend(self.to_backend().eval(t))
    }

    /// Reparameterizes a portion of the cubic to the unit interval.
    pub fn subsegment(self, range: Range<f64>) -> Self {
        Self::from_backend(self.to_backend().subsegment(range))
    }
}

impl PathSeg {
    /// Evaluates the segment at parameter `t`.
    pub fn eval(self, t: f64) -> Point {
        Point::from_backend(self.to_backend().eval(t))
    }

    /// Returns the start point.
    pub fn start(self) -> Point {
        Point::from_backend(self.to_backend().start())
    }

    /// Returns the end point.
    pub fn end(self) -> Point {
        Point::from_backend(self.to_backend().end())
    }

    /// Reparameterizes a portion of the segment to the unit interval.
    pub fn subsegment(self, range: Range<f64>) -> Self {
        Self::from_backend(self.to_backend().subsegment(range))
    }

    /// Computes arc length with the given absolute accuracy goal.
    pub fn arclen(self, accuracy: f64) -> f64 {
        self.to_backend().arclen(accuracy)
    }
}

/// A command in a Bézier path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathEl {
    /// Starts a subpath at a point.
    MoveTo(Point),
    /// Draws a line to a point.
    LineTo(Point),
    /// Draws a quadratic with one control point and an endpoint.
    QuadTo(Point, Point),
    /// Draws a cubic with two control points and an endpoint.
    CurveTo(Point, Point, Point),
    /// Closes the current subpath.
    ClosePath,
}

impl PathEl {
    /// Returns the command's explicit endpoint, if it has one.
    pub fn end_point(self) -> Option<Point> {
        self.to_backend().end_point().map(Point::from_backend)
    }

    pub(crate) fn to_backend(self) -> kurbo::PathEl {
        match self {
            Self::MoveTo(p) => kurbo::PathEl::MoveTo(p.to_backend()),
            Self::LineTo(p) => kurbo::PathEl::LineTo(p.to_backend()),
            Self::QuadTo(a, p) => kurbo::PathEl::QuadTo(a.to_backend(), p.to_backend()),
            Self::CurveTo(a, b, p) => {
                kurbo::PathEl::CurveTo(a.to_backend(), b.to_backend(), p.to_backend())
            }
            Self::ClosePath => kurbo::PathEl::ClosePath,
        }
    }

    pub(crate) fn from_backend(element: kurbo::PathEl) -> Self {
        match element {
            kurbo::PathEl::MoveTo(p) => Self::MoveTo(Point::from_backend(p)),
            kurbo::PathEl::LineTo(p) => Self::LineTo(Point::from_backend(p)),
            kurbo::PathEl::QuadTo(a, p) => {
                Self::QuadTo(Point::from_backend(a), Point::from_backend(p))
            }
            kurbo::PathEl::CurveTo(a, b, p) => Self::CurveTo(
                Point::from_backend(a),
                Point::from_backend(b),
                Point::from_backend(p),
            ),
            kurbo::PathEl::ClosePath => Self::ClosePath,
        }
    }
}

impl Mul<PathEl> for Affine {
    type Output = PathEl;

    fn mul(self, element: PathEl) -> Self::Output {
        PathEl::from_backend(self.to_backend() * element.to_backend())
    }
}

/// A sequence of Bézier path commands stored in drawing order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BezPath(Vec<PathEl>);

impl BezPath {
    /// Creates an empty path.
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Borrows the commands in drawing order.
    pub fn elements(&self) -> &[PathEl] {
        &self.0
    }

    /// Returns whether the path has no explicit line or curve commands.
    pub fn is_empty(&self) -> bool {
        self.iter()
            .all(|el| matches!(el, PathEl::MoveTo(_) | PathEl::ClosePath))
    }

    /// Iterates over commands in drawing order.
    pub fn iter(&self) -> impl Iterator<Item = PathEl> + Clone + '_ {
        self.0.iter().copied()
    }

    /// Iterates over drawable segments, including any closing lines.
    pub fn segments(&self) -> impl Iterator<Item = PathSeg> + Clone + '_ {
        kurbo::segments(self.backend_elements()).map(PathSeg::from_backend)
    }

    /// Appends a path command. The first command must be a move.
    pub fn push(&mut self, element: PathEl) {
        debug_assert!(!self.0.is_empty() || matches!(element, PathEl::MoveTo(_)));
        self.0.push(element);
    }

    /// Starts a subpath.
    pub fn move_to(&mut self, point: impl Into<Point>) {
        self.push(PathEl::MoveTo(point.into()));
    }

    /// Appends a line.
    pub fn line_to(&mut self, point: impl Into<Point>) {
        self.push(PathEl::LineTo(point.into()));
    }

    /// Appends a quadratic.
    pub fn quad_to(&mut self, control: impl Into<Point>, end: impl Into<Point>) {
        self.push(PathEl::QuadTo(control.into(), end.into()));
    }

    /// Appends a cubic.
    pub fn curve_to(
        &mut self,
        first: impl Into<Point>,
        second: impl Into<Point>,
        end: impl Into<Point>,
    ) {
        self.push(PathEl::CurveTo(first.into(), second.into(), end.into()));
    }

    /// Closes the current subpath.
    pub fn close_path(&mut self) {
        self.push(PathEl::ClosePath);
    }

    /// Returns the tight bounds of the path's drawable segments.
    pub fn bounding_box(&self) -> Rect {
        Rect::from_backend(self.to_backend().bounding_box())
    }

    /// Returns the signed area enclosed by the path.
    pub fn area(&self) -> f64 {
        self.to_backend().area()
    }

    /// Transforms the path in place, including every control point.
    pub fn apply_affine(&mut self, transform: Affine) {
        for element in &mut self.0 {
            *element = transform * *element;
        }
    }

    pub(crate) fn backend_elements(&self) -> impl Iterator<Item = kurbo::PathEl> + Clone + '_ {
        self.iter().map(PathEl::to_backend)
    }

    pub(crate) fn to_backend(&self) -> kurbo::BezPath {
        self.backend_elements().collect()
    }

    pub(crate) fn from_backend(elements: impl IntoIterator<Item = kurbo::PathEl>) -> Self {
        elements.into_iter().map(PathEl::from_backend).collect()
    }
}

impl IntoIterator for BezPath {
    type Item = PathEl;
    type IntoIter = std::vec::IntoIter<PathEl>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Extend<PathEl> for BezPath {
    fn extend<T: IntoIterator<Item = PathEl>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl FromIterator<PathEl> for BezPath {
    fn from_iter<T: IntoIterator<Item = PathEl>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Mul<BezPath> for Affine {
    type Output = BezPath;

    fn mul(self, mut path: BezPath) -> Self::Output {
        path.apply_affine(self);
        path
    }
}

impl Mul<&BezPath> for Affine {
    type Output = BezPath;

    fn mul(self, path: &BezPath) -> Self::Output {
        self * path.clone()
    }
}

/// Approximates a circle with a closed Bézier path at the given tolerance.
pub fn circle_path(center: impl Into<Point>, radius: f64, tolerance: f64) -> BezPath {
    BezPath::from_backend(
        kurbo::Circle::new(center.into().to_backend(), radius).path_elements(tolerance),
    )
}

/// Approximates a rounded rectangle with a closed Bézier path at the given tolerance.
pub fn rounded_rect_path(rect: Rect, radius: f64, tolerance: f64) -> BezPath {
    BezPath::from_backend(
        kurbo::RoundedRect::from_rect(rect.to_backend(), radius).path_elements(tolerance),
    )
}

#[cfg(any(feature = "shape", feature = "stroke"))]
pub(crate) fn arc_to_cubics(
    center: Point,
    radius: f64,
    start: f64,
    sweep: f64,
    tolerance: f64,
    mut emit: impl FnMut(Point, Point, Point),
) {
    kurbo::Arc::new(center.to_backend(), (radius, radius), start, sweep, 0.).to_cubic_beziers(
        tolerance,
        |a, b, end| {
            emit(
                Point::from_backend(a),
                Point::from_backend(b),
                Point::from_backend(end),
            );
        },
    );
}
