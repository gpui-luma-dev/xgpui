use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use xgpui_curve::{
    CurveError,
    geom::{Affine, BezPath},
    measure::{ContourPolicy, MeasuredPath, PathFrame},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Example {
    Open,
    Closed,
    #[default]
    Disconnected,
}

impl Example {
    fn path(self) -> BezPath {
        let mut path = BezPath::new();
        match self {
            Self::Open => {
                path.move_to((110., 270.));
                path.curve_to((225., 85.), (285., 85.), (408., 215.));
                path.line_to((570., 115.));
                path.line_to((690., 275.));
            }
            Self::Closed => {
                path.move_to((200., 175.));
                path.curve_to((200., 45.), (555., 55.), (590., 145.));
                path.curve_to((680., 345.), (270., 365.), (200., 290.));
                path.close_path();
            }
            Self::Disconnected => {
                path.move_to((110., 130.));
                path.quad_to((235., 205.), (370., 100.));
                path.move_to((240., 275.));
                path.curve_to((370., 155.), (415., 345.), (570., 285.));
                path.line_to((690., 220.));
            }
        }
        path
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Query {
    Distance,
    Count,
    Spacing,
}

#[derive(Clone, Copy)]
pub(super) struct FramesPanel {
    example: Example,
    query: Query,
    policy: ContourPolicy,
    offset: u32,
    count: usize,
    spacing: u32,
}

impl Default for FramesPanel {
    fn default() -> Self {
        Self {
            example: Example::default(),
            query: Query::Spacing,
            policy: ContourPolicy::Continuous,
            offset: 0,
            count: 5,
            spacing: 80,
        }
    }
}

impl FramesPanel {
    fn sample(self, path: &BezPath) -> Result<Vec<PathFrame>, CurveError> {
        let path = MeasuredPath::new(path, 1e-5)?;
        let offset = f64::from(self.offset);
        match self.query {
            Query::Distance => path.frames_at_distance(offset, self.policy),
            Query::Count => path.frames_by_count(self.count, offset, self.policy),
            Query::Spacing => path.frames_by_spacing(f64::from(self.spacing), offset, self.policy),
        }
    }

    fn value(self) -> String {
        match self.query {
            Query::Distance => format!("Distance {}", self.offset),
            Query::Count => format!("Count {}", self.count),
            Query::Spacing => format!("Spacing {}", self.spacing),
        }
    }

    fn adjust(&mut self, up: bool, offset: bool) {
        let sign = if up { 1 } else { -1 };
        if offset || self.query == Query::Distance {
            self.offset = self.offset.saturating_add_signed(10 * sign).min(900);
        } else if self.query == Query::Count {
            self.count = self.count.saturating_add_signed(sign as isize).clamp(1, 20);
        } else {
            self.spacing = self.spacing.saturating_add_signed(10 * sign).clamp(20, 300);
        }
    }
}

pub(super) fn render(lab: &CurveLab, cx: &mut Context<CurveLab>) -> AnyElement {
    let settings = lab.frames;
    let path = settings.example.path();
    let (frames, status) = match settings.sample(&path) {
        Ok(frames) => {
            let status = format!("{} frames", frames.len());
            (frames, status)
        }
        Err(error) => (Vec::new(), error.to_string()),
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(
                    [
                        ("Open", Example::Open),
                        ("Closed", Example::Closed),
                        ("Disconnected", Example::Disconnected),
                    ]
                    .map(|(label, example)| {
                        button(label, settings.example == example).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.frames.example = example;
                                cx.notify();
                            },
                        ))
                    }),
                )
                .children(
                    [
                        ("Distance", Query::Distance),
                        ("Count", Query::Count),
                        ("Spacing", Query::Spacing),
                    ]
                    .map(|(label, query)| {
                        button(label, settings.query == query).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.frames.query = query;
                                cx.notify();
                            },
                        ))
                    }),
                )
                .children(
                    [
                        ("Continuous", ContourPolicy::Continuous),
                        ("Per contour", ContourPolicy::PerContour),
                    ]
                    .map(|(label, policy)| {
                        button(label, settings.policy == policy).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.frames.policy = policy;
                                cx.notify();
                            },
                        ))
                    }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(div().child(settings.value()))
                .children(adjust_buttons(cx, false))
                .when(settings.query != Query::Distance, |el| {
                    el.child(div().child(format!("Start offset {}", settings.offset)))
                        .children(adjust_buttons(cx, true))
                })
                .child(div().text_color(rgb(0x9fb0cc)).child(status)),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .rounded_lg()
                .bg(rgb(0x151d2e))
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let transform = fit(bounds, 800., 400.);
                            stroke(window, &path, transform, 2., rgb(0x8fa3c3));
                            for frame in &frames {
                                let geometry = frame.geometry;
                                let p = transform * geometry.location.point;
                                if let Some(normal) = geometry.normal {
                                    stroke(
                                        window,
                                        &line(p, p + normal * 13.),
                                        Affine::IDENTITY,
                                        2.,
                                        rgb(0xffd166),
                                    );
                                }
                                if let Some(tangent) = geometry.location.tangent {
                                    let tip = p + tangent * 24.;
                                    let back = tip - tangent * 7.;
                                    let wing =
                                        xgpui_curve::geom::Vec2::new(-tangent.y, tangent.x) * 4.;
                                    let mut arrow = line(p, tip);
                                    arrow.move_to(back + wing);
                                    arrow.line_to(tip);
                                    arrow.line_to(back - wing);
                                    stroke(window, &arrow, Affine::IDENTITY, 2., rgb(0x4dd2a8));
                                }
                                handle(window, p, 3., rgb(0xf0f5ff));
                            }
                        },
                    )
                    .size_full(),
                ),
        )
        .into_any_element()
}

fn adjust_buttons(cx: &mut Context<CurveLab>, offset: bool) -> [Stateful<Div>; 2] {
    let labels = if offset {
        ["Offset −", "Offset +"]
    } else {
        ["−", "+"]
    };
    [false, true].map(|up| {
        button(labels[usize::from(up)], false).on_click(cx.listener(move |this, _, _, cx| {
            this.frames.adjust(up, offset);
            cx.notify();
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn contour_frames_panel_uses_each_query_and_contour_policy() {
        let disconnected = Example::Disconnected.path();
        let mut panel = FramesPanel::default();
        let continuous = panel.sample(&disconnected).unwrap();
        panel.policy = ContourPolicy::PerContour;
        let restarted = panel.sample(&disconnected).unwrap();
        let second_start = |frames: &[PathFrame]| {
            frames
                .iter()
                .find(|f| f.geometry.location.contour == 1)
                .unwrap()
                .contour_distance
        };
        assert!(second_start(&continuous) > 0.);
        assert_eq!(second_start(&restarted), 0.);

        panel.query = Query::Distance;
        panel.offset = 40;
        let frames = panel.sample(&disconnected).unwrap();
        assert_eq!(frames.len(), 2);
        assert!(frames.iter().all(|f| f.contour_distance == 40.));

        panel.query = Query::Count;
        panel.offset = 0;
        let open = panel.sample(&Example::Open.path()).unwrap();
        let closed = panel.sample(&Example::Closed.path()).unwrap();
        assert_eq!(open.len(), panel.count);
        assert_eq!(closed.len(), panel.count);
        assert_eq!(open.last().unwrap().geometry.location.t, 1.);
        assert_ne!(
            closed.first().unwrap().geometry.location.point,
            closed.last().unwrap().geometry.location.point
        );
    }
}
