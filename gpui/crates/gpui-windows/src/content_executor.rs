//! Host-owned portable D3D11 execution. Window drawing only consumes completed textures.
use gpui::content as c;
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};
use windows::{
    Win32::Graphics::{Direct3D::*, Direct3D11::*, Dxgi::Common::*},
    core::Interface,
};
mod encoding;
mod pipeline;
mod translation;
use pipeline::*;
type Result<T> = std::result::Result<T, c::ContentError>;
fn error(kind: c::ContentErrorKind, message: impl Into<String>) -> c::ContentError {
    c::ContentError::new(kind, message)
}
fn execution(e: impl std::fmt::Display) -> c::ContentError {
    error(c::ContentErrorKind::ExecutionFailure, e.to_string())
}
fn allocation(e: impl std::fmt::Display) -> c::ContentError {
    error(c::ContentErrorKind::AllocationFailure, e.to_string())
}
#[derive(Clone, Debug)]
pub struct Dx11ContentOptions {
    pub queue_capacity: usize,
    pub pending_jobs: usize,
    pub pipeline_cache_entries: usize,
}
impl Default for Dx11ContentOptions {
    fn default() -> Self {
        Self {
            queue_capacity: 2,
            pending_jobs: 8,
            pipeline_cache_entries: 64,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Dx11ContentLimits {
    pub max_gpu_bytes: u64,
    pub max_readback_bytes: u64,
    #[cfg(feature = "test-support")]
    pub fail_before_node: Option<usize>,
    #[cfg(feature = "test-support")]
    pub fail_after_record: Option<usize>,
    #[cfg(feature = "test-support")]
    pub pause_before_node: Option<(usize, Arc<Dx11ContentTestGate>)>,
}
impl Default for Dx11ContentLimits {
    fn default() -> Self {
        Self {
            max_gpu_bytes: 128 * 1024 * 1024,
            max_readback_bytes: 64 * 1024 * 1024,
            #[cfg(feature = "test-support")]
            fail_before_node: None,
            #[cfg(feature = "test-support")]
            fail_after_record: None,
            #[cfg(feature = "test-support")]
            pause_before_node: None,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Dx11ContentStats {
    pub operations: usize,
    pub submissions: usize,
    pub max_in_flight: usize,
    pub pipeline_compilations: usize,
    pub byte_copy_compilations: usize,
    pub peak_reserved_bytes: u64,
    /// Native external images aliased without an upload or output allocation.
    pub external_imports: usize,
    /// Newly allocated graph image storage (not borrowed external textures).
    pub image_allocations: usize,
    pub readbacks: usize,
    pub worker_thread: Option<String>,
}
#[derive(Debug)]
pub struct Dx11ContentImage {
    pub descriptor: c::ImageDescriptor,
    pub levels: Vec<Vec<u8>>,
}
#[derive(Debug)]
pub struct Dx11ContentFrame {
    pub images: Vec<Dx11ContentImage>,
    pub stats: Dx11ContentStats,
    pub(crate) gpu: Vec<GpuImage>,
}
struct State {
    result: Mutex<Option<Result<Dx11ContentFrame>>>,
    done: Condvar,
    waker: Mutex<Option<Waker>>,
    cancelled: AtomicBool,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl State {
    fn finish(&self, result: Result<Dx11ContentFrame>) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        *self.result.lock().unwrap() = Some(result);
        self.done.notify_all();
        if let Some(w) = self.waker.lock().unwrap().take() {
            w.wake();
        }
        (self.wake)();
    }
}
pub struct Dx11ContentJob {
    state: Arc<State>,
}
impl Dx11ContentJob {
    /// Cancel unstarted work without waiting; submitted prefixes retire normally.
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn take(&self) -> Option<Result<Dx11ContentFrame>> {
        self.state.result.lock().unwrap().take()
    }
    pub fn wait_for_readback(self, timeout: Duration) -> Result<Dx11ContentFrame> {
        let start = Instant::now();
        let mut result = self.state.result.lock().unwrap();
        loop {
            if let Some(result) = result.take() {
                return result;
            }
            let remaining = timeout
                .checked_sub(start.elapsed())
                .ok_or_else(|| execution("portable D3D11 job timed out"))?;
            let (next, wait) = self.state.done.wait_timeout(result, remaining).unwrap();
            result = next;
            if wait.timed_out() && result.is_none() {
                return Err(execution("portable D3D11 job timed out"));
            }
        }
    }
}
impl Future for Dx11ContentJob {
    type Output = Result<Dx11ContentFrame>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut result = self.state.result.lock().unwrap();
        if let Some(result) = result.take() {
            Poll::Ready(result)
        } else {
            *self.state.waker.lock().unwrap() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
impl Drop for Dx11ContentJob {
    fn drop(&mut self) {
        self.state.cancelled.store(true, Ordering::Release);
    }
}
struct Request {
    graph: Arc<c::ContentGraph>,
    limits: Dx11ContentLimits,
    state: Arc<State>,
    readback: bool,
}
#[derive(Debug)]
enum Resource {
    ExternalImage {
        texture: ID3D11Texture2D,
        charge: u64,
        _lease: c::ExternalImageLease,
    },
    Image {
        texture: ID3D11Texture2D,
        charge: u64,
    },
    Buffer {
        buffer: ID3D11Buffer,
        charge: u64,
    },
}
impl Resource {
    fn texture(&self) -> &ID3D11Texture2D {
        match self {
            Self::Image { texture, .. } | Self::ExternalImage { texture, .. } => texture,
            _ => unreachable!(),
        }
    }
    fn buffer(&self) -> &ID3D11Buffer {
        match self {
            Self::Buffer { buffer, .. } => buffer,
            _ => unreachable!(),
        }
    }
    fn charge(&self) -> u64 {
        match self {
            Self::Image { charge, .. }
            | Self::ExternalImage { charge, .. }
            | Self::Buffer { charge, .. } => *charge,
        }
    }
}
#[derive(Debug)]
struct Reservation {
    budget: Arc<AtomicU64>,
    charge: u64,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.charge, Ordering::AcqRel);
    }
}
#[derive(Debug)]
struct ImageOwner {
    resource: Arc<Resource>,
    _reservation: Reservation,
}
#[derive(Clone, Debug)]
pub(crate) struct GpuImage {
    owner: Arc<ImageOwner>,
    pub descriptor: c::ImageDescriptor,
}
impl GpuImage {
    pub fn texture(&self) -> &ID3D11Texture2D {
        self.owner.resource.texture()
    }
}
struct Worker {
    recording_failed: AtomicBool,
    byte_copy_shader: Mutex<Option<ID3D11ComputeShader>>,
    byte_copy_compilations: AtomicU64,
    quarantine: Mutex<Vec<encoding::Flight>>,
    sampling: Arc<Mutex<Vec<(ID3D11Query, Vec<GpuImage>)>>>,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    deferred: ID3D11DeviceContext,
    options: Dx11ContentOptions,
    pipelines: HashMap<PipelineKey, Arc<Pipeline>>,
    pipeline_order: VecDeque<PipelineKey>,
    exported: Arc<AtomicU64>,
}
pub struct Dx11ContentExecutor {
    sampling: Arc<Mutex<Vec<(ID3D11Query, Vec<GpuImage>)>>>,
    sender: std::sync::mpsc::SyncSender<Request>,
    capabilities: c::ContentCapabilities,
    exported: Arc<AtomicU64>,
}
impl Dx11ContentExecutor {
    pub fn new(options: Dx11ContentOptions) -> Result<Self> {
        let devices = crate::DirectXDevices::new().map_err(execution)?;
        Self::on_device(devices.device, devices.device_context, options)
    }
    /// Attach the executor to the consumer's device and immediate context.
    /// The runtime context lock protects query/command-list operations; host
    /// pipelines are restored when portable command lists are executed.
    pub fn on_device(
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        options: Dx11ContentOptions,
    ) -> Result<Self> {
        if options.queue_capacity == 0
            || options.pending_jobs == 0
            || options.pipeline_cache_entries == 0
        {
            return Err(error(
                c::ContentErrorKind::InvalidDescriptor,
                "content queue/pipeline limits must be positive",
            ));
        }
        if unsafe { context.GetDevice() }.map_err(execution)?.as_raw() != device.as_raw()
            || unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE
        {
            return Err(error(
                c::ContentErrorKind::InvalidDescriptor,
                "portable executor requires the matching immediate context",
            ));
        }
        // ExecuteCommandList(..., true) saves/restores UI pipeline state atomically.
        // GetData/Flush and UI calls are protected by the runtime's context lock.
        let protection: ID3D11Multithread = context.cast().map_err(execution)?;
        unsafe {
            let _ = protection.SetMultithreadProtected(true);
        }
        let mut deferred = None;
        unsafe {
            device
                .CreateDeferredContext(0, Some(&mut deferred))
                .map_err(execution)?;
        }
        let capabilities = capabilities(&device);
        let exported = Arc::new(AtomicU64::new(0));
        let sampling = Arc::new(Mutex::new(Vec::new()));
        let worker = Worker {
            recording_failed: AtomicBool::new(false),
            byte_copy_shader: Mutex::new(None),
            byte_copy_compilations: AtomicU64::new(0),
            sampling: sampling.clone(),
            quarantine: Mutex::new(Vec::new()),
            device,
            context,
            deferred: deferred.ok_or_else(|| execution("missing portable context"))?,
            options: options.clone(),
            pipelines: HashMap::new(),
            pipeline_order: VecDeque::new(),
            exported: exported.clone(),
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Request>(options.pending_jobs);
        std::thread::Builder::new()
            .name("dx11-portable-content".into())
            .spawn(move || {
                let mut worker = worker;
                loop {
                    worker.retire_sampling();
                    match receiver.recv_timeout(Duration::from_millis(10)) {
                        Ok(request) => {
                            let result = worker.execute(
                                request.graph,
                                request.limits,
                                &request.state.cancelled,
                                request.readback,
                            );
                            request.state.finish(result);
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .map_err(execution)?;
        Ok(Self {
            sampling,
            sender,
            capabilities,
            exported,
        })
    }
    pub(crate) fn retain_sampling(&self, query: ID3D11Query, images: Vec<GpuImage>) {
        if !images.is_empty() {
            self.sampling.lock().unwrap().push((query, images));
        }
    }
    pub fn capabilities(&self) -> &c::ContentCapabilities {
        &self.capabilities
    }
    pub fn retained_gpu_bytes(&self) -> u64 {
        self.exported.load(Ordering::Acquire)
    }
    pub fn submit(
        &self,
        graph: Arc<c::ContentGraph>,
        limits: Dx11ContentLimits,
    ) -> Result<Dx11ContentJob> {
        self.enqueue(graph, limits, Arc::new(|| {}), true)
    }
    pub(crate) fn submit_gpu(
        &self,
        graph: Arc<c::ContentGraph>,
        limits: Dx11ContentLimits,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Dx11ContentJob> {
        self.enqueue(graph, limits, wake, false)
    }
    fn enqueue(
        &self,
        graph: Arc<c::ContentGraph>,
        limits: Dx11ContentLimits,
        wake: Arc<dyn Fn() + Send + Sync>,
        readback: bool,
    ) -> Result<Dx11ContentJob> {
        let state = Arc::new(State {
            result: Mutex::new(None),
            done: Condvar::new(),
            waker: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            wake,
        });
        self.sender
            .try_send(Request {
                graph,
                limits,
                state: state.clone(),
                readback,
            })
            .map_err(|e| match e {
                std::sync::mpsc::TrySendError::Full(_) => error(
                    c::ContentErrorKind::ResourceLimit,
                    "portable D3D11 request queue is full",
                ),
                _ => execution("portable D3D11 worker closed"),
            })?;
        Ok(Dx11ContentJob { state })
    }
}
fn capabilities(device: &ID3D11Device) -> c::ContentCapabilities {
    let feature_level_11 = unsafe { device.GetFeatureLevel() }.0 >= D3D_FEATURE_LEVEL_11_0.0;
    let mut caps = c::ContentCapabilities::default();
    caps.external_images = feature_level_11;
    caps.max_image_dimension = 16384;
    caps.max_buffer_bytes = 128 * 1024 * 1024;
    caps.compute = Some(c::ComputeLimits {
        max_workgroup_size: [1024, 1024, 64],
        max_workgroup_invocations: 1024,
        max_dispatch: [65535; 3],
        max_workgroup_memory_bytes: 32768,
    });
    caps.storage_image_formats = vec![c::PixelFormat::Rgba8, c::PixelFormat::Rgba16Float];
    caps.bindings = c::BindingLimits {
        max_bind_groups: 4,
        max_bindings_per_group: 128,
        max_bindings: 166,
        max_uniform_bytes: 65536,
        max_storage_bytes: 128 * 1024 * 1024,
        storage_offset_alignment: 4,
        max_vertex_buffers: 32,
        max_vertex_attributes: 32,
        max_vertex_stride: 2048,
    };
    let support = |f| unsafe { device.CheckFormatSupport(format(f)) }.unwrap_or(0);
    caps.image_formats.retain(|f| {
        let flags = support(*f);
        let required = (D3D11_FORMAT_SUPPORT_TEXTURE2D.0
            | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE.0
            | D3D11_FORMAT_SUPPORT_RENDER_TARGET.0) as u32;
        flags & required == required
    });
    caps.renderable_image_formats
        .retain(|f| caps.image_formats.contains(f));
    caps.filterable_image_formats
        .retain(|f| caps.image_formats.contains(f));
    caps.storage_image_formats.retain(|f| {
        caps.image_formats.contains(f)
            && support(*f) & D3D11_FORMAT_SUPPORT_TYPED_UNORDERED_ACCESS_VIEW.0 as u32 != 0
    });
    if !feature_level_11 {
        caps.image_formats.clear();
        caps.renderable_image_formats.clear();
        caps.filterable_image_formats.clear();
        caps.storage_image_formats.clear();
        caps.compute = None;
    }
    caps
}
fn query(device: &ID3D11Device) -> Result<ID3D11Query> {
    let mut q = None;
    unsafe {
        device
            .CreateQuery(
                &D3D11_QUERY_DESC {
                    Query: D3D11_QUERY_EVENT,
                    MiscFlags: 0,
                },
                Some(&mut q),
            )
            .map_err(allocation)?;
    }
    q.ok_or_else(|| allocation("missing event query"))
}
fn complete(device: &ID3D11Device, context: &ID3D11DeviceContext, q: &ID3D11Query) -> Result<bool> {
    unsafe {
        device.GetDeviceRemovedReason().map_err(execution)?;
        let mut value = 0u32;
        context
            .GetData(
                q,
                Some((&mut value as *mut u32).cast()),
                4,
                D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
            )
            .map_err(execution)?;
        Ok(value != 0)
    }
}
fn wait(device: &ID3D11Device, context: &ID3D11DeviceContext, q: &ID3D11Query) -> Result<()> {
    let start = Instant::now();
    while !complete(device, context, q)? {
        if start.elapsed() > Duration::from_secs(20) {
            return Err(execution("D3D11 GPU completion timed out"));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
#[cfg(feature = "test-support")]
#[derive(Debug, Default)]
pub struct Dx11ContentTestGate {
    entered: AtomicBool,
    released: AtomicBool,
}
#[cfg(feature = "test-support")]
impl Dx11ContentTestGate {
    pub fn release(&self) {
        self.released.store(true, Ordering::Release);
    }
    pub fn wait_until_entered(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while !self.entered.load(Ordering::Acquire) {
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }
    fn enter(&self, cancelled: &AtomicBool) {
        self.entered.store(true, Ordering::Release);
        while !self.released.load(Ordering::Acquire) && !cancelled.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Worker {
    fn retire_sampling(&self) -> bool {
        let mut pending = self.sampling.lock().unwrap();
        if !pending.is_empty() {
            unsafe {
                self.context.Flush();
            }
        }
        pending.retain(|(q, _)| matches!(complete(&self.device, &self.context, q), Ok(false)));
        pending.is_empty()
    }
}
