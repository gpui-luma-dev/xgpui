//! Admission for background GPU preparation, staging lifetime and frame handoffs.
use anyhow::{Result, ensure};
use std::sync::{Arc, Mutex};

/// Shared upload admission. Metal/wgpu leases include in-flight staging until
/// GPU completion; D3D11 currently releases at observed submission. Idle pool
/// resources have a separate equal capacity. These are not total VRAM bounds.
#[derive(Clone, Copy, Debug)]
pub struct UploadLimits {
    pub max_jobs: usize,
    pub max_staging_bytes: usize,
}
impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            max_jobs: 2,
            max_staging_bytes: 128 * 1024 * 1024,
        }
    }
}
/// Hard limits for newly adopted sources in one scene frame, shared by images
/// using the same loader. A source must fit in one frame; chunking is not implicit.
#[derive(Clone, Copy, Debug)]
pub struct FrameUploadLimits {
    pub max_uploads: usize,
    pub max_bytes: usize,
}
impl Default for FrameUploadLimits {
    fn default() -> Self {
        Self {
            max_uploads: 2,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}
pub(crate) enum BackendPool {
    #[cfg(target_os = "macos")]
    Metal(u64, gpui_apple::custom_draw::MetalImageStagingPool),
    #[cfg(feature = "wgpu")]
    Wgpu(u64, gpui_wgpu::custom_draw::WgpuImageStagingPool),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UploadStats {
    pub active_jobs: usize,
    pub staging_bytes: usize,
}
pub(crate) struct UploadScheduler {
    limits: UploadLimits,
    frame_limits: FrameUploadLimits,
    pub(crate) pool: Mutex<Option<BackendPool>>,
    usage: Mutex<UploadStats>,
}
pub(crate) struct WorkerPermit(Arc<UploadScheduler>);
pub(crate) struct StagingPermit {
    scheduler: Arc<UploadScheduler>,
    bytes: usize,
}
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.0.usage.lock().unwrap().active_jobs -= 1;
    }
}
impl Drop for StagingPermit {
    fn drop(&mut self) {
        self.scheduler.usage.lock().unwrap().staging_bytes -= self.bytes;
    }
}
impl UploadScheduler {
    #[cfg(test)]
    pub(crate) fn new(limits: UploadLimits) -> Result<Arc<Self>> {
        Self::with_frame_limits(limits, FrameUploadLimits::default())
    }
    pub(crate) fn with_frame_limits(
        limits: UploadLimits,
        frame_limits: FrameUploadLimits,
    ) -> Result<Arc<Self>> {
        ensure!(
            limits.max_jobs > 0
                && limits.max_staging_bytes > 0
                && frame_limits.max_uploads > 0
                && frame_limits.max_bytes > 0,
            "invalid upload limits"
        );
        Ok(Arc::new(Self {
            limits,
            frame_limits,
            pool: Mutex::new(None),
            usage: Mutex::new(UploadStats::default()),
        }))
    }
    /// Busy admission is retried on a future paint, without spawning a task.
    pub(crate) fn reserve(
        self: &Arc<Self>,
        bytes: usize,
    ) -> Result<Option<(WorkerPermit, StagingPermit)>> {
        ensure!(
            bytes <= self.limits.max_staging_bytes,
            "image exceeds upload staging budget"
        );
        let mut usage = self.usage.lock().unwrap();
        if usage.active_jobs >= self.limits.max_jobs
            || bytes > self.limits.max_staging_bytes - usage.staging_bytes
        {
            return Ok(None);
        }
        usage.active_jobs += 1;
        usage.staging_bytes += bytes;
        Ok(Some((
            WorkerPermit(self.clone()),
            StagingPermit {
                scheduler: self.clone(),
                bytes,
            },
        )))
    }
    #[cfg(any(target_os = "macos", feature = "wgpu"))]
    pub(crate) fn pool_capacity(&self) -> usize {
        self.limits.max_staging_bytes
    }
    pub(crate) fn validate_frame_size(&self, bytes: usize) -> Result<()> {
        ensure!(
            bytes <= self.frame_limits.max_bytes,
            "image exceeds per-frame upload byte limit"
        );
        Ok(())
    }
    pub(crate) fn admit_frame(
        self: &Arc<Self>,
        frame: &gpui::PreparationFrame,
        bytes: usize,
    ) -> bool {
        frame.try_reserve_upload(
            Arc::as_ptr(self) as usize as u64,
            bytes,
            self.frame_limits.max_bytes,
            self.frame_limits.max_uploads,
        )
    }
    pub(crate) fn pool_stats(&self) -> gpui::ImageStagingPoolStats {
        match self.pool.lock().unwrap().as_ref() {
            #[cfg(target_os = "macos")]
            Some(BackendPool::Metal(_, pool)) => pool.stats(),
            #[cfg(feature = "wgpu")]
            Some(BackendPool::Wgpu(_, pool)) => pool.stats(),
            _ => gpui::ImageStagingPoolStats::default(),
        }
    }
    pub(crate) fn stats(&self) -> UploadStats {
        *self.usage.lock().unwrap()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_slots_and_staged_bytes_have_independent_lifetimes() -> Result<()> {
        let scheduler = UploadScheduler::new(UploadLimits {
            max_jobs: 1,
            max_staging_bytes: 32,
        })?;
        let (worker, staging) = scheduler.reserve(24)?.unwrap();
        assert!(scheduler.reserve(1)?.is_none());
        drop(worker);
        assert!(scheduler.reserve(16)?.is_none());
        assert_eq!(
            scheduler.stats(),
            UploadStats {
                active_jobs: 0,
                staging_bytes: 24
            }
        );
        drop(staging);
        let (worker, staging) = scheduler.reserve(32)?.unwrap();
        drop((worker, staging));
        assert_eq!(scheduler.stats(), UploadStats::default());
        assert!(scheduler.reserve(33).is_err());
        Ok(())
    }
}
