//! Series gallery: every plot curve on the same samples, with an optional
//! missing sample, area fills and viewport clipping.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use xgpui_curve::kurbo::{Point as KPoint, Rect};
use xgpui_curve::series::{Baseline, SeriesCurve, series_area, series_path, series_path_clipped};

const W: f64 = 300.;
const H: f64 = 150.;

const CURVES: [(&str, SeriesCurve); 12] = [
    ("Linear", SeriesCurve::Linear),
    ("Step", SeriesCurve::Step),
    ("Step before", SeriesCurve::StepBefore),
    ("Step after", SeriesCurve::StepAfter),
    ("Basis", SeriesCurve::Basis),
    ("Cardinal 0.5", SeriesCurve::Cardinal { tension: 0.5 }),
    ("Catmull-Rom α 0.5", SeriesCurve::CatmullRom { alpha: 0.5 }),
    ("Catmull-Rom α 1", SeriesCurve::CatmullRom { alpha: 1. }),
    ("Natural", SeriesCurve::Natural),
    ("Monotone X (PCHIP)", SeriesCurve::MonotoneX),
    ("Bundle β 0.6", SeriesCurve::Bundle { beta: 0.6 }),
    ("Monotone Y", SeriesCurve::MonotoneY),
];

#[derive(Clone, Copy, Default)]
pub struct SeriesPanel {
    gap: bool,
    area: bool,
    clip: bool,
}

fn samples(gap: bool, vertical: bool) -> Vec<KPoint> {
    let ys = [
        118., 70., 92., 40., 46., 108., 120., 64., 22., 30., 84., 70.,
    ];
    let mut out: Vec<KPoint> = ys
        .iter()
        .enumerate()
        .map(|(i, &y)| {
            let x = 14. + i as f64 * (W - 28.) / (ys.len() - 1) as f64;
            if vertical {
                // Monotone in y: a sideways series.
                KPoint::new(
                    60. + (y - 20.) * 1.8,
                    10. + i as f64 * (H - 20.) / (ys.len() - 1) as f64,
                )
            } else {
                KPoint::new(x, y)
            }
        })
        .collect();
    if gap {
        out[6] = KPoint::new(f64::NAN, f64::NAN);
    }
    out
}

pub fn render(lab: &mut CurveLab, _window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let SeriesPanel { gap, area, clip } = lab.series;
    let controls = div()
        .flex()
        .gap_2()
        .child(
            button("Missing sample", gap).on_click(cx.listener(|this, _, _, cx| {
                this.series.gap = !this.series.gap;
                cx.notify();
            })),
        )
        .child(
            button("Area fill", area).on_click(cx.listener(|this, _, _, cx| {
                this.series.area = !this.series.area;
                cx.notify();
            })),
        )
        .child(
            button("Clip to viewport", clip).on_click(cx.listener(|this, _, _, cx| {
                this.series.clip = !this.series.clip;
                cx.notify();
            })),
        );
    let viewport = Rect::new(40., 30., W - 40., H - 30.);
    let cards = CURVES.into_iter().map(move |(label, curve)| {
        let points = samples(gap, curve == SeriesCurve::MonotoneY);
        card(label).child(
            div().w(px(300.)).h(px(150.)).child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let t = fit(bounds, W, H);
                        window.paint_quad(fill(bounds, rgb(0x0c1220)));
                        if area && curve != SeriesCurve::MonotoneY {
                            let fill_path = series_area(&points, curve, Baseline::Constant(H - 8.));
                            filled(window, &fill_path, t, hsla(0.58, 0.7, 0.55, 0.25));
                        }
                        let path = if clip {
                            series_path_clipped(&points, curve, viewport)
                        } else {
                            series_path(&points, curve)
                        };
                        if clip {
                            let mut frame = xgpui_curve::kurbo::BezPath::new();
                            frame.move_to((viewport.x0, viewport.y0));
                            frame.line_to((viewport.x1, viewport.y0));
                            frame.line_to((viewport.x1, viewport.y1));
                            frame.line_to((viewport.x0, viewport.y1));
                            frame.close_path();
                            stroke(window, &frame, t, 1., hsla(0.1, 0.9, 0.6, 0.6));
                        }
                        stroke(window, &path, t, 2., rgb(0x8fd3ff));
                        for p in points.iter().filter(|p| p.x.is_finite()) {
                            dot(window, t * *p, 2.5, rgb(0xffffff));
                        }
                    },
                )
                .size_full(),
            ),
        )
    });
    div()
        .id("series-scroll")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .child(controls)
        .child(div().flex().flex_wrap().gap_3().children(cards))
        .into_any_element()
}
