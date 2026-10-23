use std::time::Instant;

/// Animation time is independent of render cadence. Account for the preceding
/// interval before changing pause/speed so toggles preserve the current phase.
pub struct AnimationClock {
    seconds: f64,
    updated: Instant,
}
impl AnimationClock {
    pub fn new(now: Instant) -> Self {
        Self {
            seconds: 0.,
            updated: now,
        }
    }
    pub fn seek(&mut self, seconds: f32, now: Instant) {
        self.seconds = seconds as f64;
        self.updated = now;
    }
    pub fn seconds(&self) -> f32 {
        self.seconds as f32
    }
    pub fn advance(&mut self, now: Instant, running: bool, slow: bool) -> f32 {
        if running {
            self.seconds += now.saturating_duration_since(self.updated).as_secs_f64()
                * if slow { 0.1 } else { 1. };
        }
        self.updated = now;
        self.seconds()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn pause_and_speed_changes_preserve_phase() {
        let now = Instant::now();
        let mut clock = AnimationClock::new(now);
        assert_eq!(clock.advance(now + Duration::from_secs(2), true, false), 2.);
        // The speed-toggle callback accounts for the old-speed interval first.
        assert_eq!(clock.advance(now + Duration::from_secs(3), true, false), 3.);
        assert!((clock.advance(now + Duration::from_secs(5), true, true) - 3.2).abs() < 0.0001);
        assert!((clock.advance(now + Duration::from_secs(20), false, true) - 3.2).abs() < 0.0001);
        assert!((clock.advance(now + Duration::from_secs(21), true, true) - 3.3).abs() < 0.0001);
        assert!((clock.advance(now + Duration::from_secs(22), true, false) - 4.3).abs() < 0.0001);
    }
}
