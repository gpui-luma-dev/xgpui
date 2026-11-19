use crate::{Point, Scene, WorldBounds};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// World-space guide: constant coordinate on axis, start/end on the other axis.
#[derive(Clone, Copy, Debug)]
pub struct AlignmentGuide {
    pub axis: Axis,
    pub coordinate: f64,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct SnapResult {
    pub delta: Point,
    pub x: Option<AlignmentGuide>,
    pub y: Option<AlignmentGuide>,
}

impl<T> Scene<T> {
    /// Snap group edges/centers to nearby stationary item edges/centers.
    /// Tolerance and search radius are world units (divide logical pixels by zoom).
    /// Exclude moving items. Each axis chooses the closest match independently;
    /// equal distances retain back-to-front query order for deterministic results.
    /// Invalid moving bounds, non-finite deltas, or overflowing translations return
    /// zero translation and no guides. Invalid tolerance/radius or an overflowing
    /// search rectangle returns the valid requested translation without snapping.
    pub fn snap_translation(
        &self,
        moving: WorldBounds,
        delta: Point,
        tolerance: f64,
        radius: f64,
        exclude: impl Fn(&T) -> bool,
    ) -> SnapResult {
        let mut result = SnapResult {
            delta,
            x: None,
            y: None,
        };
        if !moving.is_valid()
            || !delta.x.is_finite()
            || !delta.y.is_finite()
            || !moving.translated(delta).is_valid()
        {
            result.delta = Point::default();
            return result;
        }
        if !tolerance.is_finite() || tolerance < 0. || !radius.is_finite() || radius < 0. {
            return result;
        }
        let proposed = moving.translated(delta);
        let search = WorldBounds {
            min: Point::new(proposed.min.x - radius, proposed.min.y - radius),
            max: Point::new(proposed.max.x + radius, proposed.max.y + radius),
        };
        if !search.is_valid() {
            return result;
        }
        let mut x = None;
        let mut y = None;
        for item in self.query(search).filter(|item| !exclude(&item.data)) {
            nearest(
                proposed.min.x,
                proposed.max.x,
                item.bounds.min.x,
                item.bounds.max.x,
                tolerance,
                item.bounds,
                &mut x,
            );
            nearest(
                proposed.min.y,
                proposed.max.y,
                item.bounds.min.y,
                item.bounds.max.y,
                tolerance,
                item.bounds,
                &mut y,
            );
        }
        if let Some((correction, _, _)) = x {
            result.delta.x += correction;
        }
        if let Some((correction, _, _)) = y {
            result.delta.y += correction;
        }
        let snapped = moving.translated(result.delta);
        if !snapped.is_valid() {
            return SnapResult {
                delta,
                x: None,
                y: None,
            };
        }
        result.x = x.map(|(_, coordinate, bounds)| AlignmentGuide {
            axis: Axis::X,
            coordinate,
            start: snapped.min.y.min(bounds.min.y),
            end: snapped.max.y.max(bounds.max.y),
        });
        result.y = y.map(|(_, coordinate, bounds)| AlignmentGuide {
            axis: Axis::Y,
            coordinate,
            start: snapped.min.x.min(bounds.min.x),
            end: snapped.max.x.max(bounds.max.x),
        });
        result
    }
}

fn nearest(
    min: f64,
    max: f64,
    target_min: f64,
    target_max: f64,
    tolerance: f64,
    bounds: WorldBounds,
    best: &mut Option<(f64, f64, WorldBounds)>,
) {
    for source in [min, min * 0.5 + max * 0.5, max] {
        for target in [target_min, target_min * 0.5 + target_max * 0.5, target_max] {
            let correction = target - source;
            if correction.abs() <= tolerance
                && best.is_none_or(|(previous, _, _)| correction.abs() < previous.abs())
            {
                *best = Some((correction, target, bounds));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_or_overflowing_translations_return_zero_without_guides() {
        let valid = WorldBounds {
            min: Point::default(),
            max: Point::new(1., 1.),
        };
        let scene = Scene::new([crate::SceneItem {
            bounds: valid,
            data: (),
        }]);
        for (moving, delta) in [
            (valid, Point::new(f64::NAN, 0.)),
            (valid, Point::new(0., f64::INFINITY)),
            (
                WorldBounds {
                    min: Point::new(2., 0.),
                    max: Point::new(1., 1.),
                },
                Point::default(),
            ),
            (
                WorldBounds {
                    min: Point::new(f64::MAX, 0.),
                    max: Point::new(f64::MAX, 1.),
                },
                Point::new(f64::MAX, 0.),
            ),
        ] {
            let result = scene.snap_translation(moving, delta, 1., 10., |_| false);
            assert_eq!(result.delta, Point::default());
            assert!(result.x.is_none() && result.y.is_none());
        }
    }

    #[test]
    fn overflowing_search_does_not_query_or_generate_guides() {
        let moving = WorldBounds {
            min: Point::new(f64::MAX, 0.),
            max: Point::new(f64::MAX, 1.),
        };
        let scene = Scene::new([crate::SceneItem {
            bounds: moving,
            data: (),
        }]);
        let delta = Point::new(0., 1.);
        let result = scene.snap_translation(moving, delta, 1., f64::MAX, |_| {
            panic!("invalid search queried")
        });
        assert_eq!(result.delta, delta);
        assert!(result.x.is_none() && result.y.is_none());
    }

    #[test]
    fn snapping_excludes_selection_and_uses_nearest_edge_or_center() {
        let moving = WorldBounds {
            min: Point::new(0., 0.),
            max: Point::new(100., 100.),
        };
        let target = WorldBounds {
            min: Point::new(200., 200.),
            max: Point::new(300., 300.),
        };
        let scene = Scene::new([
            crate::SceneItem {
                bounds: moving,
                data: 0,
            },
            crate::SceneItem {
                bounds: target,
                data: 1,
            },
        ]);
        let snap = scene.snap_translation(moving, Point::new(97., 149.), 6., 200., |id| *id == 0);
        assert_eq!(snap.delta, Point::new(100., 150.));
        assert_eq!(snap.x.unwrap().coordinate, 200.);
        assert_eq!(snap.y.unwrap().coordinate, 200.);
        let free = scene.snap_translation(moving, Point::new(90., 140.), 6., 200., |_| false);
        assert_eq!(free.delta, Point::new(90., 140.));
        let excluded = scene.snap_translation(moving, Point::new(97., 149.), 6., 200., |_| true);
        assert!(excluded.x.is_none() && excluded.y.is_none());
    }

    #[test]
    fn caller_can_keep_snap_tolerance_constant_in_screen_pixels() {
        let moving = WorldBounds {
            min: Point::default(),
            max: Point::new(100., 100.),
        };
        let scene = Scene::new([crate::SceneItem {
            bounds: WorldBounds {
                min: Point::new(200., 0.),
                max: Point::new(300., 100.),
            },
            data: (),
        }]);
        for zoom in [0.1, 1., 10.] {
            let snap = scene.snap_translation(
                moving,
                Point::new(100. - 5. / zoom, 0.),
                6. / zoom,
                200. / zoom,
                |_| false,
            );
            assert!((snap.delta.x - 100.).abs() < 1e-8);
            let free = scene.snap_translation(
                moving,
                Point::new(100. - 7. / zoom, 0.),
                6. / zoom,
                200. / zoom,
                |_| false,
            );
            assert!(free.x.is_none());
        }
    }
}
