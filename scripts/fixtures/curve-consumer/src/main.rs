use xgpui_curve::geom::{Affine, BezPath, PathEl, Point, Vec2};
use xgpui_curve::stroke::{Cap, Join, StrokeStyle, stroke_outline};
use xgpui_curve::svg_path;

fn main() -> Result<(), xgpui_curve::CurveError> {
    let path: BezPath = svg_path::parse("M0 0 Q3 6 6 0 Z M10 0 L20 0")?;
    let transform = Affine::translate(Vec2::new(10., 20.)) * Affine::scale(2.);
    let transformed = transform * &path;
    assert!(matches!(transformed.elements()[2], PathEl::ClosePath));
    let mut segments = transformed.segments();
    assert_eq!(segments.next().unwrap().eval(0.5), Point::new(16., 26.));
    assert_eq!(segments.next().unwrap().end(), Point::new(10., 20.));
    assert_eq!(
        svg_path::parse(&svg_path::to_svg(&transformed))?,
        transformed
    );

    let line = svg_path::parse("M0 0 L10 0")?;
    let bounds = stroke_outline(&line, &StrokeStyle::new(2.), 0.01).bounding_box();
    for (actual, expected) in [bounds.x0, bounds.y0, bounds.x1, bounds.y1]
        .into_iter()
        .zip([-1., -1., 11., 1.])
    {
        assert!((actual - expected).abs() < 0.01);
    }

    let style = StrokeStyle {
        join: Join::Miter(3.),
        end_cap: Cap::Square,
        dash_pattern: vec![2., 1.],
        dash_offset: 0.5,
        ..StrokeStyle::new(2.)
    };
    assert!(!stroke_outline(&transformed, &style, 0.01).is_empty());
    Ok(())
}
