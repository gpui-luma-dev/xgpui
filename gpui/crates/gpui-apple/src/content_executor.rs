//! Worker-owned headless execution of portable content graphs on macOS Metal.
//!
//! This is an offscreen backend, not window presentation. Device initialization
//! and blocking readback waits are intended for setup/validation outside UI draw
//! callbacks. `submit` and awaiting its job do not block the calling UI thread.
//! The renderer uses a private completed-texture handoff on its shared device;
//! CPU readback is only the public headless validation path. External imports remain separate.
use crate::metal::{self, *};
use gpui::content as c;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};
mod encoding;
mod gpu;
mod pipeline;
pub(crate) use gpu::*;
use pipeline::*;
type Result<T> = std::result::Result<T, c::ContentError>;
fn failure(kind: c::ContentErrorKind, message: impl Into<String>) -> c::ContentError {
    c::ContentError::new(kind, message)
}
fn execution(message: impl Into<String>) -> c::ContentError {
    failure(c::ContentErrorKind::ExecutionFailure, message)
}
fn allocation(message: &str) -> c::ContentError {
    failure(c::ContentErrorKind::AllocationFailure, message)
}

/// Worker/queue policy. Small native queues deliberately exercise incremental submission.
#[derive(Clone, Debug)]
pub struct MetalContentOptions {
    /// Maximum native command buffers not yet completed.
    pub queue_capacity: usize,
    /// Bounded requests waiting behind the active job; full queues return an error.
    pub pending_jobs: usize,
    /// Maximum retained native pipeline variants and unique variants in one graph.
    pub pipeline_cache_entries: usize,
}
impl Default for MetalContentOptions {
    fn default() -> Self {
        Self {
            queue_capacity: 2,
            pending_jobs: 2,
            pipeline_cache_entries: 64,
        }
    }
}
/// Per-job memory policy. Charges include native output padding and temporary GPU data.
#[derive(Clone, Debug)]
pub struct MetalContentLimits {
    /// Maximum buffer/texture allocation charge, including readback headroom.
    /// Native pipeline state has a separate entry-count budget.
    pub max_gpu_bytes: u64,
    /// Maximum total packed CPU readback bytes for requested roots.
    pub max_readback_bytes: u64,
    /// Deterministic failure injection after an already-submitted prefix, for tests only.
    #[cfg(feature = "test-support")]
    pub fail_before_node: Option<usize>,
    /// Fail after resource admission but before a recorder exists, for cleanup tests.
    #[cfg(feature = "test-support")]
    pub fail_after_admit: Option<usize>,
    /// Deterministic worker pause for deadline/queue tests; never used in production.
    #[cfg(feature = "test-support")]
    pub pause_before_node: Option<(usize, Arc<MetalContentTestGate>)>,
}
impl Default for MetalContentLimits {
    fn default() -> Self {
        Self {
            max_gpu_bytes: 128 * 1024 * 1024,
            max_readback_bytes: 64 * 1024 * 1024,
            #[cfg(feature = "test-support")]
            fail_before_node: None,
            #[cfg(feature = "test-support")]
            fail_after_admit: None,
            #[cfg(feature = "test-support")]
            pause_before_node: None,
        }
    }
}
/// Test-only deterministic rendezvous, avoiding timing assumptions about GPU speed.
#[cfg(feature = "test-support")]
#[derive(Debug, Default)]
pub struct MetalContentTestGate {
    state: Mutex<(bool, bool)>,
    ready: Condvar,
}
#[cfg(feature = "test-support")]
impl MetalContentTestGate {
    /// Wait until the native worker has reached its configured pause point.
    pub fn wait_until_entered(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        let mut state = self.state.lock().unwrap();
        while !state.0 {
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return false;
            }
            state = self.ready.wait_timeout(state, remaining).unwrap().0;
        }
        true
    }
    /// Release a paused worker. Tests must release on assertion failure as well.
    pub fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.ready.notify_all();
    }
    fn arrive(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.ready.notify_all();
        while !state.1 {
            state = self.ready.wait(state).unwrap();
        }
    }
}
/// CPU-only image result; each level is tightly packed in the descriptor's channel order.
#[derive(Clone, Debug)]
pub struct MetalContentImage {
    /// Source dimensions, storage, and color/alpha meaning (no implicit color conversion).
    pub descriptor: c::ImageDescriptor,
    /// Largest mip first; binary16 words and all other multibyte values are little-endian.
    pub levels: Vec<Arc<[u8]>>,
}
/// Instrumentation for admitted execution, independent of GPU timing measurements.
#[derive(Clone, Debug, Default)]
pub struct MetalContentStats {
    /// Native execution owner, useful for verifying that work stays off the caller thread.
    pub worker_thread: Option<String>,
    /// Graph operations encoded, including one per multi-output dispatch.
    pub operations: usize,
    /// Producer command buffers committed.
    pub submissions: usize,
    /// External native views imported without a pixel upload.
    pub external_imports: usize,
    /// Newly allocated graph output image storage (excludes imported views).
    pub image_allocations: usize,
    /// Source-sized GPU NV12 conversions encoded.
    pub native_conversions: usize,
    /// Fixed NV12 conversion pipelines compiled for this worker on this request.
    pub native_conversion_compilations: usize,
    /// Root readback command buffers committed.
    pub readbacks: usize,
    /// Largest count of submitted work not yet observed complete.
    pub max_in_flight: usize,
    /// Largest accounted native working set, including reserved readback headroom.
    pub peak_reserved_bytes: u64,
    /// Native user-program pipeline variants compiled on this job.
    pub pipeline_compilations: usize,
    /// Unaligned buffer copies handled by a byte-copy compute kernel.
    pub byte_copy_dispatches: usize,
}
/// Completed headless output, one image per requested root, in request order.
#[derive(Clone, Debug)]
pub struct MetalContentFrame {
    /// Fully GPU-completed CPU snapshots; stale frames are never substituted.
    pub images: Vec<MetalContentImage>,
    /// Bounded-execution diagnostics.
    pub stats: MetalContentStats,
}
struct JobInner {
    result: Option<Result<MetalContentFrame>>,
    waker: Option<Waker>,
}
struct JobState {
    inner: Mutex<JobInner>,
    ready: Condvar,
    cancelled: AtomicBool,
}
impl JobState {
    fn finish(&self, result: Result<MetalContentFrame>) {
        let mut inner = self.inner.lock().unwrap();
        inner.result = Some(result);
        let waker = inner.waker.take();
        drop(inner);
        self.ready.notify_all();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
/// Awaitable request. Dropping/canceling it prevents unstarted work; submitted
/// work remains owned until completion. A deadline cannot interrupt a driver hang.
pub struct MetalContentJob {
    state: Arc<JobState>,
}
impl MetalContentJob {
    /// Request cancellation without waiting for native retirement.
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
    }
    /// Blocking headless validation convenience. Never call from a UI draw callback.
    pub fn wait_for_readback(self, timeout: Duration) -> Result<MetalContentFrame> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| execution("invalid readback deadline"))?;
        let mut inner = self.state.inner.lock().unwrap();
        loop {
            if let Some(result) = inner.result.take() {
                return result;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.cancel();
                return Err(failure(
                    c::ContentErrorKind::Timeout,
                    "Metal readback deadline expired; native work retires independently",
                ));
            }
            inner = self.state.ready.wait_timeout(inner, remaining).unwrap().0;
        }
    }
}
impl Future for MetalContentJob {
    type Output = Result<MetalContentFrame>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.state.inner.lock().unwrap();
        if let Some(result) = inner.result.take() {
            Poll::Ready(result)
        } else {
            inner.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
impl Drop for MetalContentJob {
    fn drop(&mut self) {
        self.cancel();
    }
}
struct ReadbackRequest {
    graph: Arc<c::ContentGraph>,
    limits: MetalContentLimits,
    state: Arc<JobState>,
}

/// Serial worker handle. No queue, device, texture, or command buffer is exposed
/// to content producers. Dropping the last handle never joins on the UI thread.
#[derive(Clone)]
pub struct MetalContentExecutor {
    requests: mpsc::SyncSender<Request>,
    capabilities: c::ContentCapabilities,
    #[cfg(any(test, feature = "test-support"))]
    exported_bytes: Arc<std::sync::atomic::AtomicU64>,
}
impl MetalContentExecutor {
    /// Initialize a dedicated native worker and bounded queue. Initialization may
    /// compile a helper kernel; perform setup outside UI rendering.
    pub fn new(options: MetalContentOptions) -> Result<Self> {
        Self::on_device(options, None)
    }
    pub(crate) fn on_device(options: MetalContentOptions, device: Option<Device>) -> Result<Self> {
        if options.queue_capacity == 0
            || options.pending_jobs == 0
            || options.pipeline_cache_entries == 0
        {
            return Err(failure(
                c::ContentErrorKind::InvalidDescriptor,
                "Metal queue, pending-job and pipeline limits must be positive",
            ));
        }
        let exported_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let worker_budget = exported_bytes.clone();
        let (requests, receiver) = mpsc::sync_channel::<Request>(options.pending_jobs);
        let (ready, startup) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("xgpui-content-metal".into())
            .spawn(move || {
                let worker =
                    objc2::rc::autoreleasepool(|_| Worker::new(options, device, worker_budget));
                let mut worker = match worker {
                    Ok(worker) => worker,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let _ = ready.send(Ok(worker.capabilities.clone()));
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Readback(request) => {
                            let result = worker
                                .execute(
                                    request.graph,
                                    request.limits,
                                    &request.state.cancelled,
                                    false,
                                )
                                .map(|output| MetalContentFrame {
                                    images: output.images,
                                    stats: output.stats,
                                });
                            request.state.finish(result);
                        }
                        Request::Gpu(request) => {
                            let result = worker
                                .execute(
                                    request.graph,
                                    request.limits,
                                    &request.state.cancelled,
                                    true,
                                )
                                .map(|output| MetalGpuFrame {
                                    images: output.gpu,
                                    stats: output.stats,
                                });
                            request.state.finish(result);
                        }
                    }
                }
            })
            .map_err(|e| execution(format!("start Metal worker: {e}")))?;
        let capabilities = startup
            .recv()
            .map_err(|_| execution("Metal worker stopped during startup"))??;
        Ok(Self {
            requests,
            capabilities,
            #[cfg(any(test, feature = "test-support"))]
            exported_bytes,
        })
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn retained_gpu_bytes(&self) -> u64 {
        self.exported_bytes.load(Ordering::Acquire)
    }
    /// Functional device report for CPU graph validation.
    pub fn capabilities(&self) -> &c::ContentCapabilities {
        &self.capabilities
    }
    /// Enqueue CPU-only graph data without waiting for execution. Native compilation,
    /// allocations, command recording, completion, and readback all stay on the worker.
    pub fn submit(
        &self,
        graph: Arc<c::ContentGraph>,
        limits: MetalContentLimits,
    ) -> Result<MetalContentJob> {
        let state = Arc::new(JobState {
            inner: Mutex::new(JobInner {
                result: None,
                waker: None,
            }),
            ready: Condvar::new(),
            cancelled: AtomicBool::new(false),
        });
        self.requests
            .try_send(Request::Readback(ReadbackRequest {
                graph,
                limits,
                state: state.clone(),
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => failure(
                    c::ContentErrorKind::ResourceLimit,
                    "Metal worker pending-job allowance is full",
                ),
                mpsc::TrySendError::Disconnected(_) => execution("Metal worker is unavailable"),
            })?;
        Ok(MetalContentJob { state })
    }
}

enum Resource {
    Image(Texture),
    ExternalImage {
        texture: Texture,
        import: crate::custom_draw::core_video::ExternalCoreVideoImport,
        _lease: c::ExternalImageLease,
    },
    Buffer(metal::Buffer),
}
// SAFETY: Metal resource retain/release is thread-safe. Only the serial worker
// maps CPU data, before submission or after completion. GPU mutation/read order
// is defined by the host queue. Completion threads only drop ownership leases.
unsafe impl Send for Resource {}
unsafe impl Sync for Resource {}
impl Resource {
    fn image(&self) -> &TextureRef {
        match self {
            Self::Image(t) => t,
            Self::ExternalImage { texture, .. } => texture,
            _ => unreachable!(),
        }
    }
    fn buffer(&self) -> &metal::BufferRef {
        match self {
            Self::Buffer(b) => b,
            _ => unreachable!(),
        }
    }
    fn allocated_bytes(&self) -> u64 {
        match self {
            Self::Image(t) => MTLResource::allocatedSize(&**t) as u64,
            Self::ExternalImage { texture, .. } => MTLResource::allocatedSize(&**texture) as u64,
            Self::Buffer(b) => MTLResource::allocatedSize(&**b) as u64,
        }
    }
}
struct Worker {
    device: Device,
    queue: CommandQueue,
    capabilities: c::ContentCapabilities,
    options: MetalContentOptions,
    pipelines: HashMap<PipelineKey, std::rc::Rc<Pipeline>>,
    byte_copy: ComputePipelineState,
    nv12_pipeline: Option<RenderPipelineState>,
    exported_bytes: Arc<std::sync::atomic::AtomicU64>,
}
impl Worker {
    fn new(
        options: MetalContentOptions,
        device: Option<Device>,
        exported_bytes: Arc<std::sync::atomic::AtomicU64>,
    ) -> Result<Self> {
        let device = device
            .or_else(|| MTLCreateSystemDefaultDevice())
            .ok_or_else(|| {
                failure(
                    c::ContentErrorKind::UnsupportedCapability,
                    "no native Metal device",
                )
            })?;
        let queue = device
            .newCommandQueueWithMaxCommandBufferCount(options.queue_capacity)
            .ok_or_else(|| allocation("allocate bounded Metal queue"))?;
        let threads = device.maxThreadsPerThreadgroup();
        let mut capabilities = c::ContentCapabilities {
            external_images: true,
            max_image_dimension: 16384,
            max_buffer_bytes: (device.maxBufferLength() as u64).min(u64::from(u32::MAX)),
            ..Default::default()
        };
        capabilities.bindings.max_storage_bytes = capabilities.max_buffer_bytes;
        capabilities.bindings.storage_offset_alignment = 4;
        capabilities.compute = Some(c::ComputeLimits {
            max_workgroup_size: [
                threads.width as u32,
                threads.height as u32,
                threads.depth as u32,
            ],
            max_workgroup_invocations: threads.width.max(threads.height).max(threads.depth) as u32,
            max_dispatch: [65535; 3],
            max_workgroup_memory_bytes: device.maxThreadgroupMemoryLength() as u64,
        });
        capabilities.storage_image_formats =
            vec![c::PixelFormat::Rgba8, c::PixelFormat::Rgba16Float];
        let byte_copy = compile_byte_copy(&device)?;
        Ok(Self {
            device,
            queue,
            capabilities,
            options,
            pipelines: HashMap::new(),
            byte_copy,
            nv12_pipeline: None,
            exported_bytes,
        })
    }
}

fn pixel_format(format: c::PixelFormat) -> MTLPixelFormat {
    match format {
        c::PixelFormat::Rgba8 => MTLPixelFormat::RGBA8Unorm,
        c::PixelFormat::Bgra8 => MTLPixelFormat::BGRA8Unorm,
        c::PixelFormat::Rgba16Float => MTLPixelFormat::RGBA16Float,
        c::PixelFormat::R8 => MTLPixelFormat::R8Unorm,
    }
}
fn texture_descriptor(
    d: &c::ImageDescriptor,
    usage: MTLTextureUsage,
) -> objc2::rc::Retained<TextureDescriptor> {
    let descriptor = TextureDescriptor::new();
    unsafe {
        descriptor.setWidth(d.extent().width() as usize);
        descriptor.setHeight(d.extent().height() as usize);
        descriptor.setMipmapLevelCount(d.mip_levels() as usize);
    }
    descriptor.setPixelFormat(pixel_format(d.format()));
    descriptor.setStorageMode(MTLStorageMode::Private);
    descriptor.setUsage(usage);
    descriptor
}
fn buffer_length(logical: u64) -> Result<usize> {
    usize::try_from(
        logical
            .checked_add(3)
            .ok_or_else(|| allocation("buffer size overflow"))?
            & !3,
    )
    .map_err(|_| allocation("buffer size exceeds address space"))
}
fn buffer_charge(device: &DeviceRef, logical: u64) -> Result<u64> {
    let len = buffer_length(logical)?;
    if len > device.maxBufferLength() {
        return Err(failure(
            c::ContentErrorKind::UnsupportedCapability,
            "Metal buffer/staging size exceeds device limit",
        ));
    }
    Ok((device
        .heapBufferSizeAndAlignWithLength_options(len, MTLResourceOptions::StorageModeShared)
        .size as u64)
        .max((len as u64).div_ceil(16384) * 16384))
}
fn allocate_buffer(device: &DeviceRef, logical: u64) -> Result<Arc<Resource>> {
    let length = buffer_length(logical)?;
    let b = device
        .newBufferWithLength_options(length, MTLResourceOptions::StorageModeShared)
        .ok_or_else(|| allocation("allocate Metal shared buffer"))?;
    // SAFETY: new allocation, not published or submitted; initialize even padding.
    unsafe {
        std::ptr::write_bytes(b.contents().as_ptr().cast::<u8>(), 0, length);
    }
    Ok(Arc::new(Resource::Buffer(metal::Buffer(b))))
}
fn resource(resources: &[Option<Vec<Arc<Resource>>>], id: c::ResourceId) -> &Arc<Resource> {
    &resources[id.node()]
        .as_ref()
        .expect("validated live dependency")[id.output()]
}
