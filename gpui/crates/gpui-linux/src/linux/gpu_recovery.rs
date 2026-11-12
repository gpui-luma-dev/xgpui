//! Bounded GPU device recovery for the X11 and Wayland windows (#119).
//!
//! A window whose renderer lost its device retries recovery from its draw
//! path. [`GpuRecovery`] spaces those attempts out and gives up after
//! [`MAX_RECOVERY_ATTEMPTS`], and [`GpuLossReporter`] then tells the app once.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{ForegroundExecutor, GpuDeviceLost};

/// Recovery attempts before a window gives up and stops drawing.
pub(crate) const MAX_RECOVERY_ATTEMPTS: u32 = 5;
/// Delay after the first failed attempt; each later failure doubles it.
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(100);

/// What a window does after a recovery step.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RecoveryStep {
    /// The device was recreated; draw again.
    Recovered,
    /// An attempt failed or the next one is not due; ask for another frame.
    Retry,
    /// The last attempt failed: stop drawing and report this reason.
    Failed(String),
    /// Recovery gave up earlier; do nothing.
    GaveUp,
}

/// Retry state for one window's lost device.
#[derive(Debug, Default)]
pub(crate) struct GpuRecovery {
    failed_attempts: u32,
    next_attempt: Option<Instant>,
    gave_up: bool,
}

impl GpuRecovery {
    /// Runs `recover` if an attempt is due at `now`, and says what to do next.
    pub(crate) fn step(
        &mut self,
        now: Instant,
        recover: impl FnOnce() -> anyhow::Result<()>,
    ) -> RecoveryStep {
        if self.gave_up {
            return RecoveryStep::GaveUp;
        }
        if self.next_attempt.is_some_and(|due| now < due) {
            return RecoveryStep::Retry;
        }
        match recover() {
            Ok(()) => {
                if self.failed_attempts > 0 {
                    log::info!(
                        "GPU recovered after {} failed attempts",
                        self.failed_attempts
                    );
                } else {
                    log::info!("GPU recovered");
                }
                *self = Self::default();
                RecoveryStep::Recovered
            }
            Err(error) => {
                self.failed_attempts += 1;
                if self.failed_attempts >= MAX_RECOVERY_ATTEMPTS {
                    self.gave_up = true;
                    return RecoveryStep::Failed(format!(
                        "GPU recovery failed {MAX_RECOVERY_ATTEMPTS} times: {error:#}"
                    ));
                }
                let delay = FIRST_RETRY_DELAY * 2u32.pow(self.failed_attempts - 1);
                log::warn!(
                    "GPU recovery attempt {} of {MAX_RECOVERY_ATTEMPTS} failed, retrying in {delay:?}: {error:#}",
                    self.failed_attempts
                );
                self.next_attempt = Some(now + delay);
                RecoveryStep::Retry
            }
        }
    }
}

/// Device-loss fault injection for hardware test runs (#119).
///
/// Only debug and `test-support` builds read these variables; release builds
/// compile them out.
///
/// - `XGPUI_FAULT_GPU_LOSS_AFTER=<seconds>`: a window reports its device lost
///   that long after it opens, until recovery succeeds.
/// - `XGPUI_FAULT_GPU_RECOVERY=fail`: every recovery attempt fails.
pub(crate) struct InjectedLoss {
    due: Option<Instant>,
    active: bool,
}

#[cfg(any(debug_assertions, feature = "test-support"))]
fn fault_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[cfg(not(any(debug_assertions, feature = "test-support")))]
fn fault_var(_name: &str) -> Option<String> {
    None
}

impl InjectedLoss {
    pub(crate) fn from_env(start: Instant) -> Self {
        let after = fault_var("XGPUI_FAULT_GPU_LOSS_AFTER")
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.)
            .map(Duration::from_secs_f64);
        if let Some(after) = after {
            log::warn!("fault injection: GPU device loss in {after:?}");
        }
        Self {
            due: after.map(|after| start + after),
            active: false,
        }
    }

    /// Whether the injected loss is in effect at `now`.
    pub(crate) fn is_lost(&mut self, now: Instant) -> bool {
        if self.due.is_some_and(|due| now >= due) {
            self.due = None;
            self.active = true;
            log::warn!("fault injection: reporting a GPU device loss");
        }
        self.active
    }

    /// Ends the injected loss after a successful recovery.
    pub(crate) fn recovered(&mut self) {
        self.active = false;
    }
}

