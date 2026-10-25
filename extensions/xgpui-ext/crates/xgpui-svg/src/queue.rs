use crate::RasterPolicy;
use gpui::{DevicePixels, Size};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Preview,
    Detail,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    pub pixels: Size<DevicePixels>,
    pub stage: Stage,
    id: u64,
    revision: u64,
    generation: usize,
}
#[derive(Clone, Copy)]
struct Target {
    generation: usize,
    pixels: Size<DevicePixels>,
    changed: Duration,
}
/// A latest-request queue. Time is supplied by the host for deterministic tests.
/// Active resvg work is never claimed to be interruptible.
pub struct RasterQueue {
    pub policy: RasterPolicy,
    desired: Option<Target>,
    current: Option<(usize, Size<DevicePixels>)>,
    active: Option<Job>,
    revision: u64,
    next_id: u64,
    failed: bool,
}
impl RasterQueue {
    pub fn new(policy: RasterPolicy) -> Self {
        Self {
            policy,
            desired: None,
            current: None,
            active: None,
            revision: 0,
            next_id: 0,
            failed: false,
        }
    }
    /// Invalidate pending/results while allowing an existing worker to drain.
    pub fn invalidate(&mut self) {
        self.desired = None;
        self.current = None;
        self.revision += 1;
        self.failed = false;
    }
    pub fn active_stage(&self) -> Option<Stage> {
        self.active.map(|job| job.stage)
    }
    pub fn set_current(&mut self, generation: usize, pixels: Size<DevicePixels>) {
        self.current = Some((generation, pixels));
    }
    pub fn request(&mut self, generation: usize, pixels: Size<DevicePixels>, now: Duration) {
        if self
            .desired
            .is_some_and(|d| d.generation == generation && d.pixels == pixels)
        {
            return;
        }
        self.desired = Some(Target {
            generation,
            pixels,
            changed: now,
        });
        self.revision += 1;
        self.failed = false;
    }
    pub fn busy(&self) -> bool {
        self.active.is_some()
    }
    pub fn ready(&self) -> bool {
        self.desired
            .zip(self.current)
            .is_some_and(|(d, (generation, p))| {
                generation == d.generation
                    && p.width >= d.pixels.width
                    && p.height >= d.pixels.height
            })
    }
    pub fn next(&mut self, now: Duration) -> Option<Job> {
        if self.busy() || self.ready() || self.failed {
            return None;
        }
        let desired = self.desired?;
        let preview = self.policy.preview(desired.pixels);
        let has_preview = self.current.is_some_and(|(g, p)| {
            g == desired.generation && p.width >= preview.width && p.height >= preview.height
        });
        let (pixels, stage) =
            if !has_preview && (preview != desired.pixels || self.current.is_none()) {
                (preview, Stage::Preview)
            } else {
                if now.saturating_sub(desired.changed) < self.policy.settle_delay {
                    return None;
                }
                (desired.pixels, Stage::Detail)
            };
        self.next_id += 1;
        let job = Job {
            pixels,
            stage,
            id: self.next_id,
            revision: self.revision,
            generation: desired.generation,
        };
        self.active = Some(job);
        Some(job)
    }
    pub(crate) fn is_current(&self, job: Job) -> bool {
        self.active == Some(job)
            && (job.revision == self.revision
                || (job.stage == Stage::Preview
                    && self.desired.is_some_and(|d| d.generation == job.generation)))
    }
    pub(crate) fn cancel_pending(&mut self, job: Job) {
        if self.active == Some(job) {
            self.active = None;
        }
    }
    pub fn wake_after(&self, now: Duration) -> Option<Duration> {
        if self.busy() || self.ready() || self.failed {
            return None;
        }
        let target = self.desired?;
        Some(
            self.policy
                .settle_delay
                .saturating_sub(now.saturating_sub(target.changed)),
        )
    }
    /// Returns true only for an active result that still matches the latest request.
    pub fn complete(&mut self, job: Job, success: bool) -> bool {
        if self.active != Some(job) {
            return false;
        }
        self.active = None;
        if job.revision != self.revision {
            // A coarse preview of the same document is useful during a wheel burst.
            // Never accept an old document/budget, a stale detail, or a downgrade.
            let useful_preview = success
                && job.stage == Stage::Preview
                && self.desired.is_some_and(|d| d.generation == job.generation)
                && self.current.is_none_or(|(g, p)| {
                    g != job.generation
                        || (p.width <= job.pixels.width && p.height <= job.pixels.height)
                });
            if !useful_preview {
                return false;
            }
        }
        if success {
            if let Some(target) = self.desired {
                self.current = Some((target.generation, job.pixels));
            }
        } else {
            self.failed = true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;
    fn pixels(n: i32) -> Size<DevicePixels> {
        size(DevicePixels(n), DevicePixels(n))
    }
    #[test]
    fn burst_renders_preview_then_only_latest_settled_detail() {
        let mut q = RasterQueue::new(RasterPolicy::default());
        q.set_current(0, pixels(320));
        q.request(0, pixels(1200), Duration::ZERO);
        let preview = q.next(Duration::ZERO).unwrap();
        assert_eq!(preview.stage, Stage::Preview);
        assert!(q.next(Duration::from_millis(500)).is_none());
        q.request(0, pixels(1800), Duration::from_millis(20));
        assert!(q.complete(preview, true)); // same-document coarse preview remains useful
        assert!(q.next(Duration::from_millis(30)).is_none());
        q.request(0, pixels(2048), Duration::from_millis(60));
        assert!(q.next(Duration::from_millis(179)).is_none());
        let detail = q.next(Duration::from_millis(180)).unwrap();
        assert_eq!(detail.pixels, pixels(2048));
        assert_eq!(detail.stage, Stage::Detail);
        assert!(q.complete(detail, true));
        assert!(q.ready());
        q.request(0, pixels(1000), Duration::from_millis(200));
        assert!(q.ready());
        assert!(q.next(Duration::from_millis(400)).is_none()); // reuse on zoom-out/pan
    }
    #[test]
    fn old_document_and_failure_cannot_replay_or_spin() {
        let mut q = RasterQueue::new(RasterPolicy::default());
        q.request(0, pixels(1500), Duration::ZERO);
        let old = q.next(Duration::ZERO).unwrap();
        q.set_current(1, pixels(320));
        q.request(1, pixels(1500), Duration::from_millis(20));
        assert!(!q.complete(old, true));
        let fresh = q.next(Duration::from_millis(30)).unwrap();
        assert!(q.complete(fresh, false));
        assert!(q.next(Duration::from_secs(1)).is_none());
        assert!(q.wake_after(Duration::from_secs(1)).is_none());
        assert!(!q.complete(fresh, true));
    }
}
