//! Heavy-engine mode (#197): keeps the UI responsive when an engine's GPU
//! work is large compared with a display frame.
//!
//! Measured on Intel Iris Xe (Linux and Windows) and Apple M-series, a
//! GPU-saturating engine starved the UI until three things were combined:
//!
//! 1. **Observe-only presentation.** The compositor never waits on the GPU
//!    for an engine frame; it shows frames once complete
//!    ([`WgpuSurfaceTarget::set_gpu_wait`](crate::WgpuSurfaceTarget)).
//! 2. **Short submissions.** One frame's work is split into chunks of a few
//!    milliseconds of GPU time, so the GPU can schedule the compositor
//!    between them. [`HeavyEngine::chunk_ms`] sets the budget;
//!    `suggested_chunks` turns the measured frame time into a chunk count.
//! 3. **Throttling on compositor lag.** The engine slows down while
//!    frames take longer than [`HeavyEngine::lag_budget_ms`] from present
//!    to first composition, and recovers when they don't; `pace` returns the
//!    delay before the next frame.
//!
//! On Linux, engine queues are also created at low priority where supported
//! (`WgpuSurfaceTarget::request_device`).
use crate::SurfaceStats;
use std::time::{Duration, Instant};

/// Configuration for heavy-engine mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeavyEngine {
    /// Highest frame rate the engine paces to.
    pub max_hz: f32,
    /// Lowest frame rate while the compositor lags.
    pub min_hz: f32,
    /// Present-to-composition delay above which the engine slows down.
    pub lag_budget_ms: f32,
    /// GPU time per submission chunk to aim for.
    pub chunk_ms: f32,
    /// Most submissions to split one frame into (default 64). Measured
    /// with 4 heavy 4K engines on Iris Xe and M-series, 64 matched or beat
    /// 16 on Linux, Windows and macOS.
    pub max_chunks: u32,
}

impl Default for HeavyEngine {
    fn default() -> Self {
        Self {
            max_hz: 60.,
            min_hz: 0.25,
            lag_budget_ms: 33.,
            chunk_ms: 4.,
            max_chunks: 64,
        }
    }
}

impl HeavyEngine {
    fn validated(self) -> Self {
        let max_hz = if self.max_hz.is_finite() {
            self.max_hz.clamp(0.01, 1000.)
        } else {
            60.
        };
        let min_hz = if self.min_hz.is_finite() {
            self.min_hz.clamp(0.01, max_hz)
        } else {
            0.25f32.min(max_hz)
        };
        Self {
            max_hz,
            min_hz,
            lag_budget_ms: if self.lag_budget_ms.is_finite() && self.lag_budget_ms > 0. {
                self.lag_budget_ms
            } else {
                33.
            },
            chunk_ms: if self.chunk_ms.is_finite() && self.chunk_ms > 0. {
                self.chunk_ms
            } else {
                4.
            },
            max_chunks: self.max_chunks.clamp(1, 64),
        }
    }
}

/// Pacing and frame-cost state kept by a target in heavy-engine mode.
#[derive(Debug)]
pub(crate) struct Heavy {
    config: HeavyEngine,
    rate: f32,
    /// Smoothed GPU time per frame (submission to observed completion; an
    /// upper bound, refined each frame).
    frame_ms: Option<f32>,
    pending: Option<Instant>,
}

