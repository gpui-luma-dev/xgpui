//! Parameter-preserving fitting. Unlike Schneider's geometric fit, this keeps
//! the caller's source parameter and never moves samples along a cubic.
use super::{generate, max_error};
use crate::{
    CurveError,
    error::finite,
    geom::{BezPath, CubicBez, Point, Vec2},
};
use std::ops::Range;

/// Controls a parameter-preserving fit. Coordinates and tolerance share units.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct ParameterizedFitOptions {
    /// Maximum requested distance at the supplied sample parameters. This is
    /// not a bound between samples or on derivatives.
    pub tolerance: f64,
    /// Maximum output cubics (1..=8192). A limited fit is returned with
    /// diagnostics rather than silently claiming to meet tolerance.
    pub max_segments: usize,
}
impl Default for ParameterizedFitOptions {
    fn default() -> Self {
        Self {
            tolerance: 1.,
            max_segments: 1024,
        }
    }
}

/// One fitted cubic and its source interval. The interval's end is included
/// for evaluation; adjacent intervals share their endpoint.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ParameterizedSegment {
    /// Cubic evaluated at u = (parameter - range.start) / (range.end - range.start).
    pub curve: CubicBez,
    /// Strictly increasing source interval.
    pub parameter_range: Range<f64>,
}

/// Quality of a parameter-preserving fit, measured against *all* input samples
/// at their supplied parameters, including stationary/repeated positions.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct FitDiagnostics {
    /// Number of supplied samples.
    pub input_samples: usize,
    /// Number of output cubic segments.
    pub segments: usize,
    /// Maximum paired sample error (not nearest distance to a different loop).
    pub max_sample_error: f64,
    /// Root-mean-square paired sample error.
    pub rms_sample_error: f64,
    /// Whether all paired sample errors meet the requested tolerance.
    pub tolerance_met: bool,
    /// Whether a needed split was prevented by `max_segments`.
    pub segment_limit_reached: bool,
}

/// A fitted path retaining the original source parameter. Getters borrow
/// immutable data so the path and parameter metadata cannot become inconsistent.
#[derive(Clone, Debug)]
pub struct ParameterizedFit {
    path: BezPath,
    segments: Vec<ParameterizedSegment>,
    diagnostics: FitDiagnostics,
}
impl ParameterizedFit {
    /// Cubic path, in source order.
    pub fn path(&self) -> &BezPath {
        &self.path
    }
    /// Source intervals and their corresponding cubics, in path order.
    pub fn segments(&self) -> &[ParameterizedSegment] {
        &self.segments
    }
    /// Sample error and work-budget diagnostics.
    pub fn diagnostics(&self) -> &FitDiagnostics {
        &self.diagnostics
    }
    /// Locate a finite source parameter, clamped to the fitted domain. Returns
    /// (segment index, local cubic u). A shared endpoint uses the preceding segment.
    pub fn segment_at(&self, parameter: f64) -> Option<(usize, f64)> {
        if !parameter.is_finite() {
            return None;
        }
        let i = self
            .segments
            .partition_point(|s| s.parameter_range.end < parameter)
            .min(self.segments.len().checked_sub(1)?);
        let range = &self.segments[i].parameter_range;
        let value = parameter.clamp(range.start, range.end);
        Some((i, (value - range.start) / (range.end - range.start)))
    }
    /// Evaluate the fitted curve at the source parameter, clamped to its domain.
    /// Returns `None` for non-finite input.
    pub fn point_at(&self, parameter: f64) -> Option<Point> {
        let (i, u) = self.segment_at(parameter)?;
        let curve = self.segments[i].curve;
        Some(
            if curve.p0 == curve.p1 && curve.p0 == curve.p2 && curve.p0 == curve.p3 {
                curve.p0
            } else {
                curve.eval(u)
            },
        )
    }
    /// Recover a source parameter from a segment and finite local u (clamped
    /// to 0..=1). Returns `None` for an invalid index or non-finite u.
    pub fn parameter_at(&self, segment: usize, u: f64) -> Option<f64> {
        if !u.is_finite() {
            return None;
        }
        let range = &self.segments.get(segment)?.parameter_range;
        Some(range.start + (range.end - range.start) * u.clamp(0., 1.))
    }
}

