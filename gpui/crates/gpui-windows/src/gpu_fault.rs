//! Device-loss fault injection for hardware test runs (#119).
//!
//! Only debug and `test-support` builds read these variables; release builds
//! compile them out.
//!
//! - `XGPUI_FAULT_GPU_LOSS_AFTER=<seconds>`: report a device loss once, that
//!   long after the platform starts, so recovery runs without a real TDR.
//! - `XGPUI_FAULT_GPU_RECOVERY=fail`: make every device recreation fail, so
//!   recovery gives up.

use std::time::{Duration, Instant};

#[cfg(any(debug_assertions, feature = "test-support"))]
fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[cfg(not(any(debug_assertions, feature = "test-support")))]
fn var(_name: &str) -> Option<String> {
    None
}

/// A one-shot injected device loss.
pub(crate) struct InjectedLoss(Option<Instant>);

impl InjectedLoss {
    pub(crate) fn from_env(start: Instant) -> Self {
        let after = var("XGPUI_FAULT_GPU_LOSS_AFTER")
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.)
            .map(Duration::from_secs_f64);
        if let Some(after) = after {
            log::warn!("fault injection: GPU device loss in {after:?}");
        }
        Self(after.map(|after| start + after))
    }

    /// True once, when the injected loss is due.
    pub(crate) fn take_due(&mut self, now: Instant) -> bool {
        if self.0.is_some_and(|due| now >= due) {
            self.0 = None;
            log::warn!("fault injection: reporting a GPU device loss");
            return true;
        }
        false
    }
}

/// Fails when `XGPUI_FAULT_GPU_RECOVERY=fail` is set.
pub(crate) fn check_recovery() -> anyhow::Result<()> {
    if var("XGPUI_FAULT_GPU_RECOVERY").as_deref() == Some("fail") {
        anyhow::bail!("injected recovery failure (XGPUI_FAULT_GPU_RECOVERY=fail)");
    }
    Ok(())
}