/// Fails when `XGPUI_FAULT_GPU_RECOVERY=fail` is set.
pub(crate) fn check_injected_recovery() -> anyhow::Result<()> {
    if fault_var("XGPUI_FAULT_GPU_RECOVERY").as_deref() == Some("fail") {
        anyhow::bail!("injected recovery failure (XGPUI_FAULT_GPU_RECOVERY=fail)");
    }
    Ok(())
}

/// Tells the app once that GPU recovery failed. Shared by the platform and its windows.
#[derive(Clone)]
pub(crate) struct GpuLossReporter {
    executor: ForegroundExecutor,
    shared: Rc<GpuLossShared>,
}

#[derive(Default)]
struct GpuLossShared {
    callback: RefCell<Option<Box<dyn FnMut(GpuDeviceLost)>>>,
    reported: Cell<bool>,
}

impl GpuLossReporter {
    pub(crate) fn new(executor: ForegroundExecutor) -> Self {
        Self {
            executor,
            shared: Rc::default(),
        }
    }

    pub(crate) fn set_callback(&self, callback: Box<dyn FnMut(GpuDeviceLost)>) {
        *self.shared.callback.borrow_mut() = Some(callback);
    }

    /// Reports a failed recovery once, from the event loop: windows call this
    /// from their draw path, where the app is still borrowed.
    pub(crate) fn report(&self, reason: String) {
        log::error!("GPU device recovery failed; windows stop drawing: {reason}");
        if self.shared.reported.replace(true) {
            return;
        }
        let shared = self.shared.clone();
        self.executor
            .spawn(async move {
                let callback = shared.callback.borrow_mut().take();
                if let Some(mut callback) = callback {
                    callback(GpuDeviceLost::new(reason));
                    shared.callback.borrow_mut().get_or_insert(callback);
                }
            })
            .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> anyhow::Result<()> {
        Err(anyhow::anyhow!("no adapter"))
    }

    #[test]
    fn injected_loss_lasts_until_recovered() {
        let start = Instant::now();
        let mut loss = InjectedLoss {
            due: Some(start + Duration::from_secs(1)),
            active: false,
        };
        assert!(!loss.is_lost(start));
        assert!(loss.is_lost(start + Duration::from_secs(1)));
        assert!(loss.is_lost(start + Duration::from_secs(2)));
        loss.recovered();
        // One-shot: it does not come back.
        assert!(!loss.is_lost(start + Duration::from_secs(60)));
    }

    #[test]
    fn success_recovers_and_resets() {
        let mut recovery = GpuRecovery::default();
        let start = Instant::now();
        assert_eq!(recovery.step(start, fail), RecoveryStep::Retry);
        let later = start + Duration::from_secs(1);
        assert_eq!(recovery.step(later, || Ok(())), RecoveryStep::Recovered);
        // A later loss gets the full set of attempts again.
        for attempt in 1..MAX_RECOVERY_ATTEMPTS {
            let now = later + Duration::from_secs(10 * u64::from(attempt));
            assert_eq!(recovery.step(now, fail), RecoveryStep::Retry);
        }
    }

    #[test]
    fn attempts_back_off_then_give_up() {
        let mut recovery = GpuRecovery::default();
        let mut now = Instant::now();
        let mut attempts = 0;
        let mut counted = || {
            attempts += 1;
            fail()
        };
        assert_eq!(recovery.step(now, &mut counted), RecoveryStep::Retry);
        let mut delay = FIRST_RETRY_DELAY;
        for _ in 1..MAX_RECOVERY_ATTEMPTS - 1 {
            // Not due yet: no attempt is made.
            let early = now + delay - Duration::from_millis(1);
            assert_eq!(recovery.step(early, &mut counted), RecoveryStep::Retry);
            now += delay;
            assert_eq!(recovery.step(now, &mut counted), RecoveryStep::Retry);
            delay *= 2;
        }
        now += delay;
        let RecoveryStep::Failed(reason) = recovery.step(now, &mut counted) else {
            panic!("the last attempt should give up");
        };
        assert!(reason.contains("no adapter"), "{reason}");
        assert_eq!(attempts, MAX_RECOVERY_ATTEMPTS);
        // After giving up, nothing runs again.
        let much_later = now + Duration::from_secs(3600);
        assert_eq!(
            recovery.step(much_later, || panic!("must not retry")),
            RecoveryStep::GaveUp
        );
    }
}
