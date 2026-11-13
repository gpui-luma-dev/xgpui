//! Diagnostics for the Wayland frame loop (#271).
//!
//! A window waiting for a compositor frame callback ignores redraw requests until
//! the callback arrives, so a callback that never comes stops presentation
//! silently. These hooks make that visible.
//!
//! Log target `xgpui::wayland::frame`: `trace` records every frame-loop step,
//! `warn` reports a stalled callback, `info` reports the present mode and a late
//! callback.
//!
//! Debug environment variables (not API; they exist to diagnose #271):
//! - `XGPUI_WAYLAND_PRESENT_MODE=fifo|mailbox|immediate` overrides the present mode.
//! - `XGPUI_WAYLAND_FRAME_WATCHDOG=1` treats a callback outstanding longer than
//!   [`STALL_AFTER`] as lost and resumes the frame loop.

use gpui_wgpu::wgpu;
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

pub(crate) const TARGET: &str = "xgpui::wayland::frame";

/// Several refresh intervals even at 30 Hz; well past any normal callback delay
/// for a visible surface.
pub(crate) const STALL_AFTER: Duration = Duration::from_millis(500);

/// The present mode Wayland windows request, unless overridden for diagnosis.
pub(crate) fn present_mode() -> wgpu::PresentMode {
    static MODE: OnceLock<wgpu::PresentMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        let requested = std::env::var("XGPUI_WAYLAND_PRESENT_MODE").ok();
        let mode = match requested.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("") | Some("mailbox") => wgpu::PresentMode::Mailbox,
            Some("fifo") => wgpu::PresentMode::Fifo,
            Some("immediate") => wgpu::PresentMode::Immediate,
            Some(other) => {
                log::warn!(
                    target: TARGET,
                    "ignoring XGPUI_WAYLAND_PRESENT_MODE={other}; expected fifo, mailbox or immediate"
                );
                wgpu::PresentMode::Mailbox
            }
        };
        log::info!(target: TARGET, "Wayland windows request present mode {mode:?}");
        mode
    })
}

pub(crate) fn watchdog_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let enabled = std::env::var("XGPUI_WAYLAND_FRAME_WATCHDOG").is_ok_and(|v| v == "1");
        if enabled {
            log::info!(target: TARGET, "frame callback watchdog enabled");
        }
        enabled
    })
}

/// The outstanding frame callback after a successful present.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CallbackWait {
    since: Option<Instant>,
    warned: bool,
    presents: u64,
}

impl CallbackWait {
    pub(crate) fn presented(&mut self, now: Instant) {
        self.presents += 1;
        self.since = Some(now);
        self.warned = false;
    }

    pub(crate) fn presents(&self) -> u64 {
        self.presents
    }

    /// Clears the wait; returns how long it lasted if it had been reported stalled.
    pub(crate) fn arrived(&mut self, now: Instant) -> Option<Duration> {
        let late = self
            .since
            .take()
            .filter(|_| self.warned)
            .map(|since| now.saturating_duration_since(since));
        self.warned = false;
        late
    }

    /// The wait's age the first time it passes [`STALL_AFTER`]; `None` otherwise.
    pub(crate) fn newly_stalled(&mut self, now: Instant) -> Option<Duration> {
        let age = now.saturating_duration_since(self.since?);
        if self.warned || age < STALL_AFTER {
            return None;
        }
        self.warned = true;
        Some(age)
    }

    pub(crate) fn abandon(&mut self) {
        self.since = None;
        self.warned = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stall_is_reported_once_and_a_late_callback_reports_its_delay() {
        let start = Instant::now();
        let mut wait = CallbackWait::default();
        assert_eq!(
            wait.newly_stalled(start + STALL_AFTER),
            None,
            "no present yet"
        );

        wait.presented(start);
        assert_eq!(wait.presents(), 1);
        assert_eq!(wait.newly_stalled(start + STALL_AFTER / 2), None);
        assert_eq!(
            wait.newly_stalled(start + STALL_AFTER),
            Some(STALL_AFTER),
            "first check past the threshold reports"
        );
        assert_eq!(
            wait.newly_stalled(start + STALL_AFTER * 2),
            None,
            "reported once"
        );
        assert_eq!(wait.arrived(start + STALL_AFTER * 3), Some(STALL_AFTER * 3));
        assert_eq!(
            wait.newly_stalled(start + STALL_AFTER * 10),
            None,
            "cleared"
        );
    }

    #[test]
    fn a_prompt_callback_is_not_reported() {
        let start = Instant::now();
        let mut wait = CallbackWait::default();
        wait.presented(start);
        assert_eq!(wait.arrived(start + Duration::from_millis(16)), None);
    }

    #[test]
    fn abandoning_a_wait_stops_stall_reports() {
        let start = Instant::now();
        let mut wait = CallbackWait::default();
        wait.presented(start);
        wait.abandon();
        assert_eq!(wait.newly_stalled(start + STALL_AFTER * 2), None);
    }
}
