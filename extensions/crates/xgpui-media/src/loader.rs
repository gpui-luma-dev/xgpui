use crate::upload::UploadScheduler;
use crate::{FrameUploadLimits, MediaImage, UploadLimits, UploadStats, mipmaps};
use anyhow::{Context, Result, ensure};
use std::{
    io::{Cursor, Read},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Conservative admission limits shared by all clones of a loader. Payload
/// budgets cover retained mip bytes, not decoder scratch or driver allocations.
#[derive(Clone, Copy, Debug)]
pub struct MediaLimits {
    pub max_jobs: usize,
    pub max_resident_bytes: usize,
    pub max_image_bytes: usize,
    pub max_encoded_bytes: usize,
}
impl Default for MediaLimits {
    fn default() -> Self {
        Self {
            max_jobs: 2,
            max_resident_bytes: 256 * 1024 * 1024,
            max_image_bytes: 64 * 1024 * 1024,
            max_encoded_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Default)]
struct Usage {
    jobs: usize,
    bytes: usize,
}
struct Pool {
    uploads: Arc<UploadScheduler>,
    limits: MediaLimits,
    usage: Mutex<Usage>,
}
/// Bounded native background image loading. Admission returns an error instead
/// of creating an unbounded task queue. Keep one shared loader per application.
#[derive(Clone)]
pub struct MediaLoader(Arc<Pool>);
impl Default for MediaLoader {
    fn default() -> Self {
        Self::new(MediaLimits::default()).unwrap()
    }
}
/// A retained reservation travels with every clone of the resulting image.
/// Reserve the per-image maximum before decode; conservative unused capacity is
/// released only when the job and all image snapshots have been dropped.
pub(crate) struct Reservation {
    pool: Arc<Pool>,
    _guard: Option<Arc<dyn Send + Sync>>,
}
impl Reservation {
    pub(crate) fn uploads(&self) -> Arc<UploadScheduler> {
        self.pool.uploads.clone()
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.pool.usage.lock().unwrap().bytes -= self.pool.limits.max_image_bytes;
    }
}
struct ActiveJob(Arc<Pool>);
impl Drop for ActiveJob {
    fn drop(&mut self) {
        self.0.usage.lock().unwrap().jobs -= 1;
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadStatus {
    Loading,
    /// CPU decode/mips complete. First paint starts background GPU staging;
    /// observe `MediaImage::upload_status` for staging and host submission.
    Ready,
    Failed(String),
    Cancelled,
}
struct JobState {
    status: LoadStatus,
    image: Option<MediaImage>,
}
/// Poll from a GPUI task or host view; no blocking join is performed on drop.
/// Dropping/cancelling a job suppresses publication. Codec decoding is bounded
/// but is not forcibly interrupted; the worker releases its slot on return.
pub struct ImageJob {
    state: Arc<Mutex<JobState>>,
    cancelled: Arc<AtomicBool>,
}
impl ImageJob {
    pub fn status(&self) -> LoadStatus {
        self.state.lock().unwrap().status.clone()
    }
    /// Take the completed image once. Inspect `status` for failures.
    pub fn try_take(&mut self) -> Option<MediaImage> {
        self.state.lock().unwrap().image.take()
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap();
        state.image.take();
        state.status = LoadStatus::Cancelled;
    }
}
impl Drop for ImageJob {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl MediaLoader {
    pub fn new(limits: MediaLimits) -> Result<Self> {
        Self::with_upload_limits(limits, UploadLimits::default())
    }
    /// Configure independent decode/payload and background-staging admission.
    pub fn with_upload_limits(limits: MediaLimits, uploads: UploadLimits) -> Result<Self> {
        Self::with_upload_policy(limits, uploads, FrameUploadLimits::default())
    }
    /// Configure decode, staging, and per-frame copy admission independently.
    pub fn with_upload_policy(
        limits: MediaLimits,
        uploads: UploadLimits,
        frames: FrameUploadLimits,
    ) -> Result<Self> {
        ensure!(
            limits.max_jobs > 0
                && limits.max_image_bytes > 0
                && limits.max_image_bytes <= gpui::ImageTextureData::MAX_BYTES
                && limits.max_resident_bytes >= limits.max_image_bytes
                && limits.max_encoded_bytes > 0
                && limits.max_encoded_bytes <= 64 * 1024 * 1024,
            "invalid media limits"
        );
        Ok(Self(Arc::new(Pool {
            uploads: UploadScheduler::with_frame_limits(uploads, frames)?,
            limits,
            usage: Mutex::new(Usage::default()),
        })))
    }
    /// Current active workers and outstanding staging reservations.
    pub fn upload_stats(&self) -> UploadStats {
        self.0.uploads.stats()
    }
    /// Current queue's Metal/wgpu idle-cache allocation and reuse evidence.
    /// D3D11 staging is not pooled yet and reports zero cache statistics.
    pub fn staging_pool_stats(&self) -> gpui::ImageStagingPoolStats {
        self.0.uploads.pool_stats()
    }
    /// Decode PNG/JPEG/WebP's first frame on a background worker and prepare a
    /// full mip chain. Input is interpreted as SDR sRGB; ICC/HDR conversion and
    /// orientation metadata are outside this first API.
    pub fn load(&self, path: impl Into<PathBuf>) -> Result<ImageJob> {
        let path = path.into();
        self.load_pixels(
            crate::pipeline::ImageColor::SRGB,
            move |limits, cancelled| {
                let mut encoded = Vec::new();
                std::fs::File::open(path)?
                    .take(limits.max_encoded_bytes as u64 + 1)
                    .read_to_end(&mut encoded)?;
                ensure!(
                    encoded.len() <= limits.max_encoded_bytes,
                    "encoded image exceeds limit"
                );
                let (width, height) = image::ImageReader::new(Cursor::new(&encoded))
                    .with_guessed_format()?
                    .into_dimensions()?;
                ensure!(
                    mipmaps::payload_bytes(width, height)? <= limits.max_image_bytes,
                    "image exceeds payload limit"
                );
                ensure!(!cancelled.load(Ordering::Acquire), "image load cancelled");
                let mut reader =
                    image::ImageReader::new(Cursor::new(&encoded)).with_guessed_format()?;
                let mut decode_limits = image::Limits::default();
                decode_limits.max_image_width = Some(16384);
                decode_limits.max_image_height = Some(16384);
                decode_limits.max_alloc = Some(limits.max_image_bytes as u64 * 2);
                reader.limits(decode_limits);
                let rgba = reader.decode()?.into_rgba8().into_raw();
                drop(encoded);
                let data =
                    mipmaps::build(width, height, rgba, || cancelled.load(Ordering::Acquire))?;
                Ok(data)
            },
        )
    }
    /// Convert supplied bounded SDR pixels on an admitted background worker.
    /// Source metadata is explicit; no RAW/TIFF engine or system decoder needed.
    /// Clips to bounded linear P3; use `load_rgba16f_with_mapping` to select a
    /// mapping policy. Out-of-P3 Adobe RGB colors are not retained.
    pub fn load_rgba16f(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        color: crate::pipeline::ImageColor,
    ) -> Result<ImageJob> {
        self.load_rgba16f_with_mapping(
            width,
            height,
            pixels,
            color,
            crate::pipeline::GamutMapping::Clip,
        )
    }
    /// Background conversion with explicit gamut mapping to bounded linear P3.
    /// Mapping precedes association and mip generation. No ICC inference.
    pub fn load_rgba16f_with_mapping(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        color: crate::pipeline::ImageColor,
        mapping: crate::pipeline::GamutMapping,
    ) -> Result<ImageJob> {
        self.load_rgba16f_in(
            width,
            height,
            pixels,
            color,
            mapping,
            crate::pipeline::ImagePrimaries::DisplayP3,
        )
    }
    /// Background conversion to bounded associated linear Rec.2020 SDR and mips.
    /// Retains Adobe RGB/P3 source gamuts; no ICC inference or native wide placement.
    pub fn load_rec2020_rgba16f(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        color: crate::pipeline::ImageColor,
        mapping: crate::pipeline::GamutMapping,
    ) -> Result<ImageJob> {
        self.load_rgba16f_in(
            width,
            height,
            pixels,
            color,
            mapping,
            crate::pipeline::ImagePrimaries::Rec2020,
        )
    }
    fn load_rgba16f_in(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        color: crate::pipeline::ImageColor,
        mapping: crate::pipeline::GamutMapping,
        working: crate::pipeline::ImagePrimaries,
    ) -> Result<ImageJob> {
        ensure!(
            crate::pipeline::rgba16f_payload_bytes(width, height)? <= self.0.limits.max_image_bytes,
            "image exceeds payload limit"
        );
        ensure!(
            pixels.len() == width as usize * height as usize
                && pixels.len().saturating_mul(16) <= self.0.limits.max_encoded_bytes,
            "supplied pixels exceed input limit"
        );
        self.load_pixels(color, move |_, cancelled| {
            crate::pipeline::float_image::build_in(
                width,
                height,
                pixels,
                color,
                mapping,
                working,
                || cancelled.load(Ordering::Acquire),
            )
        })
    }
    fn load_pixels(
        &self,
        color: crate::pipeline::ImageColor,
        build: impl FnOnce(MediaLimits, &AtomicBool) -> Result<gpui::ImageTextureData> + Send + 'static,
    ) -> Result<ImageJob> {
        self.load_guarded(color, None, build)
    }
    pub(crate) fn load_guarded(
        &self,
        color: crate::pipeline::ImageColor,
        guard: Option<Arc<dyn Send + Sync>>,
        build: impl FnOnce(MediaLimits, &AtomicBool) -> Result<gpui::ImageTextureData> + Send + 'static,
    ) -> Result<ImageJob> {
        {
            let mut usage = self.0.usage.lock().unwrap();
            ensure!(usage.jobs < self.0.limits.max_jobs, "media workers busy");
            ensure!(
                self.0.limits.max_image_bytes <= self.0.limits.max_resident_bytes - usage.bytes,
                "media payload budget exhausted"
            );
            usage.jobs += 1;
            usage.bytes += self.0.limits.max_image_bytes;
        }
        let reservation = Arc::new(Reservation {
            pool: self.0.clone(),
            _guard: guard,
        });
        let active = ActiveJob(self.0.clone());
        let job = ImageJob {
            state: Arc::new(Mutex::new(JobState {
                status: LoadStatus::Loading,
                image: None,
            })),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let state = Arc::downgrade(&job.state);
        let cancelled = job.cancelled.clone();
        let limits = self.0.limits;
        std::thread::Builder::new()
            .name("xgpui-media".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<MediaImage> {
                        ensure!(!cancelled.load(Ordering::Acquire), "image load cancelled");
                        let data = build(limits, &cancelled)?;
                        Ok(MediaImage::new_with_color(data, reservation, color))
                    },
                ))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("image provider panicked")));
                drop(active);
                if let Some(state) = state.upgrade() {
                    let mut state = state.lock().unwrap();
                    if cancelled.load(Ordering::Acquire) {
                        return;
                    }
                    match result {
                        Ok(image) => {
                            state.image = Some(image);
                            state.status = LoadStatus::Ready;
                        }
                        Err(error) => state.status = LoadStatus::Failed(format!("{error:#}")),
                    }
                }
            })
            .context("start media worker")?;
        Ok(job)
    }
}
