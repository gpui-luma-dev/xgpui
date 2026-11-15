//! Presentation mode for wgpu targets, including the automatic choice (#262).
//!
//! `interop-boids` showed neither fixed mode suits every load. With GPU waits,
//! a step longer than a display frame holds up the compositor (at 54 ms steps
//! even resizing fell to 18 fps). Observe-only, a short step is sometimes not
//! finished when its present wakes the UI, so it takes a second UI frame (9 ms
//! steps fell from 60 to 38 steps/s). `Auto` measures each present's GPU time
//! and switches at one refresh interval, back below three quarters of one.
use crate::wgpu;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How the compositor treats a target's frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presentation {
    /// The compositor waits on the GPU for each frame, so the next UI frame
    /// shows it. Best while a frame's GPU work fits in a display frame.
    GpuWait,
    /// The compositor never waits on the GPU; it shows each frame once its
    /// work is observed complete. Best for work longer than a display frame.
    ObserveOnly,
    /// Choose per frame from the measured GPU time: observe-only above one
    /// refresh interval, GPU waits again below three quarters of one. Uses
    /// the window's refresh interval ([`EngineSurface::refresh_interval`]),
    /// or 60 Hz before the surface has been painted.
    ///
    /// [`EngineSurface::refresh_interval`]: crate::EngineSurface::refresh_interval
    Auto,
}

const OBSERVE_ONLY_ABOVE: f32 = 1.;
const GPU_WAIT_BELOW: f32 = 0.75;
/// Presents to ignore after a mode change: pipeline creation and first-use
/// costs make the first measurements unrepresentative.
const WARM_UP: u32 = 5;
const DEFAULT_REFRESH: Duration = Duration::from_nanos(16_666_667);

/// Mode, GPU timing and the automatic decision for one target.
///
/// GPU time is measured on a timing thread that waits on each timed
/// submission. Completion callbacks would only fire when the engine next
/// polls its device, typically a display frame later, which would measure the
/// engine's own wait instead of the GPU work.
pub(crate) struct Presenter {
    mode: Presentation,
    /// `Auto`'s current decision.
    observe_only: bool,
    gpu_ms: Option<f32>,
    warm_up: u32,
    /// Submissions to time; one at a time, so a slow GPU isn't queued up.
    timing: std::sync::mpsc::Sender<(wgpu::SubmissionIndex, Instant)>,
    in_flight: Arc<AtomicBool>,
    /// The last measured submission-to-completion time.
    measured: Arc<Mutex<Option<Duration>>>,
}

impl std::fmt::Debug for Presenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Presenter")
            .field("mode", &self.mode)
            .field("observe_only", &self.observe_only)
            .field("gpu_ms", &self.gpu_ms)
            .finish()
    }
}

impl Presenter {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let (timing, requests) = std::sync::mpsc::channel::<(wgpu::SubmissionIndex, Instant)>();
        let in_flight = Arc::new(AtomicBool::new(false));
        let measured = Arc::new(Mutex::new(None));
        let device = device.clone();
        let (flag, result) = (in_flight.clone(), measured.clone());
        // Ends when the presenter (and so the sender) is dropped.
        let _ = std::thread::Builder::new()
            .name("gpu-interop-timing".into())
            .spawn(move || {
                for (index, submitted) in requests {
                    let finished = device.poll(wgpu::PollType::Wait {
                        submission_index: Some(index),
                        timeout: Some(Duration::from_secs(5)),
                    });
                    if finished.is_ok() {
                        *result.lock().unwrap() = Some(submitted.elapsed());
                    }
                    flag.store(false, Ordering::Release);
                }
            });
        Self {
            mode: Presentation::GpuWait,
            observe_only: false,
            gpu_ms: None,
            warm_up: WARM_UP,
            timing,
            in_flight,
            measured,
        }
    }

    pub(crate) fn set(&mut self, mode: Presentation) {
        if mode == Presentation::Auto && self.mode != Presentation::Auto {
            self.warm_up = WARM_UP;
        }
        self.mode = mode;
    }

    /// Whether the next frame gets a GPU-side wait.
    pub(crate) fn gpu_wait(&self) -> bool {
        match self.mode {
            Presentation::GpuWait => true,
            Presentation::ObserveOnly => false,
            Presentation::Auto => !self.observe_only,
        }
    }

    pub(crate) fn gpu_ms(&self) -> Option<f32> {
        self.gpu_ms
    }

    /// Learn the last timed present's GPU time if it has finished, then, in
    /// `Auto`, choose the mode for the next frame. Returns whether to wait.
    pub(crate) fn update(&mut self, refresh: Option<Duration>) -> bool {
        if let Some(measured) = self.measured.lock().unwrap().take() {
            if self.warm_up > 0 {
                self.warm_up -= 1;
            } else {
                let ms = measured.as_secs_f32() * 1000.;
                self.gpu_ms = Some(self.gpu_ms.map_or(ms, |smoothed| smoothed * 0.7 + ms * 0.3));
            }
        }
        if self.mode == Presentation::Auto
            && let Some(gpu_ms) = self.gpu_ms
        {
            let frame_ms = refresh.unwrap_or(DEFAULT_REFRESH).as_secs_f32() * 1000.;
            let switch = if self.observe_only {
                gpu_ms < GPU_WAIT_BELOW * frame_ms
            } else {
                gpu_ms > OBSERVE_ONLY_ABOVE * frame_ms
            };
            if switch {
                self.observe_only = !self.observe_only;
            }
        }
        self.gpu_wait()
    }

    /// Time the submission `index`, made at `at`, unless one is still being
    /// timed.
    pub(crate) fn submitted(&mut self, index: wgpu::SubmissionIndex, at: Instant) {
        if self.in_flight.swap(true, Ordering::AcqRel) {
            return;
        }
        if self.timing.send((index, at)).is_err() {
            self.in_flight.store(false, Ordering::Release);
        }
    }
}