impl Heavy {
    pub(crate) fn new(config: HeavyEngine) -> Self {
        let config = config.validated();
        Self {
            rate: config.max_hz,
            config,
            frame_ms: None,
            pending: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn config(&self) -> HeavyEngine {
        self.config
    }

    /// The engine submitted a frame's work now.
    pub(crate) fn on_present(&mut self, submitted: Instant) {
        self.pending.get_or_insert(submitted);
    }

    /// Record completion of the pending frame if `reached`.
    pub(crate) fn observe(&mut self, reached: bool) {
        if reached && let Some(submitted) = self.pending.take() {
            let ms = submitted.elapsed().as_secs_f32() * 1000.;
            self.frame_ms = Some(match self.frame_ms {
                Some(previous) => previous * 0.7 + ms * 0.3,
                None => ms,
            });
        }
    }

    /// Delay before the next frame: halve the rate (down to `min_hz`) while
    /// composition lags past the budget, recover by 10% per frame below
    /// two thirds of it.
    pub(crate) fn pace(&mut self, stats: &SurfaceStats) -> Duration {
        let lag = stats.composite_latency_ms;
        let budget = self.config.lag_budget_ms;
        if lag > budget {
            self.rate = (self.rate * 0.5).max(self.config.min_hz);
        } else if lag < budget * (2. / 3.) {
            self.rate = (self.rate * 1.1).min(self.config.max_hz);
        }
        Duration::from_secs_f32(1. / self.rate)
    }

    #[cfg(test)]
    pub(crate) fn rate_hz(&self) -> f32 {
        self.rate
    }

    #[cfg(test)]
    pub(crate) fn frame_ms(&self) -> Option<f32> {
        self.frame_ms
    }

    /// Chunks that keep each submission near `chunk_ms` of GPU time.
    pub(crate) fn suggested_chunks(&self) -> u32 {
        self.frame_ms
            .map(|ms| {
                (ms / self.config.chunk_ms)
                    .ceil()
                    .clamp(1., self.config.max_chunks as f32) as u32
            })
            .unwrap_or(1)
    }
}

/// Snapshot of a target's heavy-engine state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeavyStatus {
    pub config: HeavyEngine,
    /// Current paced frame rate.
    pub rate_hz: f32,
    /// Smoothed GPU time per frame, once measured.
    pub frame_ms: Option<f32>,
    /// Chunks per frame that meet `chunk_ms`.
    pub suggested_chunks: u32,
}

impl Heavy {
    pub(crate) fn status(&self) -> HeavyStatus {
        HeavyStatus {
            config: self.config,
            rate_hz: self.rate,
            frame_ms: self.frame_ms,
            suggested_chunks: self.suggested_chunks(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lagging(ms: f32) -> SurfaceStats {
        SurfaceStats {
            composite_latency_ms: ms,
            ..Default::default()
        }
    }

    #[test]
    fn pace_backs_off_while_lagging_and_recovers() {
        let mut heavy = Heavy::new(HeavyEngine::default());
        assert_eq!(heavy.pace(&lagging(5.)), Duration::from_secs_f32(1. / 60.));
        for _ in 0..20 {
            heavy.pace(&lagging(200.));
        }
        assert_eq!(heavy.rate_hz(), 0.25, "floors at min_hz");
        // Within budget but above two thirds of it: hold.
        heavy.pace(&lagging(30.));
        assert_eq!(heavy.rate_hz(), 0.25);
        for _ in 0..200 {
            heavy.pace(&lagging(5.));
        }
        assert_eq!(heavy.rate_hz(), 60., "recovers to max_hz");
    }

    #[test]
    fn chunks_follow_measured_frame_time() {
        let mut heavy = Heavy::new(HeavyEngine::default());
        assert_eq!(heavy.suggested_chunks(), 1, "unmeasured");
        heavy.on_present(Instant::now() - Duration::from_millis(40));
        heavy.observe(false);
        assert_eq!(heavy.frame_ms(), None, "not reached yet");
        heavy.observe(true);
        let ms = heavy.frame_ms().unwrap();
        assert!(ms >= 40., "{ms}");
        let cap = HeavyEngine::default().max_chunks;
        assert_eq!(heavy.suggested_chunks(), ((ms / 4.).ceil() as u32).min(cap));
        // A frame far over budget is capped at max_chunks.
        heavy.on_present(Instant::now() - Duration::from_secs(2));
        heavy.observe(true);
        assert_eq!(heavy.suggested_chunks(), cap);
    }

    #[test]
    fn invalid_config_is_sanitised() {
        let heavy = Heavy::new(HeavyEngine {
            max_hz: f32::NAN,
            min_hz: 500.,
            lag_budget_ms: -1.,
            chunk_ms: 0.,
            max_chunks: 0,
        });
        let config = heavy.config();
        assert_eq!(config.max_hz, 60.);
        assert_eq!(config.min_hz, 60.);
        assert_eq!(config.lag_budget_ms, 33.);
        assert_eq!(config.chunk_ms, 4.);
        assert_eq!(config.max_chunks, 1);
    }
}
