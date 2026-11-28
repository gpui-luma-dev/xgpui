use super::{CurveError, MAX_FRAGMENTS, MeasuredPath, PathGeometry, finite};

/// How frame queries traverse disconnected contours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourPolicy {
    /// Use one distance axis over all positive-length contours, in path order.
    /// At a contour break, a sample selects the start of the next contour.
    Continuous,
    /// Start the query at the same local offset on each positive-length contour.
    /// Count and spacing are applied to each contour independently.
    PerContour,
}

/// A differential frame at a sampled arc distance.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct PathFrame {
    /// Distance from the start of the whole path, across contours in order.
    pub distance: f64,
    /// Distance from the start of the selected contour.
    pub contour_distance: f64,
    /// Point, segment identity, tangent, normal, and curvature.
    pub geometry: PathGeometry,
}

#[derive(Clone, Copy)]
enum Sampling {
    Once,
    Count(usize),
    Spacing(f64),
}

impl MeasuredPath {
    /// Query one frame at `distance`, or one per eligible contour with
    /// [`ContourPolicy::PerContour`]. Distance is a finite, nonnegative start
    /// offset in path coordinates. Offsets outside the sampled range yield no frame.
    ///
    /// Open range ends are included. A closed contour's terminal seam is excluded:
    /// the seam is sampled at its start. At a segment corner, the incoming
    /// segment supplies the frame. Tangent and normal can be absent where
    /// direction is undefined; cusp and stationary-point curvature can be absent.
    /// Zero-length contours are skipped. Count and spacing queries use these rules too.
    pub fn frames_at_distance(
        &self,
        distance: f64,
        policy: ContourPolicy,
    ) -> Result<Vec<PathFrame>, CurveError> {
        self.sample_frames(distance, policy, Sampling::Once)
    }

    /// Distribute `count` frames evenly from `start_offset` to the end of the
    /// sampled range. The count applies to the whole path or to each contour,
    /// according to `policy`. For a closed end, divide by `count` so the
    /// terminal seam is excluded. For an open end, include both ends.
    ///
    /// A count of zero returns no frames. A count of one, or an offset exactly
    /// at an open end, returns the frame at the offset. See
    /// [`Self::frames_at_distance`] for range and frame rules. A query can
    /// return at most 16,384 frames.
    pub fn frames_by_count(
        &self,
        count: usize,
        start_offset: f64,
        policy: ContourPolicy,
    ) -> Result<Vec<PathFrame>, CurveError> {
        self.sample_frames(start_offset, policy, Sampling::Count(count))
    }

    /// Sample every `spacing` units from `start_offset` while inside the sampled
    /// range. Spacing must be finite and positive. `PerContour` restarts spacing
    /// at each contour; `Continuous` keeps the spacing across breaks.
    ///
    /// An open terminal frame is included only if it falls on the spacing grid.
    /// See [`Self::frames_at_distance`] for range and frame rules. A query can
    /// return at most 16,384 frames.
    pub fn frames_by_spacing(
        &self,
        spacing: f64,
        start_offset: f64,
        policy: ContourPolicy,
    ) -> Result<Vec<PathFrame>, CurveError> {
        finite(&[spacing])?;
        if spacing <= 0. {
            return Err(CurveError::OutOfRange("frame spacing"));
        }
        self.sample_frames(start_offset, policy, Sampling::Spacing(spacing))
    }

    fn sample_frames(
        &self,
        offset: f64,
        policy: ContourPolicy,
        sampling: Sampling,
    ) -> Result<Vec<PathFrame>, CurveError> {
        finite(&[offset, self.length])?;
        if offset < 0. {
            return Err(CurveError::OutOfRange("frame start offset"));
        }
        let mut base = 0.;
        let mut spans = Vec::new();
        for (index, contour) in self.contours.iter().enumerate() {
            if contour.length > 0. {
                spans.push((index, base));
                base += contour.length;
            }
        }
        let mut frames = Vec::new();
        let mut push = |contour, base, local| {
            if frames.len() == MAX_FRAGMENTS {
                return Err(frame_limit());
            }
            let geometry = self
                .geometry_for(self.location_in(contour, local))
                .ok_or(CurveError::NonFinite)?;
            frames.push(PathFrame {
                distance: base + local,
                contour_distance: local,
                geometry,
            });
            Ok(())
        };

        match policy {
            ContourPolicy::Continuous => {
                if let Some(&(last, _)) = spans.last() {
                    let mut span = 0;
                    sampling.visit(offset, self.length, self.contours[last].closed, |s| {
                        while span + 1 < spans.len() && s >= spans[span + 1].1 {
                            span += 1;
                        }
                        let (index, base) = spans[span];
                        let length = self.contours[index].length;
                        let local = if s == self.length {
                            length
                        } else {
                            (s - base).min(length)
                        };
                        push(index, base, local)
                    })?;
                }
            }
            ContourPolicy::PerContour => {
                for (index, base) in spans {
                    let contour = &self.contours[index];
                    sampling.visit(offset, contour.length, contour.closed, |local| {
                        push(index, base, local)
                    })?;
                }
            }
        }
        Ok(frames)
    }
}

impl Sampling {
    fn visit(
        self,
        offset: f64,
        end: f64,
        closed: bool,
        mut emit: impl FnMut(f64) -> Result<(), CurveError>,
    ) -> Result<(), CurveError> {
        let in_range = |s| s < end || (!closed && s == end);
        if !in_range(offset) || matches!(self, Self::Count(0)) {
            return Ok(());
        }
        if matches!(self, Self::Once | Self::Count(1)) || offset == end {
            return emit(offset);
        }

        let (count, step) = match self {
            Self::Once => unreachable!(),
            Self::Count(count) => {
                if count > MAX_FRAGMENTS {
                    return Err(frame_limit());
                }
                let intervals = if closed { count } else { count - 1 };
                (count, (end - offset) / intervals as f64)
            }
            Self::Spacing(step) => (MAX_FRAGMENTS + 1, step),
        };
        let mut previous = None;
        for i in 0..count {
            let s = if matches!(self, Self::Count(_)) && !closed && i == count - 1 {
                end
            } else {
                step.mul_add(i as f64, offset)
            };
            if !in_range(s) {
                if matches!(self, Self::Count(_)) {
                    return Err(CurveError::WorkLimit(
                        "frame count exceeds numeric resolution",
                    ));
                }
                break;
            }
            if previous.is_some_and(|previous| s <= previous) {
                return Err(CurveError::WorkLimit(
                    "frame spacing is below numeric resolution",
                ));
            }
            emit(s)?;
            previous = Some(s);
        }
        Ok(())
    }
}

fn frame_limit() -> CurveError {
    CurveError::WorkLimit("frame query exceeds 16384 frames")
}
