#[cfg(any(target_os = "macos", feature = "wgpu"))]
use crate::upload::BackendPool;
use crate::{
    loader::Reservation,
    upload::{StagingPermit, UploadScheduler},
};
use anyhow::{Result, bail};
use gpui::{CustomContent, DevicePixels, ImageTextureData, Size};
use std::{
    any::Any,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// CPU preparation, host submission and GPU completion are distinct stages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadStatus {
    Idle,
    Queued,
    Preparing,
    Staged,
    Uploading,
    /// Upload commands have been submitted/executed by the host, ordered before
    /// sampling. This does not mean the GPU has completed them.
    Ready,
    Failed(String),
}
/// Immutable decoded image with bounded background GPU staging. Clones share
/// CPU data, a source-payload reservation and one backend/queue cache.
#[derive(Clone)]
pub struct MediaImage(Arc<Inner>);
struct Inner {
    data: ImageTextureData,
    source_color: crate::pipeline::PixelColor,
    uploads: AtomicUsize,
    reservation: Arc<Reservation>,
    state: Mutex<State>,
}
struct State {
    key: Option<(u8, u64)>,
    generation: u64,
    status: UploadStatus,
    staged: Option<Staged>,
    permit: Option<Arc<StagingPermit>>,
    output: Option<Output>,
    placeholder: Option<Output>,
}
impl State {
    fn new() -> Self {
        Self {
            key: None,
            generation: 0,
            status: UploadStatus::Idle,
            staged: None,
            permit: None,
            output: None,
            placeholder: None,
        }
    }
    fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.status = UploadStatus::Idle;
        self.staged = None;
        self.permit = None;
        self.output = None;
    }
    fn observe_submission(&mut self) {
        if self.output.as_ref().is_some_and(Output::is_submitted) {
            self.status = UploadStatus::Ready;
            self.staged = None;
            self.permit = None;
        }
    }
}
enum Output {
    #[cfg(target_os = "macos")]
    Metal(gpui_apple::custom_draw::MetalImageOutput),
    #[cfg(feature = "wgpu")]
    Wgpu(gpui_wgpu::custom_draw::WgpuCustomOutput),
    #[cfg(target_os = "windows")]
    Dx11(gpui_windows::custom_draw::Dx11CustomOutput),
}
impl Output {
    fn is_submitted(&self) -> bool {
        match self {
            #[cfg(target_os = "macos")]
            Self::Metal(output) => output.is_submitted(),
            #[cfg(feature = "wgpu")]
            Self::Wgpu(output) => output.is_submitted(),
            #[cfg(target_os = "windows")]
            Self::Dx11(output) => output.is_submitted(),
        }
    }
    fn into_any(self) -> Box<dyn Any> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Metal(output) => Box::new(output),
            #[cfg(feature = "wgpu")]
            Self::Wgpu(output) => Box::new(output),
            #[cfg(target_os = "windows")]
            Self::Dx11(output) => Box::new(output),
        }
    }
}
enum Staged {
    #[cfg(target_os = "macos")]
    Metal(gpui_apple::custom_draw::MetalImageOutput),
    #[cfg(feature = "wgpu")]
    Wgpu(gpui_wgpu::custom_draw::WgpuStagedImage),
    #[cfg(target_os = "windows")]
    Dx11(gpui_windows::custom_draw::Dx11StagedImage),
}
enum Worker {
    #[cfg(target_os = "macos")]
    Metal(
        gpui_apple::custom_draw::MetalCustomContext,
        gpui_apple::custom_draw::MetalImageStagingPool,
    ),
    #[cfg(feature = "wgpu")]
    Wgpu(
        gpui_wgpu::custom_draw::WgpuImageUploadContext,
        gpui_wgpu::custom_draw::WgpuImageStagingPool,
    ),
    #[cfg(target_os = "windows")]
    Dx11(gpui_windows::custom_draw::Dx11ImageUploadContext),
}
impl Worker {
    fn stage(self, data: &ImageTextureData, guard: Arc<dyn Send + Sync>) -> Result<Staged> {
        let _ = &guard;
        match self {
            #[cfg(target_os = "macos")]
            Self::Metal(context, pool) => Ok(Staged::Metal(context.stage_image_upload_with_pool(
                data,
                &pool,
                Some(guard),
            )?)),
            #[cfg(feature = "wgpu")]
            Self::Wgpu(context, pool) => Ok(Staged::Wgpu(context.stage_with_pool(data, &pool)?)),
            #[cfg(target_os = "windows")]
            Self::Dx11(context) => Ok(Staged::Dx11(context.stage(data)?)),
        }
    }
}
enum Host<'a> {
    #[cfg(target_os = "macos")]
    Metal(&'a gpui_apple::custom_draw::MetalCustomContext),
    #[cfg(feature = "wgpu")]
    Wgpu(&'a gpui_wgpu::custom_draw::WgpuCustomContext),
    #[cfg(target_os = "windows")]
    Dx11(&'a gpui_windows::custom_draw::Dx11CustomContext),
    #[cfg(not(any(target_os = "macos", target_os = "windows", feature = "wgpu")))]
    Unsupported(std::marker::PhantomData<&'a ()>),
}
impl Host<'_> {
    fn key(&self) -> (u8, u64) {
        match self {
            #[cfg(target_os = "macos")]
            Self::Metal(context) => (0, context.cache_key()),
            #[cfg(feature = "wgpu")]
            Self::Wgpu(context) => (1, context.cache_key() as u64),
            #[cfg(target_os = "windows")]
            Self::Dx11(context) => (2, context.cache_key() as u64),
            #[cfg(not(any(target_os = "macos", target_os = "windows", feature = "wgpu")))]
            Self::Unsupported(_) => unreachable!(),
        }
    }
    fn worker(&self, scheduler: &UploadScheduler) -> Worker {
        let mut cached = scheduler.pool.lock().unwrap();
        match self {
            #[cfg(target_os = "macos")]
            Self::Metal(context) => {
                if !matches!(cached.as_ref(),Some(BackendPool::Metal(key,_)) if *key == self.key().1)
                {
                    *cached = Some(BackendPool::Metal(
                        self.key().1,
                        context.image_staging_pool(scheduler.pool_capacity()),
                    ));
                }
                let Some(BackendPool::Metal(_, pool)) = cached.as_ref() else {
                    unreachable!()
                };
                Worker::Metal((*context).clone(), pool.clone())
            }
            #[cfg(feature = "wgpu")]
            Self::Wgpu(context) => {
                if !matches!(cached.as_ref(),Some(BackendPool::Wgpu(key,_)) if *key == self.key().1)
                {
                    *cached = Some(BackendPool::Wgpu(
                        self.key().1,
                        context
                            .image_upload_context()
                            .staging_pool(scheduler.pool_capacity()),
                    ));
                }
                let Some(BackendPool::Wgpu(_, pool)) = cached.as_ref() else {
                    unreachable!()
                };
                Worker::Wgpu(context.image_upload_context(), pool.clone())
            }
            #[cfg(target_os = "windows")]
            Self::Dx11(context) => {
                *cached = None;
                Worker::Dx11(context.image_upload_context())
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows", feature = "wgpu")))]
            Self::Unsupported(_) => unreachable!(),
        }
    }
    fn reuse(&self, output: &Output) -> Result<Option<Output>> {
        match (self, output) {
            #[cfg(target_os = "macos")]
            (Self::Metal(context), Output::Metal(output)) => {
                Ok(Some(Output::Metal(output.reusable_for(context)?)))
            }
            #[cfg(feature = "wgpu")]
            (Self::Wgpu(context), Output::Wgpu(output)) if context.can_reuse(output) => {
                Ok(Some(Output::Wgpu(output.clone())))
            }
            #[cfg(target_os = "windows")]
            (Self::Dx11(context), Output::Dx11(output)) if context.can_reuse(output) => {
                Ok(Some(Output::Dx11(output.clone())))
            }
            #[allow(unreachable_patterns)]
            _ => Ok(None),
        }
    }
    fn adopt(&self, staged: &Staged, guard: Arc<dyn Send + Sync>) -> Result<Output> {
        let _ = &guard;
        match (self, staged) {
            #[cfg(target_os = "macos")]
            (Self::Metal(context), Staged::Metal(output)) => {
                Ok(Output::Metal(output.reusable_for(context)?))
            }
            #[cfg(feature = "wgpu")]
            (Self::Wgpu(context), Staged::Wgpu(staged)) => Ok(Output::Wgpu(
                context.adopt_image_upload_with_guard(staged, Some(guard))?,
            )),
            #[cfg(target_os = "windows")]
            (Self::Dx11(context), Staged::Dx11(staged)) => {
                Ok(Output::Dx11(context.adopt_image_upload(staged)?))
            }
            #[allow(unreachable_patterns)]
            _ => bail!("staged image backend changed"),
        }
    }
    fn placeholder(&self, format: gpui::ImageTextureFormat) -> Result<Output> {
        let data = match format {
            gpui::ImageTextureFormat::LinearRec2020Rgba16Float => {
                ImageTextureData::new_linear_rec2020_rgba16f(1, 1, vec![vec![0; 8]])?
            }
            gpui::ImageTextureFormat::Rgba8Srgb => ImageTextureData::new(1, 1, vec![vec![0; 4]])?,
            gpui::ImageTextureFormat::LinearP3Rgba16Float => {
                ImageTextureData::new_linear_p3_rgba16f(1, 1, vec![vec![0; 8]])?
            }
        };
        match self {
            #[cfg(target_os = "macos")]
            Self::Metal(context) => Ok(Output::Metal(context.create_image_output(&data)?)),
            #[cfg(feature = "wgpu")]
            Self::Wgpu(context) => Ok(Output::Wgpu(context.create_image_output(&data)?)),
            #[cfg(target_os = "windows")]
            Self::Dx11(context) => Ok(Output::Dx11(context.create_image_output(&data)?)),
            #[cfg(not(any(target_os = "macos", target_os = "windows", feature = "wgpu")))]
            Self::Unsupported(_) => unreachable!(),
        }
    }
}
impl std::fmt::Debug for MediaImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaImage")
            .field("width", &self.width())
            .field("height", &self.height())
            .finish()
    }
}
impl MediaImage {
    pub(crate) fn new_with_color(
        data: ImageTextureData,
        reservation: Arc<Reservation>,
        source_color: crate::pipeline::PixelColor,
    ) -> Self {
        Self(Arc::new(Inner {
            data,
            source_color,
            uploads: AtomicUsize::new(0),
            reservation,
            state: Mutex::new(State::new()),
        }))
    }
    /// Original supplied pixel interpretation, before canonical upload conversion.
    pub fn source_color(&self) -> crate::pipeline::PixelColor {
        self.0.source_color
    }
    pub fn format(&self) -> gpui::ImageTextureFormat {
        self.0.data.format()
    }
    pub fn width(&self) -> u32 {
        self.0.data.width()
    }
    pub fn height(&self) -> u32 {
        self.0.data.height()
    }
    pub fn mip_count(&self) -> usize {
        self.0.data.levels().len()
    }
    /// Successful background preparations, including replacement devices/queues.
    pub fn upload_count(&self) -> usize {
        self.0.uploads.load(Ordering::Relaxed)
    }
    pub fn payload_bytes(&self) -> usize {
        self.0.data.byte_len()
    }
    pub fn upload_status(&self) -> UploadStatus {
        let mut state = self.0.state.lock().unwrap();
        state.observe_submission();
        state.status.clone()
    }
    /// Retry a failed preparation or invalidate an upload explicitly. Results
    /// from a previous generation are discarded, never published into this cache.
    pub fn retry_upload(&self) {
        self.0.state.lock().unwrap().reset();
    }
    fn staging_bytes(&self) -> usize {
        self.0.data.staging_byte_len()
    }
    fn start(&self, state: &mut State, worker: Worker) -> Result<()> {
        let Some((active, staging)) = self.0.reservation.uploads().reserve(self.staging_bytes())?
        else {
            state.status = UploadStatus::Queued;
            return Ok(());
        };
        let staging = Arc::new(staging);
        state.status = UploadStatus::Preparing;
        let generation = state.generation;
        let weak = Arc::downgrade(&self.0);
        let data = self.0.data.clone();
        let reservation = self.0.reservation.clone();
        std::thread::Builder::new()
            .name("xgpui-media-upload".into())
            .spawn(move || {
                let _reservation = reservation.clone();
                // Catch a backend panic so it cannot leave the image spinning forever.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if weak.strong_count() == 0 {
                        bail!("upload cancelled");
                    }
                    worker.stage(&data, Arc::new((staging.clone(), reservation)))
                }))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("image staging worker panicked")));
                drop(active);
                if let Some(inner) = weak.upgrade() {
                    let mut state = inner.state.lock().unwrap();
                    if state.generation != generation {
                        return;
                    }
                    match result {
                        Ok(result) => {
                            inner.uploads.fetch_add(1, Ordering::Relaxed);
                            state.staged = Some(result);
                            state.permit = Some(staging);
                            state.status = UploadStatus::Staged;
                        }
                        Err(error) => state.status = UploadStatus::Failed(format!("{error:#}")),
                    }
                }
            })?;
        Ok(())
    }
    fn prepare_for(&self, host: Host<'_>, frame: &gpui::PreparationFrame) -> Result<Box<dyn Any>> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("image cache poisoned"))?;
        if state.key != Some(host.key()) {
            state.reset();
            state.placeholder = None;
            state.key = Some(host.key());
        }
        state.observe_submission();
        if let Some(output) = state.output.as_ref()
            && let Some(output) = host.reuse(output)?
        {
            return Ok(output.into_any());
        }
        if let Some(staged) = state.staged.as_ref()
            && self
                .0
                .reservation
                .uploads()
                .admit_frame(frame, self.staging_bytes())
        {
            let output = host.adopt(
                staged,
                Arc::new((
                    state.permit.as_ref().unwrap().clone(),
                    self.0.reservation.clone(),
                )),
            )?;
            state.output = Some(output);
            state.status = UploadStatus::Uploading;
            return Ok(host
                .reuse(state.output.as_ref().unwrap())?
                .unwrap()
                .into_any());
        }
        if matches!(state.status, UploadStatus::Idle | UploadStatus::Queued)
            && let Err(error) = (|| -> Result<()> {
                let scheduler = self.0.reservation.uploads();
                scheduler.validate_frame_size(self.staging_bytes())?;
                self.start(&mut state, host.worker(&scheduler))
            })()
        {
            state.status = UploadStatus::Failed(format!("{error:#}"));
        }
        if let Some(output) = state.placeholder.as_ref()
            && let Some(output) = host.reuse(output)?
        {
            return Ok(output.into_any());
        }
        state.placeholder = Some(host.placeholder(self.0.data.format())?);
        Ok(host
            .reuse(state.placeholder.as_ref().unwrap())?
            .unwrap()
            .into_any())
    }
}
impl CustomContent for MediaImage {
    fn image_color_mode(&self) -> gpui::ImageColorMode {
        match self.format() {
            gpui::ImageTextureFormat::LinearRec2020Rgba16Float => {
                gpui::ImageColorMode::LinearRec2020Preview
            }
            gpui::ImageTextureFormat::Rgba8Srgb => gpui::ImageColorMode::EncodedSrgb,
            gpui::ImageTextureFormat::LinearP3Rgba16Float => gpui::ImageColorMode::LinearP3Preview,
        }
    }

    fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let request =
            gpui::PreparationRequest::new(extent.map(|v| gpui::px(v.0 as f32)), extent, 1.)?;
        self.prepare_in_frame(backend, &request, &gpui::PreparationFrame::default())
    }
    fn prepare_in_frame(
        &self,
        backend: &dyn Any,
        _request: &gpui::PreparationRequest,
        frame: &gpui::PreparationFrame,
    ) -> Result<Box<dyn Any>> {
        #[cfg(target_os = "macos")]
        if let Some(context) = backend.downcast_ref::<gpui_apple::custom_draw::MetalCustomContext>()
        {
            return self.prepare_for(Host::Metal(context), frame);
        }
        #[cfg(feature = "wgpu")]
        if let Some(context) = backend.downcast_ref::<gpui_wgpu::custom_draw::WgpuCustomContext>() {
            return self.prepare_for(Host::Wgpu(context), frame);
        }
        #[cfg(target_os = "windows")]
        if let Some(context) =
            backend.downcast_ref::<gpui_windows::custom_draw::Dx11CustomContext>()
        {
            return self.prepare_for(Host::Dx11(context), frame);
        }
        let _ = backend;
        bail!("media images require Metal, DirectX 11 or the optional wgpu backend")
    }
}
