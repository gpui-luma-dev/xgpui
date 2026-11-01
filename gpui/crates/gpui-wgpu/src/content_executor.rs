//! Host-owned portable graph execution. Native work runs on a serial worker;
//! browsers drive the same asynchronous engine on the owning event-loop thread.
//! No waits or producer readbacks occur in window drawing.
use gpui::content as c;
#[cfg(not(target_family = "wasm"))]
use std::time::{Duration, Instant};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};
mod encoding;
mod pipeline;
use pipeline::*;
type Result<T> = std::result::Result<T, c::ContentError>;
fn error(kind: c::ContentErrorKind, message: impl Into<String>) -> c::ContentError {
    c::ContentError::new(kind, message)
}
fn execution(message: impl Into<String>) -> c::ContentError {
    error(c::ContentErrorKind::ExecutionFailure, message)
}
fn unsupported(message: impl Into<String>) -> c::ContentError {
    error(c::ContentErrorKind::UnsupportedCapability, message)
}
#[derive(Clone, Debug)]
pub struct WgpuContentOptions {
    pub queue_capacity: usize,
    pub pending_jobs: usize,
    pub pipeline_cache_entries: usize,
}
impl Default for WgpuContentOptions {
    fn default() -> Self {
        Self {
            queue_capacity: 2,
            pending_jobs: 8,
            pipeline_cache_entries: 64,
        }
    }
}
#[derive(Clone, Debug)]
pub struct WgpuContentLimits {
    pub max_gpu_bytes: u64,
    pub max_readback_bytes: u64,
    #[cfg(feature = "test-support")]
    pub fail_before_node: Option<usize>,
    #[cfg(feature = "test-support")]
    pub pause_before_node: Option<(usize, Arc<WgpuContentTestGate>)>,
}
impl Default for WgpuContentLimits {
    fn default() -> Self {
        Self {
            max_gpu_bytes: 128 * 1024 * 1024,
            max_readback_bytes: 64 * 1024 * 1024,
            #[cfg(feature = "test-support")]
            fail_before_node: None,
            #[cfg(feature = "test-support")]
            pause_before_node: None,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct WgpuContentStats {
    pub operations: usize,
    pub submissions: usize,
    pub max_in_flight: usize,
    pub pipeline_compilations: usize,
    pub peak_reserved_bytes: u64,
    pub readbacks: usize,
    pub worker_thread: Option<String>,
}
#[derive(Debug)]
pub struct WgpuContentImage {
    pub descriptor: c::ImageDescriptor,
    pub levels: Vec<Vec<u8>>,
}
#[derive(Debug)]
pub struct WgpuContentFrame {
    pub images: Vec<WgpuContentImage>,
    pub stats: WgpuContentStats,
    pub(crate) gpu: Vec<GpuImage>,
}
struct State {
    result: Mutex<Option<Result<WgpuContentFrame>>>,
    done: Condvar,
    waker: Mutex<Option<Waker>>,
    cancelled: AtomicBool,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl State {
    fn finish(&self, result: Result<WgpuContentFrame>) {
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
pub struct WgpuContentJob {
    state: Arc<State>,
}
impl WgpuContentJob {
    pub(crate) fn take(&self) -> Option<Result<WgpuContentFrame>> {
        self.state.result.lock().unwrap().take()
    }
    #[cfg(not(target_family = "wasm"))]
    pub fn wait_for_readback(self, timeout: Duration) -> Result<WgpuContentFrame> {
        let start = Instant::now();
        let mut result = self.state.result.lock().unwrap();
        loop {
            if let Some(result) = result.take() {
                return result;
            }
            let remaining = timeout
                .checked_sub(start.elapsed())
                .ok_or_else(|| execution("portable wgpu job timed out"))?;
            let (next, wait) = self.state.done.wait_timeout(result, remaining).unwrap();
            result = next;
            if wait.timed_out() && result.is_none() {
                return Err(execution("portable wgpu job timed out"));
            }
        }
    }
}
impl Future for WgpuContentJob {
    type Output = Result<WgpuContentFrame>;
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
impl Drop for WgpuContentJob {
    fn drop(&mut self) {
        self.state.cancelled.store(true, Ordering::Release);
    }
}
struct Request {
    graph: Arc<c::ContentGraph>,
    limits: WgpuContentLimits,
    state: Arc<State>,
    readback: bool,
}
#[derive(Debug)]
enum Resource {
    Image { texture: wgpu::Texture, charge: u64 },
    Buffer { buffer: wgpu::Buffer, charge: u64 },
}
impl Resource {
    fn texture(&self) -> &wgpu::Texture {
        match self {
            Self::Image { texture, .. } => texture,
            _ => unreachable!(),
        }
    }
    fn buffer(&self) -> &wgpu::Buffer {
        match self {
            Self::Buffer { buffer, .. } => buffer,
            _ => unreachable!(),
        }
    }
    fn charge(&self) -> u64 {
        match self {
            Self::Image { charge, .. } | Self::Buffer { charge, .. } => *charge,
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
    pub fn texture(&self) -> &wgpu::Texture {
        self.owner.resource.texture()
    }
}
struct Worker {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    options: WgpuContentOptions,
    capabilities: c::ContentCapabilities,
    pipelines: HashMap<PipelineKey, Arc<Pipeline>>,
    pipeline_order: VecDeque<PipelineKey>,
    exported: Arc<AtomicU64>,
    byte_copy: Option<(wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
}
pub struct WgpuContentExecutor {
    capabilities: c::ContentCapabilities,
    exported: Arc<AtomicU64>,
    #[cfg(not(target_family = "wasm"))]
    sender: std::sync::mpsc::SyncSender<Request>,
    #[cfg(target_family = "wasm")]
    local: std::rc::Rc<Local>,
}
impl WgpuContentExecutor {
    #[cfg(not(target_family = "wasm"))]
    pub fn new(options: WgpuContentOptions) -> Result<Self> {
        let (context, _) =
            crate::WgpuContext::new_headless().map_err(|e| execution(e.to_string()))?;
        Self::on_device(context.device, context.queue, options)
    }
    pub(crate) fn on_device(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        options: WgpuContentOptions,
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
        let capabilities = capabilities(&device);
        let exported = Arc::new(AtomicU64::new(0));
        let worker = Worker {
            device: device.clone(),
            queue,
            options: options.clone(),
            capabilities: capabilities.clone(),
            pipelines: HashMap::new(),
            pipeline_order: VecDeque::new(),
            byte_copy: None,
            exported: exported.clone(),
        };
        #[cfg(not(target_family = "wasm"))]
        {
            let (sender, receiver) = std::sync::mpsc::sync_channel(options.pending_jobs);
            std::thread::Builder::new()
                .name("wgpu-portable-content".into())
                .spawn(move || {
                    let mut worker = worker;
                    while let Ok(request) = receiver.recv() {
                        let Request {
                            graph,
                            limits,
                            state,
                            readback,
                        } = request;
                        let result = drive(
                            &device,
                            worker.execute(graph, limits, &state.cancelled, readback),
                        );
                        state.finish(result);
                    }
                })
                .map_err(|e| execution(e.to_string()))?;
            Ok(Self {
                sender,
                capabilities,
                exported,
            })
        }
        #[cfg(target_family = "wasm")]
        {
            Ok(Self {
                capabilities,
                exported,
                local: std::rc::Rc::new(Local {
                    worker: std::cell::RefCell::new(Some(worker)),
                    queue: std::cell::RefCell::new(VecDeque::new()),
                    running: std::cell::Cell::new(false),
                    capacity: options.pending_jobs,
                }),
            })
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
        limits: WgpuContentLimits,
    ) -> Result<WgpuContentJob> {
        self.enqueue(graph, limits, Arc::new(|| {}), true)
    }
    pub(crate) fn submit_gpu(
        &self,
        graph: Arc<c::ContentGraph>,
        limits: WgpuContentLimits,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<WgpuContentJob> {
        self.enqueue(graph, limits, wake, false)
    }
    fn enqueue(
        &self,
        graph: Arc<c::ContentGraph>,
        limits: WgpuContentLimits,
        wake: Arc<dyn Fn() + Send + Sync>,
        readback: bool,
    ) -> Result<WgpuContentJob> {
        let state = Arc::new(State {
            result: Mutex::new(None),
            done: Condvar::new(),
            waker: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            wake,
        });
        let request = Request {
            graph,
            limits,
            state: state.clone(),
            readback,
        };
        #[cfg(not(target_family = "wasm"))]
        self.sender.try_send(request).map_err(|e| match e {
            std::sync::mpsc::TrySendError::Full(_) => error(
                c::ContentErrorKind::ResourceLimit,
                "portable wgpu request queue is full",
            ),
            _ => execution("portable wgpu worker closed"),
        })?;
        #[cfg(target_family = "wasm")]
        {
            let local = self.local.clone();
            if local.queue.borrow().len() >= local.capacity {
                return Err(error(
                    c::ContentErrorKind::ResourceLimit,
                    "portable wgpu request queue is full",
                ));
            }
            local.queue.borrow_mut().push_back(request);
            if !local.running.replace(true) {
                wasm_bindgen_futures::spawn_local(async move {
                    loop {
                        let request = local.queue.borrow_mut().pop_front();
                        let Some(Request {
                            graph,
                            limits,
                            state,
                            readback,
                        }) = request
                        else {
                            local.running.set(false);
                            break;
                        };
                        let mut worker = local.worker.borrow_mut().take().unwrap();
                        let device = worker.device.clone();
                        let result = drive_browser(
                            &device,
                            worker.execute(graph, limits, &state.cancelled, readback),
                        )
                        .await;
                        *local.worker.borrow_mut() = Some(worker);
                        state.finish(result);
                    }
                });
            }
        }
        Ok(WgpuContentJob { state })
    }
}
#[cfg(target_family = "wasm")]
struct Local {
    worker: std::cell::RefCell<Option<Worker>>,
    queue: std::cell::RefCell<VecDeque<Request>>,
    running: std::cell::Cell<bool>,
    capacity: usize,
}
#[cfg(not(target_family = "wasm"))]
fn drive<F: Future<Output = Result<WgpuContentFrame>>>(
    device: &wgpu::Device,
    future: F,
) -> Result<WgpuContentFrame> {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Wake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return result;
        }
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(20)),
            })
            .map_err(|e| execution(format!("wgpu device polling failed: {e}")))?;
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
fn capabilities(device: &wgpu::Device) -> c::ContentCapabilities {
    let l = device.limits();
    let mut caps = c::ContentCapabilities::default();
    caps.max_image_dimension = l.max_texture_dimension_2d;
    caps.max_buffer_bytes = l.max_buffer_size;
    caps.compute = (l.max_compute_workgroup_size_x > 0
        && l.max_storage_buffers_per_shader_stage > 0)
        .then_some(c::ComputeLimits {
            max_workgroup_size: [
                l.max_compute_workgroup_size_x,
                l.max_compute_workgroup_size_y,
                l.max_compute_workgroup_size_z,
            ],
            max_workgroup_invocations: l.max_compute_invocations_per_workgroup,
            max_dispatch: [l.max_compute_workgroups_per_dimension; 3],
            max_workgroup_memory_bytes: u64::from(l.max_compute_workgroup_storage_size),
        });
    caps.storage_image_formats = if caps.compute.is_some() {
        vec![c::PixelFormat::Rgba8, c::PixelFormat::Rgba16Float]
    } else {
        Vec::new()
    };
    caps.bindings = c::BindingLimits {
        max_bind_groups: l.max_bind_groups,
        max_bindings_per_group: l.max_bindings_per_bind_group,
        max_bindings: ((l.max_storage_buffers_per_shader_stage
            + l.max_uniform_buffers_per_shader_stage
            + l.max_sampled_textures_per_shader_stage
            + l.max_samplers_per_shader_stage)
            * 3) as usize,
        max_uniform_bytes: u64::from(l.max_uniform_buffer_binding_size),
        max_storage_bytes: if l.max_storage_buffers_per_shader_stage == 0 {
            0
        } else {
            u64::from(l.max_storage_buffer_binding_size)
        },
        storage_offset_alignment: u64::from(l.min_storage_buffer_offset_alignment),
        max_vertex_buffers: l.max_vertex_buffers,
        max_vertex_attributes: l.max_vertex_attributes,
        max_vertex_stride: l.max_vertex_buffer_array_stride,
    };
    caps
}
struct Signal {
    done: AtomicBool,
    waker: Mutex<Option<Waker>>,
}
impl Signal {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            done: AtomicBool::new(false),
            waker: Mutex::new(None),
        })
    }
    fn finish(&self) {
        self.done.store(true, Ordering::Release);
        if let Some(w) = self.waker.lock().unwrap().take() {
            w.wake();
        }
    }
}
struct Completed(Arc<Signal>);
impl Future for Completed {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut waker = self.0.waker.lock().unwrap();
        if self.0.done.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            *waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// Retain exported-image reservations through compositor sampling completion.
pub(crate) fn retain_sampling(queue: &wgpu::Queue, images: Vec<GpuImage>) {
    if images.is_empty() {
        return;
    }
    #[cfg(not(target_family = "wasm"))]
    queue.on_submitted_work_done(move || drop(images));
    #[cfg(target_family = "wasm")]
    {
        let signal = Signal::new();
        SAMPLING.with(|pending| pending.borrow_mut().push((signal.clone(), images)));
        queue.on_submitted_work_done(move || {
            signal.finish();
            SAMPLING.with(|pending| {
                pending
                    .borrow_mut()
                    .retain(|(signal, _)| !signal.done.load(Ordering::Acquire))
            });
        });
    }
}
#[cfg(target_family = "wasm")]
thread_local! {static SAMPLING:std::cell::RefCell<Vec<(Arc<Signal>,Vec<GpuImage>)>>=const {std::cell::RefCell::new(Vec::new())};}

#[cfg(target_family = "wasm")]
async fn drive_browser<F: Future<Output = Result<WgpuContentFrame>>>(
    device: &wgpu::Device,
    future: F,
) -> Result<WgpuContentFrame> {
    use wasm_bindgen::{JsCast, closure::Closure};
    let mut future = std::pin::pin!(future);
    loop {
        let state = std::future::poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await;
        if let Poll::Ready(result) = state {
            return result;
        }
        // WebGPU uses completion promises; WebGL needs nonblocking device polls.
        // A timer yields the event loop in both cases and never waits for the GPU.
        device
            .poll(wgpu::PollType::Poll)
            .map_err(|e| execution(e.to_string()))?;
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let callback = Closure::once_into_js(move || {
                let _ = resolve.call0(&wasm_bindgen::JsValue::UNDEFINED);
            });
            if let Some(window) = web_sys::window() {
                if let Err(e) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.unchecked_ref(),
                    1,
                ) {
                    let _ = reject.call1(&wasm_bindgen::JsValue::UNDEFINED, &e);
                }
            } else {
                let _ = reject.call1(
                    &wasm_bindgen::JsValue::UNDEFINED,
                    &wasm_bindgen::JsValue::from_str("content execution requires a browser window"),
                );
            }
        });
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| execution(format!("browser content scheduler: {e:?}")))?;
    }
}

/// Deterministic validation gate; production execution has no gate or UI wait.
#[cfg(feature = "test-support")]
pub struct WgpuContentTestGate {
    entered: AtomicBool,
    released: Arc<Signal>,
}
#[cfg(feature = "test-support")]
impl std::fmt::Debug for WgpuContentTestGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WgpuContentTestGate")
    }
}
#[cfg(feature = "test-support")]
impl Default for WgpuContentTestGate {
    fn default() -> Self {
        Self {
            entered: AtomicBool::new(false),
            released: Signal::new(),
        }
    }
}
#[cfg(feature = "test-support")]
impl WgpuContentTestGate {
    pub fn release(&self) {
        self.released.finish();
    }
    #[cfg(not(target_family = "wasm"))]
    pub fn wait_until_entered(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while !self.entered.load(Ordering::Acquire) {
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }
    async fn enter(&self, cancelled: &AtomicBool) {
        self.entered.store(true, Ordering::Release);
        std::future::poll_fn(|cx| {
            if cancelled.load(Ordering::Acquire) {
                return Poll::Ready(());
            }
            Pin::new(&mut Completed(self.released.clone())).poll(cx)
        })
        .await;
    }
}
