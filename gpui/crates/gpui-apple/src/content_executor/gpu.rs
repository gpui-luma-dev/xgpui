//! Private completed-texture handoff. Never exposed to application producers.
use super::*;
use std::sync::atomic::AtomicU64;
pub(super) enum Request {
    Readback(ReadbackRequest),
    Gpu(GpuRequest),
}
pub(super) struct ExecutionOutput {
    pub images: Vec<MetalContentImage>,
    pub gpu: Vec<MetalGpuImage>,
    pub stats: MetalContentStats,
}
pub(super) struct GpuImageOwner {
    pub resource: Arc<Resource>,
    // Fields drop in order: release the allocation before publishing its budget.
    pub _reservation: ExportReservation,
}
pub(super) struct ExportReservation {
    pub bytes: u64,
    pub budget: Arc<AtomicU64>,
}
impl Drop for ExportReservation {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
#[derive(Clone)]
pub(crate) struct MetalGpuImage {
    pub(super) owner: Arc<GpuImageOwner>,
    pub descriptor: c::ImageDescriptor,
}
impl MetalGpuImage {
    pub fn texture(&self) -> &TextureRef {
        self.owner.resource.image()
    }
}
pub(crate) struct MetalGpuFrame {
    pub images: Vec<MetalGpuImage>,
    pub stats: MetalContentStats,
}
pub(super) struct GpuRequest {
    pub graph: Arc<c::ContentGraph>,
    pub limits: MetalContentLimits,
    pub state: Arc<GpuJobState>,
}
pub(super) struct GpuJobState {
    result: Mutex<Option<Result<MetalGpuFrame>>>,
    pub cancelled: AtomicBool,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl GpuJobState {
    pub fn finish(&self, result: Result<MetalGpuFrame>) {
        *self.result.lock().unwrap() = Some(result);
        if !self.cancelled.load(Ordering::Acquire) {
            (self.wake)();
        }
    }
}
pub(crate) struct MetalGpuJob {
    state: Arc<GpuJobState>,
}
impl MetalGpuJob {
    pub fn take(&self) -> Option<Result<MetalGpuFrame>> {
        self.state.result.lock().unwrap().take()
    }
}
impl Drop for MetalGpuJob {
    fn drop(&mut self) {
        self.state.cancelled.store(true, Ordering::Release);
    }
}
impl MetalContentExecutor {
    pub(crate) fn submit_gpu(
        &self,
        graph: Arc<c::ContentGraph>,
        limits: MetalContentLimits,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<MetalGpuJob> {
        let state = Arc::new(GpuJobState {
            result: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            wake,
        });
        self.requests
            .try_send(Request::Gpu(GpuRequest {
                graph,
                limits,
                state: state.clone(),
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => failure(
                    c::ContentErrorKind::ResourceLimit,
                    "GPU request queue is full",
                ),
                mpsc::TrySendError::Disconnected(_) => execution("GPU worker is unavailable"),
            })?;
        Ok(MetalGpuJob { state })
    }
}
