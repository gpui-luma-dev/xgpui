//! Provider-driven immutable pages. No whole-image allocation or codec engine.
use super::*;
use anyhow::{Result, ensure};
use gpui::{ImageTextureData, ImageTextureFormat};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Providers produce only the requested mip rectangle, including its clipped
/// halo. Return one canonical immutable page, checking cancellation by row.
/// Provider/codec internals must obey the declared scratch bound separately.
pub trait TileProvider: std::fmt::Debug + Send + Sync + 'static {
    fn source(&self) -> SourceId;
    fn image(&self) -> ImageDescriptor;
    fn tile(&self, tile: TileDescriptor, cancelled: &AtomicBool) -> Result<ImageTextureData>;
}

/// Logical viewport and source-pixel camera. Scale is logical pixels per source
/// pixel; device_scale is used only for pyramid level selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasView {
    pub center: [f64; 2],
    pub scale: f64,
    pub viewport: [f64; 2],
    pub device_scale: f64,
}
impl CanvasView {
    fn validate(self) -> Result<()> {
        ensure!(
            self.center.iter().all(|v| v.is_finite())
                && self.scale.is_finite()
                && (1e-9..=1024.).contains(&self.scale)
                && self.device_scale.is_finite()
                && (0.1..=16.).contains(&self.device_scale)
                && self
                    .viewport
                    .iter()
                    .all(|v| v.is_finite() && *v > 0. && *v <= 32768.),
            "invalid canvas viewport"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct CanvasOptions {
    pub layout: TileLayout,
    pub budgets: TileBudgets,
    /// Additional provider scratch, reserved for each active job.
    pub scratch_bytes_per_job: u64,
    pub max_cached_pages: usize,
}
impl Default for CanvasOptions {
    fn default() -> Self {
        Self {
            layout: TileLayout::new(256, 1).unwrap(),
            budgets: TileBudgets {
                cpu_bytes: 16 * 1024 * 1024,
                gpu_bytes: 16 * 1024 * 1024,
                staging_bytes: 4 * 1024 * 1024,
                upload_bytes_per_frame: 1024 * 1024,
                uploads_per_frame: 2,
                decode_jobs: 2,
            },
            scratch_bytes_per_job: 512 * 1024,
            max_cached_pages: 48,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct CanvasStats {
    pub cpu_bytes: u64,
    /// Conservative reserved payload, including old snapshots/in-flight frames.
    pub gpu_bytes: u64,
    /// Conservative provider-output transfer reservation; native GPU staging
    /// and completion are tracked separately by the renderer.
    pub pending_staging_bytes: u64,
    pub active_jobs: u32,
    pub cached_pages: usize,
    pub visible_pages: usize,
    /// CPU-decoded visible pages; not a GPU readiness count.
    pub decoded_pages: usize,
    pub mip: u32,
    pub evictions: u64,
    pub stale_results: u64,
    pub cancelled_requests: u64,
    pub failures: u64,
    pub requests: u64,
}
#[derive(Debug, Default)]
struct Usage {
    cpu: u64,
    gpu: u64,
    staging: u64,
    jobs: u32,
}
#[derive(Debug)]
struct Lease {
    usage: Arc<Mutex<Usage>>,
    bytes: u64,
    staging: u64,
    scratch: u64,
    working: AtomicBool,
}
impl Lease {
    fn finish(&self) {
        if self.working.swap(false, Ordering::AcqRel) {
            let mut u = self.usage.lock().unwrap();
            u.cpu -= self.scratch;
            u.staging -= self.staging;
            u.jobs -= 1;
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.finish();
        let mut u = self.usage.lock().unwrap();
        u.cpu -= self.bytes;
        u.gpu -= self.bytes;
    }
}
#[derive(Debug)]
pub struct CanvasPage {
    tile: TileDescriptor,
    _lease: Arc<Lease>,
    content: gpui::content::Content,
}
impl CanvasPage {
    pub fn tile(&self) -> TileDescriptor {
        self.tile
    }
    /// Immutable source-sized page pixels. GPU readiness belongs to the renderer.
    pub fn draw(&self) -> gpui::content::ContentDraw {
        gpui::content::ContentDraw::new(self.content.clone())
            .color_output(gpui::content::ContentColorOutput::SrgbPreview)
    }
}
/// Immutable page-table snapshot. Scene/GPU ownership pins its pages and their
/// budget leases. Missing/refining pages reveal the retained low-resolution preview.
#[derive(Debug)]
pub struct CanvasSnapshot {
    group: gpui::content::ContentGroup,
    pub source: SourceId,
    pub image: ImageDescriptor,
    pub view: CanvasView,
    pub preview: Option<Arc<CanvasPage>>,
    pub pages: Vec<Arc<CanvasPage>>,
}
type Placement = (
    gpui::Bounds<gpui::Pixels>,
    gpui::Bounds<gpui::Pixels>,
    gpui::content::ContentDraw,
);
impl CanvasSnapshot {
    fn placements(&self, bounds: gpui::Bounds<gpui::Pixels>) -> Vec<Placement> {
        use gpui::{Bounds, point, px, size};
        let scale = self.view.scale as f32;
        let origin = bounds.center()
            - point(
                px(self.view.center[0] as f32 * scale),
                px(self.view.center[1] as f32 * scale),
            );
        let geometry = |page: &CanvasPage| {
            let [mw, mh] = self
                .image
                .mip_size(page.tile.key().mip)
                .expect("validated mip");
            let sx = self.image.width() as f32 / mw as f32 * scale;
            let sy = self.image.height() as f32 / mh as f32 * scale;
            let rect = |r: PixelRect| {
                Bounds::new(
                    origin + point(px(r.x as f32 * sx), px(r.y as f32 * sy)),
                    size(px(r.width as f32 * sx), px(r.height as f32 * sy)),
                )
            };
            (
                rect(page.tile.upload()),
                rect(page.tile.content()).intersect(&bounds),
            )
        };
        let placement =
            |page: &CanvasPage, destination: Bounds<gpui::Pixels>, clip: Bounds<gpui::Pixels>| {
                use gpui::content::{ImageCrop, ImageMapping};
                let upload = page.tile.upload();
                // Bound destination geometry without creating a filter boundary:
                // source viewport sampling retains the full page and its halo.
                let visible = destination.intersect(&clip);
                let axis = |start: f32, end: f32, base: f32, size: f32, extent: u32| {
                    let lo = ((start - base) / size * extent as f32).clamp(0., extent as f32);
                    let hi = ((end - base) / size * extent as f32).clamp(lo, extent as f32);
                    (lo, hi - lo)
                };
                let (x, w) = axis(
                    f32::from(visible.left()),
                    f32::from(visible.right()),
                    f32::from(destination.left()),
                    f32::from(destination.size.width),
                    upload.width,
                );
                let (y, h) = axis(
                    f32::from(visible.top()),
                    f32::from(visible.bottom()),
                    f32::from(destination.top()),
                    f32::from(destination.size.height),
                    upload.height,
                );
                let crop = ImageCrop::new([x, y], [w, h]).expect("visible page crop");
                (
                    visible,
                    clip,
                    page.draw()
                        .mapping(ImageMapping::default().viewport(Some(crop))),
                )
            };
        let mut placements = Vec::new();
        if let Some(preview) = &self.preview {
            let (destination, clip) = geometry(preview);
            let mut holes = vec![clip];
            // Refined translucent tiles REPLACE preview regions. Drawing them
            // over the preview would incorrectly associate alpha twice.
            for page in self.pages.iter() {
                let (_, cut) = geometry(page);
                holes = holes.into_iter().flat_map(|r| subtract(r, cut)).collect();
            }
            for clip in holes {
                if !clip.is_empty() {
                    placements.push(placement(preview, destination, clip));
                }
            }
        }
        for page in &self.pages {
            let (destination, clip) = geometry(page);
            if !clip.is_empty() {
                placements.push(placement(page, destination, clip));
            }
        }
        placements
    }
    /// Atomic portable presentation snapshot; member coordinates are viewport-local.
    pub fn content_group(&self) -> gpui::content::ContentGroup {
        self.group.clone()
    }
    pub fn paint(&self, bounds: gpui::Bounds<gpui::Pixels>, window: &mut gpui::Window) {
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            window.paint_content_group(bounds, self.content_group());
        });
    }
    fn build_group(&self, id: gpui::content::ContentGroupId) -> gpui::content::ContentGroup {
        let mut group = gpui::content::ContentGroup::new(id);
        let bounds = gpui::Bounds::new(
            gpui::point(gpui::px(0.), gpui::px(0.)),
            gpui::size(
                gpui::px(self.view.viewport[0] as f32),
                gpui::px(self.view.viewport[1] as f32),
            ),
        );
        for (destination, clip, draw) in self.placements(bounds) {
            group.push_clipped(
                gpui::CustomGeometry::RoundedRect {
                    bounds: destination,
                    corner_radii: gpui::Corners::default(),
                },
                clip,
                draw,
            );
        }
        group
    }
}
fn subtract(
    r: gpui::Bounds<gpui::Pixels>,
    cut: gpui::Bounds<gpui::Pixels>,
) -> Vec<gpui::Bounds<gpui::Pixels>> {
    use gpui::{Bounds, point, size};
    let c = r.intersect(&cut);
    if c.is_empty() {
        return vec![r];
    }
    let rect = |x, y, w, h| Bounds::new(point(x, y), size(w, h));
    vec![
        rect(r.left(), r.top(), r.size.width, c.top() - r.top()),
        rect(r.left(), c.bottom(), r.size.width, r.bottom() - c.bottom()),
        rect(r.left(), c.top(), c.left() - r.left(), c.size.height),
        rect(c.right(), c.top(), r.right() - c.right(), c.size.height),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

struct Pending {
    tile: TileDescriptor,
    #[cfg(not(target_arch = "wasm32"))]
    revision: u64,
    preview: bool,
    #[cfg(not(target_arch = "wasm32"))]
    lease: Arc<Lease>,
    #[cfg(not(target_arch = "wasm32"))]
    job: crate::ImageJob,
}
struct Cached {
    page: Arc<CanvasPage>,
    used: u64,
}
/// Bounded viewport scheduler and progressive publication. Native providers run
/// on admitted workers; Wasm generates at most one page per advance on its host
/// thread. Hosts call advance once per frame and arrange redraws while loading.
pub struct TileCanvas {
    provider: Arc<dyn TileProvider>,
    options: CanvasOptions,
    pyramid: TilePyramid,
    usage: Arc<Mutex<Usage>>,
    cache: HashMap<TileKey, Cached>,
    pending: Vec<Pending>,
    preview: Option<Arc<CanvasPage>>,
    desired: Vec<TileDescriptor>,
    view: Option<CanvasView>,
    revision: u64,
    clock: u64,
    stats: CanvasStats,
    stopped: bool,
    progressive: ProgressiveImage<CanvasSnapshot>,
    failed: HashSet<TileKey>,
    group_id: gpui::content::ContentGroupId,
    #[cfg(not(target_arch = "wasm32"))]
    loader: crate::MediaLoader,
}
impl TileCanvas {
    pub fn new(provider: Arc<dyn TileProvider>, options: CanvasOptions) -> Result<Self> {
        options.budgets.validate()?;
        ensure!(
            options.max_cached_pages > 0
                && options.scratch_bytes_per_job <= options.budgets.cpu_bytes,
            "invalid canvas cache/scratch policy"
        );
        let image = provider.image();
        let canonical = match image.format {
            PixelFormat::Rgba8 => image.color == ImageColor::SRGB,
            PixelFormat::Rgba16Float => {
                image.color
                    == ImageColor {
                        alpha: AlphaMode::Premultiplied,
                        ..ImageColor::LINEAR_P3
                    }
            }
        };
        ensure!(
            canonical,
            "tile provider must return canonical sRGB RGBA8 or associated linear-P3 RGBA16F pages"
        );
        let pyramid = TilePyramid::new(image, options.layout);
        let source = provider.source();
        #[cfg(not(target_arch = "wasm32"))]
        let loader = {
            let edge = u64::from(options.layout.edge()) + 2 * u64::from(options.layout.halo());
            let max = (edge * edge * 8).min(gpui::ImageTextureData::MAX_BYTES as u64) as usize;
            crate::MediaLoader::new(crate::MediaLimits {
                max_jobs: options.budgets.decode_jobs as usize,
                max_image_bytes: max,
                max_resident_bytes: (options.budgets.cpu_bytes as usize)
                    .saturating_mul(2)
                    .max(max),
                max_encoded_bytes: max,
            })?
        };
        let canvas = Self {
            provider,
            options,
            pyramid,
            usage: Arc::new(Mutex::new(Usage::default())),
            cache: HashMap::new(),
            pending: Vec::new(),
            preview: None,
            desired: Vec::new(),
            view: None,
            revision: 0,
            clock: 0,
            stats: CanvasStats::default(),
            stopped: false,
            failed: HashSet::new(),
            group_id: gpui::content::ContentGroupId::new(),
            progressive: ProgressiveImage::new(
                source,
                SourceCapabilities {
                    preview: true,
                    refinement: true,
                },
            ),
            #[cfg(not(target_arch = "wasm32"))]
            loader,
        };
        canvas.options.budgets.check_tile(&canvas.preview_tile()?)?;
        Ok(canvas)
    }
    fn preview_tile(&self) -> Result<TileDescriptor> {
        let image = self.provider.image();
        let mip = (0..image.mip_count())
            .find(|m| {
                image.mip_size(*m).is_ok_and(|[w, h]| {
                    w <= self.options.layout.edge() && h <= self.options.layout.edge()
                })
            })
            .unwrap();
        self.pyramid.tile(self.provider.source(), mip, 0, 0)
    }
    fn select(&self, view: CanvasView) -> Result<Vec<TileDescriptor>> {
        let image = self.provider.image();
        let lod = (1. / (view.scale * view.device_scale))
            .log2()
            .floor()
            .max(0.) as u32;
        let preview = self.preview_tile()?.payload_bytes();
        for mip in lod.min(image.mip_count() - 1)..image.mip_count() {
            let [mw, mh] = image.mip_size(mip)?;
            let [cols, rows] = self.pyramid.tile_count(mip)?;
            let edge = f64::from(self.options.layout.edge());
            let convert = |axis: usize, full: u32, m: u32, count: u32| {
                let lo = ((view.center[axis] - view.viewport[axis] / view.scale / 2.).max(0.)
                    / f64::from(full)
                    * f64::from(m)
                    / edge)
                    .floor()
                    .clamp(0., f64::from(count)) as u32;
                let hi = ((view.center[axis] + view.viewport[axis] / view.scale / 2.).max(0.)
                    / f64::from(full)
                    * f64::from(m)
                    / edge)
                    .ceil()
                    .clamp(0., f64::from(count)) as u32;
                (lo, hi)
            };
            let (x0, x1) = convert(0, image.width(), mw, cols);
            let (y0, y1) = convert(1, image.height(), mh, rows);
            // Bound selection work before enumerating a potentially enormous grid.
            if u64::from(x1 - x0) * u64::from(y1 - y0) > self.options.max_cached_pages as u64 {
                continue;
            }
            let mut tiles = Vec::new();
            let mut bytes = preview;
            for y in y0..y1 {
                for x in x0..x1 {
                    let tile = self.pyramid.tile(self.provider.source(), mip, x, y)?;
                    bytes = bytes
                        .checked_add(tile.payload_bytes())
                        .ok_or_else(|| anyhow::anyhow!("tile selection overflow"))?;
                    tiles.push(tile);
                }
            }
            if tiles
                .iter()
                .any(|tile| self.options.budgets.check_tile(tile).is_err())
            {
                continue;
            }
            let scratch = self
                .options
                .scratch_bytes_per_job
                .saturating_mul(u64::from(self.options.budgets.decode_jobs));
            if bytes > self.options.budgets.gpu_bytes
                || bytes.saturating_add(scratch) > self.options.budgets.cpu_bytes
            {
                continue;
            }
            tiles.sort_by(|a, b| {
                let distance = |t: &TileDescriptor| {
                    let c = t.content();
                    let x = (f64::from(c.x) + f64::from(c.width) / 2.) / f64::from(mw)
                        * f64::from(image.width());
                    let y = (f64::from(c.y) + f64::from(c.height) / 2.) / f64::from(mh)
                        * f64::from(image.height());
                    (x - view.center[0]).hypot(y - view.center[1])
                };
                distance(a).total_cmp(&distance(b))
            });
            return Ok(tiles);
        }
        anyhow::bail!("visible tiles and preview exceed canvas budgets")
    }
    fn reserve(&self, tile: TileDescriptor) -> Option<Arc<Lease>> {
        let b = self.options.budgets;
        let scratch = self.options.scratch_bytes_per_job;
        let mut u = self.usage.lock().unwrap();
        if u.jobs >= b.decode_jobs
            || tile.payload_bytes().saturating_add(scratch) > b.cpu_bytes.saturating_sub(u.cpu)
            || tile.payload_bytes() > b.gpu_bytes.saturating_sub(u.gpu)
            || tile.staging_bytes() > b.staging_bytes.saturating_sub(u.staging)
        {
            return None;
        }
        u.cpu += tile.payload_bytes() + scratch;
        u.gpu += tile.payload_bytes();
        u.staging += tile.staging_bytes();
        u.jobs += 1;
        Some(Arc::new(Lease {
            usage: self.usage.clone(),
            bytes: tile.payload_bytes(),
            staging: tile.staging_bytes(),
            scratch,
            working: AtomicBool::new(true),
        }))
    }
    fn validate_page(tile: TileDescriptor, data: &ImageTextureData) -> Result<()> {
        let upload = tile.upload();
        ensure!(
            data.width() == upload.width
                && data.height() == upload.height
                && data.levels().len() == 1
                && data.byte_len() as u64 == tile.payload_bytes()
                && data.format()
                    == match tile.key().image.format {
                        PixelFormat::Rgba8 => ImageTextureFormat::Rgba8Srgb,
                        PixelFormat::Rgba16Float => ImageTextureFormat::LinearP3Rgba16Float,
                    },
            "provider returned wrong tile extent, format or payload"
        );
        Ok(())
    }
    fn publish_page(&mut self, page: Arc<CanvasPage>, preview: bool) {
        page._lease.finish();
        if preview {
            self.preview = Some(page);
        } else {
            self.cache.insert(
                page.tile.key(),
                Cached {
                    page,
                    used: self.clock,
                },
            );
        }
    }
    fn evict_idle(&mut self, wanted: &HashSet<TileKey>, force: bool) -> bool {
        if let Some(key) = self
            .cache
            .iter()
            .filter(|(k, _)| !wanted.contains(k))
            .min_by_key(|(_, v)| v.used)
            .map(|(k, _)| *k)
            && (force || self.cache.len() >= self.options.max_cached_pages)
        {
            self.cache.remove(&key);
            self.stats.evictions += 1;
            return true;
        }
        false
    }
    pub fn cancel(&mut self) {
        self.stats.cancelled_requests += self.pending.len() as u64;
        self.pending.clear();
        self.cache.clear();
        self.failed.clear();
        self.preview = None;
        self.desired.clear();
        self.stopped = true;
        self.progressive.cancel();
    }
    /// Replace asset/provider. Old workers/scenes retain charged leases until
    /// completion; their results cannot populate the new generation.
    pub fn replace(&mut self, provider: Arc<dyn TileProvider>) -> Result<()> {
        let image = provider.image();
        ensure!(
            match image.format {
                PixelFormat::Rgba8 => image.color == ImageColor::SRGB,
                PixelFormat::Rgba16Float =>
                    image.color
                        == ImageColor {
                            alpha: AlphaMode::Premultiplied,
                            ..ImageColor::LINEAR_P3
                        },
            },
            "replacement requires canonical page metadata"
        );
        let pyramid = TilePyramid::new(image, self.options.layout);
        let mip = (0..image.mip_count())
            .find(|m| {
                image.mip_size(*m).is_ok_and(|[w, h]| {
                    w <= self.options.layout.edge() && h <= self.options.layout.edge()
                })
            })
            .unwrap();
        self.options
            .budgets
            .check_tile(&pyramid.tile(provider.source(), mip, 0, 0)?)?;
        self.cancel();
        self.provider = provider;
        self.pyramid = pyramid;
        self.view = None;
        self.stopped = false;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("canvas revision exhausted"))?;
        self.progressive.restart(
            self.provider.source(),
            SourceCapabilities {
                preview: true,
                refinement: true,
            },
        )?;
        Ok(())
    }
    pub fn status(&self) -> &ProgressiveStatus {
        self.progressive.status()
    }
    pub fn snapshot(&self) -> Option<Arc<CanvasSnapshot>> {
        self.progressive.snapshot().cloned()
    }
    pub fn stats(&self) -> CanvasStats {
        let u = self.usage.lock().unwrap();
        CanvasStats {
            cpu_bytes: u.cpu,
            gpu_bytes: u.gpu,
            pending_staging_bytes: u.staging,
            active_jobs: u.jobs,
            cached_pages: self.cache.len(),
            visible_pages: self.desired.len(),
            decoded_pages: self
                .desired
                .iter()
                .filter(|t| {
                    self.cache.get(&t.key()).is_some()
                        || self
                            .preview
                            .as_ref()
                            .is_some_and(|p| p.tile.key() == t.key())
                })
                .count(),
            ..self.stats
        }
    }
    /// Advance provider decoding once per frame. GPU execution and redraw are
    /// renderer-owned; per-frame page limits bound CPU publication only.
    pub fn advance(&mut self, view: CanvasView) -> Result<()> {
        view.validate()?;
        if self.stopped {
            return Ok(());
        }
        self.clock = self.clock.wrapping_add(1);
        if self.view != Some(view) {
            self.revision = self
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("canvas revision exhausted"))?;
            self.desired = self.select(view)?;
            self.stats.mip = self.desired.first().map_or(0, |t| t.key().mip);
            self.view = Some(view);
            // Preview belongs to the source, not the viewport. Preserve its job.
            self.stats.cancelled_requests +=
                self.pending.iter().filter(|p| !p.preview).count() as u64;
            self.pending.retain(|p| p.preview);
            self.failed.clear();
            self.progressive.restart(
                self.provider.source(),
                SourceCapabilities {
                    preview: true,
                    refinement: true,
                },
            )?;
        }
        let wanted = self.desired.iter().map(|t| t.key()).collect::<HashSet<_>>();
        for tile in &self.desired {
            if let Some(page) = self.cache.get_mut(&tile.key()) {
                page.used = self.clock;
            }
        }
        let mut changed = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pending = std::mem::take(&mut self.pending);
            for mut p in pending {
                if let Some(image) = p.job.try_take() {
                    if p.preview || (p.revision == self.revision && wanted.contains(&p.tile.key()))
                    {
                        self.publish_page(
                            Arc::new(CanvasPage {
                                tile: p.tile,
                                _lease: p.lease,
                                content: image.content(),
                            }),
                            p.preview,
                        );
                        changed = true;
                    } else {
                        self.stats.stale_results += 1;
                    }
                } else if let crate::LoadStatus::Failed(error) = p.job.status() {
                    self.stats.failures += 1;
                    self.failed.insert(p.tile.key());
                    self.progressive.publish(SourceUpdate {
                        request: self.progressive.request(),
                        sequence: self.clock,
                        event: SourceEvent::Failed(error),
                    });
                } else {
                    self.pending.push(p);
                }
            }
        }
        if matches!(self.progressive.status(), ProgressiveStatus::Failed(_)) {
            return Ok(());
        }
        while self.cache.len() > self.options.max_cached_pages && self.evict_idle(&wanted, true) {}
        let mut candidates = Vec::new();
        if self.preview.is_none()
            && !self.failed.contains(&self.preview_tile()?.key())
            && !self.pending.iter().any(|p| p.preview)
        {
            candidates.push((self.preview_tile()?, true));
        }
        if self.preview.is_some() {
            for tile in &self.desired {
                if !self.failed.contains(&tile.key())
                    && !self.cache.contains_key(&tile.key())
                    && !self.pending.iter().any(|p| p.tile.key() == tile.key())
                    && !self
                        .preview
                        .as_ref()
                        .is_some_and(|p| p.tile.key() == tile.key())
                {
                    candidates.push((*tile, false));
                }
            }
        }
        let mut frame_bytes = 0;
        let limit = if cfg!(target_arch = "wasm32") {
            1
        } else {
            self.options.budgets.uploads_per_frame
        };
        for (count, (tile, preview)) in candidates.into_iter().enumerate() {
            if count >= limit as usize
                || tile.staging_bytes()
                    > self
                        .options
                        .budgets
                        .upload_bytes_per_frame
                        .saturating_sub(frame_bytes)
            {
                break;
            }
            self.options.budgets.check_tile(&tile)?;
            let lease = loop {
                if let Some(lease) = self.reserve(tile) {
                    break Some(lease);
                }
                if !self.evict_idle(&wanted, true) {
                    break None;
                }
            };
            let Some(lease) = lease else {
                break;
            };
            let provider = self.provider.clone();
            #[cfg(not(target_arch = "wasm32"))]
            {
                let job = self.loader.load_guarded(
                    tile.key().image.color,
                    Some(lease.clone()),
                    move |_, cancelled| {
                        let data = provider.tile(tile, cancelled)?;
                        Self::validate_page(tile, &data)?;
                        Ok(data)
                    },
                )?;
                self.pending.push(Pending {
                    tile,
                    revision: self.revision,
                    preview,
                    lease,
                    job,
                });
            }
            #[cfg(target_arch = "wasm32")]
            {
                let result = provider
                    .tile(tile, &AtomicBool::new(false))
                    .and_then(|data| {
                        Self::validate_page(tile, &data)?;
                        Ok(data)
                    });
                let data = match result {
                    Ok(data) => data,
                    Err(error) => {
                        self.stats.failures += 1;
                        self.failed.insert(tile.key());
                        self.progressive.publish(SourceUpdate {
                            request: self.progressive.request(),
                            sequence: self.clock,
                            event: SourceEvent::Failed(format!("{error:#}")),
                        });
                        break;
                    }
                };
                self.publish_page(
                    Arc::new(CanvasPage {
                        tile,
                        _lease: lease.clone(),
                        content: gpui::content::Content::image(gpui::content::ImageAsset::ready(
                            data.pixels().clone().with_owner(lease.clone()),
                        )),
                    }),
                    preview,
                );
                changed = true;
            }
            self.stats.requests += 1;
            frame_bytes += tile.staging_bytes();
        }
        if self.preview.is_some() && (changed || self.progressive.snapshot().is_none()) {
            let pages = self
                .desired
                .iter()
                .filter_map(|t| self.cache.get(&t.key()).map(|p| p.page.clone()))
                .collect::<Vec<_>>();
            let complete = self.preview.is_some()
                && self.desired.iter().all(|t| {
                    self.cache.contains_key(&t.key())
                        || self
                            .preview
                            .as_ref()
                            .is_some_and(|p| p.tile.key() == t.key())
                });
            let mut snapshot = CanvasSnapshot {
                group: gpui::content::ContentGroup::new(self.group_id),
                source: self.provider.source(),
                image: self.provider.image(),
                view,
                preview: self.preview.clone(),
                pages,
            };
            snapshot.group = snapshot.build_group(self.group_id);
            let snapshot = Arc::new(snapshot);
            let event = if complete {
                SourceEvent::Complete(snapshot)
            } else if self.progressive.snapshot().is_none() {
                SourceEvent::Preview(snapshot)
            } else {
                SourceEvent::Refinement(snapshot)
            };
            self.progressive.publish(SourceUpdate {
                request: self.progressive.request(),
                sequence: self.clock,
                event,
            });
        }
        Ok(())
    }
}
impl Drop for TileCanvas {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::PlatformHeadlessRenderer as _;
    use std::time::{Duration, Instant};
    fn view() -> CanvasView {
        CanvasView {
            center: [6000., 3000.],
            scale: 0.1,
            viewport: [960., 480.],
            device_scale: 1.,
        }
    }
    fn settled(canvas: &mut TileCanvas, view: CanvasView) -> Result<Arc<CanvasSnapshot>> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            canvas.advance(view)?;
            if matches!(canvas.status(), ProgressiveStatus::Failed(_)) {
                anyhow::bail!("provider failed: {:?}", canvas.status());
            }
            if canvas.status() == &ProgressiveStatus::Ready {
                return Ok(canvas.snapshot().unwrap());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        anyhow::bail!("canvas failed to settle: {:?}", canvas.stats())
    }
    #[test]
    fn viewport_selection_is_bounded_and_device_scale_selects_detail() -> Result<()> {
        let canvas = TileCanvas::new(Arc::new(GeneratedCanvas::new(0)), CanvasOptions::default())?;
        let base = CanvasView {
            viewport: [400., 200.],
            ..view()
        };
        let a = canvas.select(base)?;
        let b = canvas.select(CanvasView {
            device_scale: 2.,
            ..base
        })?;
        assert!(b[0].key().mip < a[0].key().mip);
        let huge = CanvasView {
            scale: 1e-9,
            ..view()
        };
        assert!(canvas.select(huge)?.len() <= canvas.options.max_cached_pages);
        for tile in a {
            canvas.options.budgets.check_tile(&tile)?;
        }
        assert!(
            canvas
                .select(CanvasView {
                    center: [-1e15, -1e15],
                    ..view()
                })?
                .is_empty()
        );
        Ok(())
    }
    #[test]
    fn pressure_evicts_and_snapshots_pin_budget_until_last_owner_drops() -> Result<()> {
        let mut options = CanvasOptions::default();
        options.budgets.gpu_bytes = 2 * 1024 * 1024;
        options.budgets.cpu_bytes = 4 * 1024 * 1024;
        options.max_cached_pages = 6;
        let mut canvas = TileCanvas::new(Arc::new(GeneratedCanvas::new(0)), options)?;
        let v = CanvasView {
            scale: 1.,
            viewport: [700., 380.],
            ..view()
        };
        let held = settled(&mut canvas, v)?;
        let first_bytes = canvas.stats().gpu_bytes;
        let moved = CanvasView {
            center: [1000., 1000.],
            ..v
        };
        for _ in 0..15 {
            canvas.advance(moved)?;
            let stats = canvas.stats();
            assert!(
                stats.cpu_bytes <= options.budgets.cpu_bytes
                    && stats.gpu_bytes <= options.budgets.gpu_bytes
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(canvas.stats().evictions > 0);
        assert!(
            canvas.stats().gpu_bytes >= first_bytes,
            "held snapshot remains charged after eviction"
        );
        drop(held);
        drop(settled(&mut canvas, moved)?);
        let usage = canvas.usage.clone();
        canvas.cancel();
        let deadline = Instant::now() + Duration::from_secs(2);
        while usage.lock().unwrap().cpu > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(usage.lock().unwrap().gpu, 0);
        assert_eq!(usage.lock().unwrap().jobs, 0);
        Ok(())
    }
    #[test]
    fn replacement_rejects_obsolete_generations_and_retains_no_full_image() -> Result<()> {
        let mut canvas =
            TileCanvas::new(Arc::new(GeneratedCanvas::new(0)), CanvasOptions::default())?;
        let v = view();
        canvas.advance(v)?;
        canvas.replace(Arc::new(GeneratedCanvas::new(1)))?;
        canvas.advance(v)?;
        canvas.replace(Arc::new(GeneratedCanvas::new(2)))?;
        let snapshot = settled(&mut canvas, v)?;
        assert_eq!(snapshot.source.generation, 2);
        assert!(
            snapshot
                .pages
                .iter()
                .chain(snapshot.preview.iter())
                .all(|p| p.tile.key().source.generation == 2)
        );
        assert!(canvas.stats().gpu_bytes < 16 * 1024 * 1024);
        assert!(
            canvas.provider.image().full_mip_bytes()? > 300 * 1024 * 1024,
            "72 MP cannot fit the whole-image path"
        );
        assert!(canvas.stats().cancelled_requests >= 2);
        canvas.replace(Arc::new(Broken { panic: false }))?;
        let changed = settled(
            &mut canvas,
            CanvasView {
                center: [256., 128.],
                scale: 0.1,
                ..v
            },
        )?;
        assert_eq!(changed.image.width(), 513);
        assert!(
            changed
                .pages
                .iter()
                .all(|p| p.tile.key().image == changed.image)
        );
        Ok(())
    }
    #[derive(Debug)]
    struct Broken {
        panic: bool,
    }
    impl TileProvider for Broken {
        fn source(&self) -> SourceId {
            SourceId {
                asset: 1,
                generation: 0,
            }
        }
        fn image(&self) -> ImageDescriptor {
            ImageDescriptor::new(513, 257, PixelFormat::Rgba8, ImageColor::SRGB).unwrap()
        }
        fn tile(&self, tile: TileDescriptor, _: &AtomicBool) -> Result<ImageTextureData> {
            if tile.key().mip == 0 {
                if self.panic {
                    panic!("provider panic test");
                }
                anyhow::bail!("provider refinement failed");
            }
            let r = tile.upload();
            ImageTextureData::page(
                r.width,
                r.height,
                vec![128; tile.payload_bytes() as usize],
                ImageTextureFormat::Rgba8Srgb,
            )
        }
    }
    #[test]
    fn failed_or_panicking_refinement_keeps_preview_and_stops_retry_storms() -> Result<()> {
        for panic in [false, true] {
            let mut canvas = TileCanvas::new(Arc::new(Broken { panic }), CanvasOptions::default())?;
            let v = CanvasView {
                center: [256., 128.],
                scale: 1.,
                viewport: [400., 200.],
                device_scale: 1.,
            };
            let deadline = Instant::now() + Duration::from_secs(3);
            while !matches!(canvas.status(), ProgressiveStatus::Failed(_))
                && Instant::now() < deadline
            {
                canvas.advance(v)?;
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(matches!(canvas.status(), ProgressiveStatus::Failed(_)));
            assert!(canvas.snapshot().unwrap().preview.is_some());
            let requests = canvas.stats().requests;
            for _ in 0..10 {
                canvas.advance(v)?;
            }
            assert_eq!(canvas.stats().requests, requests);
        }
        Ok(())
    }
    #[test]
    fn page_validation_and_preview_subtraction_preserve_alpha_contract() -> Result<()> {
        let canvas = TileCanvas::new(Arc::new(GeneratedCanvas::new(0)), CanvasOptions::default())?;
        let tile = canvas.preview_tile()?;
        assert!(
            TileCanvas::validate_page(tile, &ImageTextureData::new(1, 1, vec![vec![0; 4]])?)
                .is_err()
        );
        let image = ImageDescriptor::new(1, 1, PixelFormat::Rgba16Float, ImageColor::LINEAR_P3)?;
        let tile = TilePyramid::new(image, TileLayout::new(64, 0)?).tile(
            SourceId {
                asset: 1,
                generation: 0,
            },
            0,
            0,
            0,
        )?;
        let p3 = ImageTextureData::page(1, 1, vec![0; 8], ImageTextureFormat::LinearP3Rgba16Float)?;
        let rec = ImageTextureData::page(
            1,
            1,
            vec![0; 8],
            ImageTextureFormat::LinearRec2020Rgba16Float,
        )?;
        assert!(TileCanvas::validate_page(tile, &p3).is_ok());
        assert!(TileCanvas::validate_page(tile, &rec).is_err());
        let r = gpui::Bounds::new(
            gpui::point(gpui::px(0.), gpui::px(0.)),
            gpui::size(gpui::px(100.), gpui::px(80.)),
        );
        let cut = gpui::Bounds::new(
            gpui::point(gpui::px(20.), gpui::px(10.)),
            gpui::size(gpui::px(50.), gpui::px(40.)),
        );
        let pieces = subtract(r, cut);
        let area: f32 = pieces
            .iter()
            .map(|r| f32::from(r.size.width) * f32::from(r.size.height))
            .sum();
        assert_eq!(area, 6000.);
        assert!(pieces.iter().all(|r| r.intersect(&cut).is_empty()));
        Ok(())
    }

    #[derive(Debug)]
    struct Gradient;
    impl TileProvider for Gradient {
        fn source(&self) -> SourceId {
            SourceId {
                asset: 2,
                generation: 0,
            }
        }
        fn image(&self) -> ImageDescriptor {
            ImageDescriptor::new(513, 257, PixelFormat::Rgba8, ImageColor::SRGB).unwrap()
        }
        fn tile(&self, tile: TileDescriptor, cancelled: &AtomicBool) -> Result<ImageTextureData> {
            let [w, h] = self.image().mip_size(tile.key().mip)?;
            let r = tile.upload();
            let mut bytes = Vec::new();
            for y in r.y..r.y + r.height {
                ensure!(!cancelled.load(Ordering::Acquire), "cancelled");
                for x in r.x..r.x + r.width {
                    bytes.extend_from_slice(&[
                        (x as f64 / w as f64 * 255.) as u8,
                        (y as f64 / h as f64 * 255.) as u8,
                        100,
                        128,
                    ]);
                }
            }
            ImageTextureData::page(r.width, r.height, bytes, ImageTextureFormat::Rgba8Srgb)
        }
    }
    fn gpu_canvas(renderer: &mut dyn gpui::PlatformHeadlessRenderer, scale: f64) -> Result<()> {
        use gpui::{ContentMask, CustomDraw, DevicePixels, ScaledPixels, Scene, point, px, size};
        let mut canvas = TileCanvas::new(Arc::new(Gradient), CanvasOptions::default())?;
        let v = CanvasView {
            center: [256., 128.],
            scale,
            viewport: [500., 240.],
            device_scale: 1.,
        };
        let snapshot = settled(&mut canvas, v)?;
        if scale >= 64. {
            let mut preview = CanvasSnapshot {
                group: gpui::content::ContentGroup::new(canvas.group_id),
                source: snapshot.source,
                image: snapshot.image,
                view: snapshot.view,
                preview: snapshot.preview.clone(),
                pages: vec![],
            };
            preview.group = preview.build_group(canvas.group_id);
            let pixels = ready_pixels(renderer, &refinement_scene(&preview))?;
            assert!(
                pixels.get_pixel(253, 100).0[2].abs_diff(50) <= 2,
                "deep-zoom coarse preview must stay visible with bounded geometry"
            );
        }
        let bounds = gpui::Bounds::new(point(px(4.), px(4.)), size(px(500.), px(240.)));
        let mut image = None;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let mut scene = Scene::default();
            // Opaque black reference makes alpha association observable.
            let background = gpui::Bounds::new(
                point(ScaledPixels(0.), ScaledPixels(0.)),
                size(ScaledPixels(508.), ScaledPixels(248.)),
            );
            scene.insert_primitive(gpui::Quad {
                bounds: background,
                content_mask: ContentMask { bounds: background },
                background: gpui::rgb(0).into(),
                ..Default::default()
            });
            scene.insert_primitive(CustomDraw {
                bounds: bounds.map(|v| ScaledPixels(f32::from(v))),
                content_mask: ContentMask {
                    bounds: bounds.map(|v| ScaledPixels(f32::from(v))),
                },
                content: snapshot.content_group().into_scene_content(),
                opacity: 1.,
                order: 0,
                corner_radii: Default::default(),
                coverage: None,
            });
            scene.finish();
            match renderer.prepare_content_snapshot(&scene) {
                Ok(()) => {
                    image = Some(renderer.render_scene_to_image(
                        &scene,
                        size(DevicePixels(508), DevicePixels(248)),
                    )?);
                    break;
                }
                Err(error)
                    if error
                        .downcast_ref::<gpui::content::ContentError>()
                        .is_some_and(|e| e.kind == gpui::content::ContentErrorKind::Pending) => {}
                Err(error) => return Err(error),
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let image = image.unwrap();
        assert_eq!(canvas.stats().decoded_pages, canvas.stats().visible_pages);
        // Includes the x=256 tile boundary and clipped odd bottom/right pages.
        for (x, y) in [
            (20, 20),
            (252, 100),
            (253, 100),
            (254, 100),
            (255, 100),
            (400, 200),
            (500, 240),
        ] {
            let expected = [
                (((x as f64 + 0.5 - (254. - 256. * scale)) / scale - 0.5) / 513. * 255.) as u8,
                (((y as f64 + 0.5 - (124. - 128. * scale)) / scale - 0.5) / 257. * 255.) as u8,
                100,
            ];
            for (c, value) in expected.iter().enumerate() {
                let byte = (*value as f32 * 128. / 255.).round() as u8;
                assert!(
                    image.get_pixel(x, y).0[c].abs_diff(byte) <= 2,
                    "tile seam/alpha {x},{y}: {:?} expected {byte} channel {c}",
                    image.get_pixel(x, y).0
                );
            }
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn metal_tiled_halos_clipping_and_transparency_match_reference() -> Result<()> {
        let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
        gpu_canvas(&mut renderer, 1.)?;
        gpu_canvas(&mut renderer, 64.)?;
        gpu_canvas(&mut renderer, 1024.)
    }
    #[cfg(feature = "wgpu")]
    #[test]
    fn wgpu_tiled_halos_clipping_and_transparency_match_reference() -> Result<()> {
        let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
        gpu_canvas(&mut renderer, 1.)?;
        gpu_canvas(&mut renderer, 64.)?;
        gpu_canvas(&mut renderer, 1024.)
    }
    #[test]
    fn portable_graph_retains_page_lease_after_canvas_and_snapshot_drop() -> Result<()> {
        use gpui::content::{
            ContentCapabilities, ContentGraph, ContentRequest, ContentRoot, GraphLimits,
            ImageExtent,
        };
        let mut canvas = TileCanvas::new(Arc::new(Gradient), CanvasOptions::default())?;
        let snapshot = settled(
            &mut canvas,
            CanvasView {
                center: [256., 128.],
                scale: 1.,
                viewport: [500., 240.],
                device_scale: 1.,
            },
        )?;
        let usage = canvas.usage.clone();
        let page = &snapshot.pages[0];
        let bytes = page.tile.payload_bytes();
        let graph = ContentGraph::describe(
            &[ContentRoot {
                content: page.draw().content,
                request: ContentRequest::new(
                    gpui::size(gpui::px(10.), gpui::px(10.)),
                    ImageExtent::new(10, 10)?,
                    1.,
                )?,
            }],
            ContentCapabilities::default(),
            GraphLimits::default(),
        )?;
        drop(snapshot);
        drop(canvas);
        assert_eq!(
            usage.lock().unwrap().cpu,
            bytes,
            "graph-held pixels must retain canvas admission"
        );
        drop(graph);
        assert_eq!(usage.lock().unwrap().cpu, 0);
        assert_eq!(usage.lock().unwrap().gpu, 0);
        Ok(())
    }
    fn refinement_scene(snapshot: &CanvasSnapshot) -> gpui::Scene {
        use gpui::*;
        let mut scene = Scene::default();
        let bounds = Bounds::new(
            point(ScaledPixels(4.), ScaledPixels(4.)),
            size(ScaledPixels(500.), ScaledPixels(240.)),
        );
        let bg = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(508.), ScaledPixels(248.)),
        );
        scene.insert_primitive(Quad {
            bounds: bg,
            content_mask: ContentMask { bounds: bg },
            background: rgb(0).into(),
            ..Default::default()
        });
        scene.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Default::default(),
            opacity: 1.,
            coverage: None,
            content: snapshot.content_group().into_scene_content(),
        });
        scene.finish();
        scene
    }
    fn ready_pixels(
        renderer: &mut dyn gpui::PlatformHeadlessRenderer,
        scene: &gpui::Scene,
    ) -> Result<image::RgbaImage> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match renderer.prepare_content_snapshot(scene) {
                Ok(()) => {
                    return renderer.render_scene_to_image(
                        scene,
                        gpui::size(gpui::DevicePixels(508), gpui::DevicePixels(248)),
                    );
                }
                Err(e)
                    if e.downcast_ref::<gpui::content::ContentError>()
                        .is_some_and(|e| e.kind == gpui::content::ContentErrorKind::Pending) => {}
                Err(e) => return Err(e),
            }
            ensure!(Instant::now() < deadline, "page GPU readiness timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn refinement_setup(
        renderer: &mut dyn gpui::PlatformHeadlessRenderer,
    ) -> Result<(gpui::Scene, image::RgbaImage)> {
        let mut canvas = TileCanvas::new(Arc::new(Gradient), CanvasOptions::default())?;
        let snapshot = settled(
            &mut canvas,
            CanvasView {
                center: [256., 128.],
                scale: 1.,
                viewport: [500., 240.],
                device_scale: 1.,
            },
        )?;
        let mut preview = CanvasSnapshot {
            group: gpui::content::ContentGroup::new(canvas.group_id),
            source: snapshot.source,
            image: snapshot.image,
            view: snapshot.view,
            preview: snapshot.preview.clone(),
            pages: vec![],
        };
        preview.group = preview.build_group(canvas.group_id);
        let pixels = ready_pixels(renderer, &refinement_scene(&preview))?;
        assert!(
            pixels.get_pixel(253, 100).0[2] > 40,
            "preview must be visible"
        );
        Ok((refinement_scene(&snapshot), pixels))
    }
    fn assert_pending_refinement(
        renderer: &mut dyn gpui::PlatformHeadlessRenderer,
        scene: &gpui::Scene,
        previous: &image::RgbaImage,
    ) -> Result<()> {
        let err = renderer.prepare_content_snapshot(scene).unwrap_err();
        assert_eq!(
            err.downcast_ref::<gpui::content::ContentError>()
                .unwrap()
                .kind,
            gpui::content::ContentErrorKind::Pending
        );
        let actual = renderer.render_scene_to_image(
            scene,
            gpui::size(gpui::DevicePixels(508), gpui::DevicePixels(248)),
        )?;
        assert_eq!(
            &actual, previous,
            "pending refinement must retain the complete preview, including translucent regions"
        );
        Ok(())
    }
    macro_rules! paused_refinement {
        ($renderer:expr,$limits:ty,$gate:ty) => {{
            let mut renderer = $renderer;
            let (scene, previous) = refinement_setup(&mut renderer)?;
            struct Release(Arc<$gate>);
            impl Drop for Release {
                fn drop(&mut self) {
                    self.0.release();
                }
            }
            let gate = Release(Arc::new(<$gate>::default()));
            let mut limits = <$limits>::default();
            limits.pause_before_node = Some((0, gate.0.clone()));
            renderer.set_content_validation_limits(limits);
            assert!(renderer.prepare_content_snapshot(&scene).is_err());
            assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
            assert_pending_refinement(&mut renderer, &scene, &previous)?;
            gate.0.release();
            let pixels = ready_pixels(&mut renderer, &scene)?;
            for x in [252, 253, 254, 255, 256] {
                assert!(
                    pixels.get_pixel(x, 100).0[2].abs_diff(50) <= 2,
                    "refinement must replace preview alpha once at tile boundaries"
                );
            }
            let jobs = renderer.content_presentation_stats().requests;
            let replay = ready_pixels(&mut renderer, &scene)?;
            assert_eq!(
                renderer.content_presentation_stats().requests,
                jobs,
                "replay must reuse page GPU realizations"
            );
            assert_eq!(pixels, replay);
            Ok(())
        }};
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn metal_paused_refinement_retains_preview_without_alpha_overlap() -> Result<()> {
        paused_refinement!(
            gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
            gpui_apple::content_executor::MetalContentLimits,
            gpui_apple::content_executor::MetalContentTestGate
        )
    }
    #[cfg(feature = "wgpu")]
    #[test]
    fn wgpu_paused_refinement_retains_preview_without_alpha_overlap() -> Result<()> {
        paused_refinement!(
            gpui_wgpu::WgpuHeadlessRenderer::new()?,
            gpui_wgpu::content_executor::WgpuContentLimits,
            gpui_wgpu::content_executor::WgpuContentTestGate
        )
    }
}