/// Fit ordered samples while preserving their strictly increasing source
/// parameters. At least two samples are required; repeated *positions* are
/// allowed. No simplification or geometric reparameterization is performed.
///
/// Splits share an estimated tangent (G1 where nonzero); derivatives and
/// curvature need not be continuous. A segment-budget limit returns a usable
/// fit with `tolerance_met = false` when necessary. Inspect the diagnostics.
/// Neither the sampled tolerance nor the source mapping certifies the unknown
/// continuous function between samples. Derived non-finite geometry is rejected.
pub fn fit_parameterized(
    points: &[Point],
    parameters: &[f64],
    options: ParameterizedFitOptions,
) -> Result<ParameterizedFit, CurveError> {
    if points.len() != parameters.len() {
        return Err(CurveError::Dimensions);
    }
    if points.len() < 2 {
        return Err(CurveError::TooFewPoints {
            required: 2,
            actual: points.len(),
        });
    }
    for p in points {
        finite(&[p.x, p.y])?;
    }
    finite(parameters)?;
    if parameters.windows(2).any(|w| w[1] <= w[0]) {
        return Err(CurveError::NotIncreasing);
    }
    if !(parameters[parameters.len() - 1] - parameters[0]).is_finite() {
        return Err(CurveError::OutOfRange("parameter span"));
    }
    if !options.tolerance.is_finite() || options.tolerance <= 0. {
        return Err(CurveError::OutOfRange("fit tolerance"));
    }
    if !(1..=8192).contains(&options.max_segments) {
        return Err(CurveError::OutOfRange("fit segment limit"));
    }
    let last = points.len() - 1;
    let direction = |first: usize, last: usize, reverse: bool| {
        if reverse {
            (first..last)
                .rev()
                .map(|i| points[i] - points[last])
                .find(|v| v.hypot() > 0.)
        } else {
            (first + 1..=last)
                .map(|i| points[i] - points[first])
                .find(|v| v.hypot() > 0.)
        }
        .map(|v| v.normalize())
        .unwrap_or(Vec2::new(1., 0.))
    };
    let mut work = vec![(0, last, direction(0, last, false), direction(0, last, true))];
    let mut segments = Vec::new();
    let mut limited = false;
    while let Some((first, last, t1, t2)) = work.pop() {
        let piece = &points[first..=last];
        let start = parameters[first];
        let span = parameters[last] - start;
        let u: Vec<_> = parameters[first..=last]
            .iter()
            .map(|p| (*p - start) / span)
            .collect();
        let curve = generate(piece, &u, t1, t2);
        finite(&[
            curve.p0.x, curve.p0.y, curve.p1.x, curve.p1.y, curve.p2.x, curve.p2.y, curve.p3.x,
            curve.p3.y,
        ])?;
        let (error, split) = if piece.iter().all(|p| *p == piece[0]) {
            (0., piece.len() / 2)
        } else {
            max_error(piece, &curve, &u)
        };
        if !error.is_finite() {
            return Err(CurveError::NonFinite);
        }
        let needs_split = error > options.tolerance && piece.len() > 2;
        let can_split = segments.len() + work.len() + 1 < options.max_segments;
        if needs_split && can_split {
            let split = first + split.clamp(1, piece.len() - 2);
            let mut center = points[split - 1] - points[split + 1];
            if center.hypot() <= 1e-300 {
                center = direction(first, split, true);
            }
            center = center.normalize();
            work.push((split, last, -center, t2));
            work.push((first, split, t1, center));
        } else {
            limited |= needs_split;
            segments.push(ParameterizedSegment {
                curve,
                parameter_range: parameters[first]..parameters[last],
            });
        }
    }
    let mut path = BezPath::new();
    path.move_to(points[0]);
    for segment in &segments {
        let b = segment.curve;
        path.curve_to(b.p1, b.p2, b.p3);
    }
    let mut result = ParameterizedFit {
        path,
        segments,
        diagnostics: FitDiagnostics {
            input_samples: points.len(),
            segments: 0,
            max_sample_error: 0.,
            rms_sample_error: 0.,
            tolerance_met: false,
            segment_limit_reached: limited,
        },
    };
    let mut sum_norm = 0.;
    let mut maximum: f64 = 0.;
    for (p, t) in points.iter().zip(parameters) {
        let error = result.point_at(*t).unwrap().distance(*p);
        if !error.is_finite() {
            return Err(CurveError::NonFinite);
        }
        maximum = maximum.max(error);
        sum_norm = f64::hypot(sum_norm, error / (points.len() as f64).sqrt());
    }
    result.diagnostics.segments = result.segments.len();
    result.diagnostics.max_sample_error = maximum;
    result.diagnostics.rms_sample_error = sum_norm;
    result.diagnostics.tolerance_met = maximum <= options.tolerance;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(tolerance: f64, max_segments: usize) -> ParameterizedFitOptions {
        ParameterizedFitOptions {
            tolerance,
            max_segments,
        }
    }
    #[test]
    fn nonuniform_source_parameter_is_not_replaced_by_chord_length() {
        let parameters = [0., 0.01, 0.2, 0.7, 1.];
        let points: Vec<_> = parameters.iter().map(|t| Point::new(t * t, 0.)).collect();
        let fit = fit_parameterized(&points, &parameters, options(1e-4, 64)).unwrap();
        for (point, t) in points.iter().zip(parameters) {
            assert!(fit.point_at(t).unwrap().distance(*point) <= 1e-4);
            let (segment, u) = fit.segment_at(t).unwrap();
            assert!((fit.parameter_at(segment, u).unwrap() - t).abs() < 1e-12);
        }
        assert!(fit.diagnostics().tolerance_met);
    }
    #[test]
    fn repeated_positions_and_loop_crossings_retain_source_order() {
        let parameters: Vec<_> = (0..=400)
            .map(|i| i as f64 / 400. * std::f64::consts::TAU * 2.)
            .collect();
        let points: Vec<_> = parameters
            .iter()
            .map(|t| Point::new(t.sin(), (2. * t).sin()))
            .collect();
        let fit = fit_parameterized(&points, &parameters, options(0.003, 512)).unwrap();
        assert!(fit.diagnostics().tolerance_met);
        for t in [0., std::f64::consts::TAU, std::f64::consts::TAU * 2.] {
            let (i, u) = fit.segment_at(t).unwrap();
            assert!((fit.parameter_at(i, u).unwrap() - t).abs() < 1e-12);
        }
        let constant = fit_parameterized(
            &[Point::new(2., 3.); 4],
            &[0., 0.1, 0.2, 9.],
            options(0.001, 10),
        )
        .unwrap();
        assert_eq!(constant.diagnostics().max_sample_error, 0.);
        assert_eq!(constant.point_at(4.), Some(Point::new(2., 3.)));
        assert!(constant.diagnostics().tolerance_met);
    }
    #[test]
    fn diagnostics_report_paired_error_and_segment_budget() {
        let parameters: Vec<_> = (0..=100).map(|i| i as f64 / 100.).collect();
        let points: Vec<_> = parameters
            .iter()
            .map(|t| Point::new(*t, (t * 20.).sin()))
            .collect();
        let fit = fit_parameterized(&points, &parameters, options(1e-5, 1)).unwrap();
        let errors: Vec<_> = points
            .iter()
            .zip(&parameters)
            .map(|(p, t)| fit.point_at(*t).unwrap().distance(*p))
            .collect();
        let expected_max = errors.iter().copied().fold(0., f64::max);
        let expected_rms = (errors.iter().map(|v| v * v).sum::<f64>() / errors.len() as f64).sqrt();
        let d = fit.diagnostics();
        assert!(d.segment_limit_reached && !d.tolerance_met);
        assert_eq!(d.segments, 1);
        assert_eq!(d.input_samples, 101);
        assert!((d.max_sample_error - expected_max).abs() < 1e-12);
        assert!((d.rms_sample_error - expected_rms).abs() < 1e-12);
        let good = fit_parameterized(&points, &parameters, options(0.001, 512)).unwrap();
        assert!(good.diagnostics().tolerance_met && !good.diagnostics().segment_limit_reached);
    }
    #[test]
    fn validates_input_and_clamps_finite_queries() {
        let p = [Point::ZERO, Point::new(1., 0.)];
        assert!(matches!(
            fit_parameterized(&p, &[0.], options(1., 2)),
            Err(CurveError::Dimensions)
        ));
        for params in [[0., 0.], [1., 0.]] {
            assert!(matches!(
                fit_parameterized(&p, &params, options(1., 2)),
                Err(CurveError::NotIncreasing)
            ));
        }
        assert!(fit_parameterized(&p, &[0., f64::INFINITY], options(1., 2)).is_err());
        assert!(fit_parameterized(&p, &[0., 1.], options(f64::NAN, 2)).is_err());
        assert!(fit_parameterized(&p, &[0., 1.], options(1., 0)).is_err());
        assert!(fit_parameterized(&[], &[], options(1., 2)).is_err());
        assert!(fit_parameterized(&p, &[-f64::MAX, f64::MAX], options(1., 2)).is_err());
        let fit = fit_parameterized(&p, &[2., 4.], options(0.001, 1)).unwrap();
        assert_eq!(fit.point_at(-10.), Some(p[0]));
        assert_eq!(fit.point_at(20.), Some(p[1]));
        assert_eq!(fit.point_at(f64::NAN), None);
        assert_eq!(fit.parameter_at(99, 0.), None);
        assert_eq!(fit.parameter_at(0, f64::INFINITY), None);
        assert!(!fit.diagnostics().segment_limit_reached);
    }
}
