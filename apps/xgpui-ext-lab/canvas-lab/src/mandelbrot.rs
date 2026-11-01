//! Coherent viewport frames. The displayed image is immutable while its successor
//! renders offscreen; camera input only reprojects the completed front image.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use xgpui_media::pipeline::ImageColor;
use xgpui_media::{LoadStatus, MediaImage, MediaLimits, MediaLoader, UploadStatus};

#[allow(dead_code)]
#[path = "../../shared/mandelbrot.rs"]
mod fractal_math;
pub(super) use fractal_math::iterations;
mod check;
#[cfg(target_os = "macos")]
mod metal;

const SETTLE: Duration = Duration::from_millis(140);
const SHARPEN_BLEND: Duration = Duration::from_millis(180);
const OVERSCAN: f64 = 1.25;
const MAX_REPROJECTION: f64 = 32.;
// The GPU route is deliberately restricted to shallow views. Absolute f32
// coordinates are not a substitute for f64/perturbation at deep zoom.
const GPU_MAX_ZOOM: f64 = 64.;

#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    center: WorldPoint,
    zoom: f64,
    viewport: WorldPoint,
    scale: f32,
}
impl View {
    fn new(camera: Camera2D, viewport: WorldPoint, scale: f32) -> Self {
        Self {
            center: camera
                .screen_to_world(WorldPoint::new(viewport.x / 2., viewport.y / 2.), viewport),
            zoom: camera.zoom(),
            viewport,
            scale,
        }
    }
    fn bounds(self) -> WorldBounds {
        let half = WorldPoint::new(
            self.viewport.x * OVERSCAN / (2. * self.zoom),
            self.viewport.y * OVERSCAN / (2. * self.zoom),
        );
        WorldBounds {
            min: WorldPoint::new(self.center.x - half.x, self.center.y - half.y),
            max: WorldPoint::new(self.center.x + half.x, self.center.y + half.y),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Request {
    view: View,
    detail: bool,
    gpu: bool,
    dimensions: [u32; 2],
    limit: u32,
}
impl Request {
    fn new(view: View, detail: bool, gpu: bool) -> Self {
        let cap: f64 = match (gpu, detail) {
            (true, true) => 1920.,
            (true, false) => 1920.,
            (false, true) => 960.,
            (false, false) => 128.,
        };
        let margin = if gpu {
            1.5
        } else if detail {
            OVERSCAN
        } else {
            3.
        };
        let width = view.viewport.x * margin * f64::from(view.scale);
        let height = view.viewport.y * margin * f64::from(view.scale);
        let factor = (cap / width.max(height)).min(1.);
        Self {
            view,
            detail,
            gpu,
            dimensions: [
                (width * factor).round().max(1.) as u32,
                (height * factor).round().max(1.) as u32,
            ],
            limit: iterations(view.zoom),
        }
    }
    fn margin(self) -> f64 {
        if self.gpu {
            1.5
        } else if self.detail {
            OVERSCAN
        } else {
            3.
        }
    }
    fn bounds(self) -> WorldBounds {
        let half = WorldPoint::new(
            self.view.viewport.x * self.margin() / (2. * self.view.zoom),
            self.view.viewport.y * self.margin() / (2. * self.view.zoom),
        );
        WorldBounds {
            min: WorldPoint::new(self.view.center.x - half.x, self.view.center.y - half.y),
            max: WorldPoint::new(self.view.center.x + half.x, self.view.center.y + half.y),
        }
    }
    fn world_at(self, x: u32, y: u32) -> WorldPoint {
        // Compute offsets before adding the f64 center, retaining deep-zoom
        // coordinate precision. CPU and GPU use the same pixel-center mapping.
        WorldPoint::new(
            self.view.center.x
                + ((f64::from(x) + 0.5) / f64::from(self.dimensions[0]) - 0.5)
                    * self.view.viewport.x
                    * self.margin()
                    / self.view.zoom,
            self.view.center.y
                + ((f64::from(y) + 0.5) / f64::from(self.dimensions[1]) - 0.5)
                    * self.view.viewport.y
                    * self.margin()
                    / self.view.zoom,
        )
    }
}
fn palette(n: u32, limit: u32) -> [f32; 4] {
    let rgb = if n == limit {
        [5. / 255., 8. / 255., 16. / 255.]
    } else {
        let t = f64::from(n) * 0.11;
        [
            t.sin() * 0.5 + 0.5,
            (t + 2.).sin() * 0.5 + 0.5,
            (t + 4.).sin() * 0.5 + 0.5,
        ]
    };
    let linear = rgb.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    [linear[0] as f32, linear[1] as f32, linear[2] as f32, 1.]
}
fn render_cpu(request: Request, generation: &AtomicU64, token: u64) -> Option<Vec<[f32; 4]>> {
    let colors: Vec<_> = (0..=request.limit)
        .map(|n| palette(n, request.limit))
        .collect();
    let [width, height] = request.dimensions;
    let mut pixels = vec![[0.; 4]; width as usize * height as usize];
    let workers =
        std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).clamp(1, 4));
    let rows = (height as usize).div_ceil(workers);
    // Disjoint row strips share a single request and palette. They are joined
    // before publication; parallel calculation never exposes partial strips.
    std::thread::scope(|scope| {
        for (strip, pixels) in pixels.chunks_mut(rows * width as usize).enumerate() {
            let colors = &colors;
            scope.spawn(move || {
                for (index, pixel) in pixels.iter_mut().enumerate() {
                    if index % 32 == 0 && generation.load(Ordering::Relaxed) != token {
                        return;
                    }
                    let x = (index % width as usize) as u32;
                    let y = (strip * rows + index / width as usize) as u32;
                    let p = request.world_at(x, y);
                    let n = fractal_math::escape(p.x / 300. - 0.5, p.y / 300., request.limit);
                    *pixel = colors[n as usize];
                }
            });
        }
    });
    (generation.load(Ordering::Relaxed) == token).then_some(pixels)
}

#[derive(Clone)]
enum Image {
    Cpu(MediaImage),
    #[cfg(target_os = "macos")]
    NativeCpu(Arc<metal::CpuFrame>),
    #[cfg(target_os = "macos")]
    Gpu(Arc<metal::Frame>),
}
impl Image {
    fn ready(&self) -> bool {
        match self {
            Self::Cpu(image) => image.upload_status() == UploadStatus::Ready,
            #[cfg(target_os = "macos")]
            Self::NativeCpu(image) => image.ready(),
            #[cfg(target_os = "macos")]
            Self::Gpu(image) => image.ready(),
        }
    }
    fn failed(&self) -> Option<String> {
        match self {
            Self::Cpu(image) => {
                if let UploadStatus::Failed(error) = image.upload_status() {
                    Some(error)
                } else {
                    None
                }
            }
            #[cfg(target_os = "macos")]
            Self::NativeCpu(image) => image.error(),
            #[cfg(target_os = "macos")]
            Self::Gpu(image) => image.error(),
        }
    }
}
#[derive(Clone)]
struct Frame {
    request: Request,
    image: Image,
}

// A pending frame is prepared with zero sampling opacity. It cannot cover the
// front image with a placeholder, and its source-sized texture bounds stay fixed.
struct FrameContent {
    frame: Frame,
    visible: bool,
    wide: bool,
}
impl std::fmt::Debug for FrameContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MandelbrotFrame")
            .field("request", &self.frame.request)
            .finish()
    }
}
impl CustomContent for FrameContent {
    fn image_source_size(&self) -> Option<Size<DevicePixels>> {
        let [w, h] = self.frame.request.dimensions;
        Some(size(DevicePixels(w as i32), DevicePixels(h as i32)))
    }
    fn image_sample_opacity(&self) -> f32 {
        if self.visible { 1. } else { 0. }
    }
    fn image_color_mode(&self) -> ImageColorMode {
        if self.wide {
            ImageColorMode::LinearP3Wide
        } else {
            ImageColorMode::LinearP3Preview
        }
    }
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        extent: Size<DevicePixels>,
    ) -> anyhow::Result<Box<dyn std::any::Any>> {
        match &self.frame.image {
            Image::Cpu(image) => image.prepare(backend, extent),
            #[cfg(target_os = "macos")]
            Image::NativeCpu(image) => image.prepare(backend),
            #[cfg(target_os = "macos")]
            Image::Gpu(image) => image.prepare(backend),
        }
    }
    fn prepare_in_frame(
        &self,
        backend: &dyn std::any::Any,
        request: &PreparationRequest,
        frame: &PreparationFrame,
    ) -> anyhow::Result<Box<dyn std::any::Any>> {
        match &self.frame.image {
            Image::Cpu(image) => image.prepare_in_frame(backend, request, frame),
            #[cfg(target_os = "macos")]
            Image::NativeCpu(image) => image.prepare(backend),
            #[cfg(target_os = "macos")]
            Image::Gpu(image) => image.prepare(backend),
        }
    }
}

pub(super) struct Mandelbrot {
    pub enabled: bool,
    pub capture: Option<PathBuf>,
    pub refined: bool,
    force_cpu: bool,
    check: Option<check::Check>,
    current: Option<View>,
    changed_at: Instant,
    front: Option<Frame>,
    sharpening: Option<(Frame, Instant)>,
    pending: Option<Frame>,
    busy: Option<Request>,
    generation: Arc<AtomicU64>,
    loader: MediaLoader,
    error: Option<String>,
    #[cfg(target_os = "macos")]
    engine: Arc<metal::Engine>,
}
impl Mandelbrot {
    pub fn new(
        enabled: bool,
        capture: Option<PathBuf>,
        force_cpu: bool,
        check: Option<PathBuf>,
    ) -> Self {
        Self {
            enabled,
            capture,
            refined: false,
            force_cpu,
            check: check.map(check::Check::new),
            current: None,
            changed_at: Instant::now(),
            front: None,
            sharpening: None,
            pending: None,
            busy: None,
            generation: Arc::new(AtomicU64::new(0)),
            loader: MediaLoader::new(MediaLimits {
                max_jobs: 1,
                max_image_bytes: 16 * 1024 * 1024,
                max_resident_bytes: 96 * 1024 * 1024,
                ..Default::default()
            })
            .expect("viewport loader limits"),
            error: None,
            #[cfg(target_os = "macos")]
            engine: Arc::new(metal::Engine::default()),
        }
    }
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.current = None;
        self.pending = None;
        self.sharpening = None;
        self.refined = false;
        self.error = None;
    }
    pub fn ready_for_capture(&self) -> bool {
        self.refined && self.sharpening.is_none()
    }
    pub fn status(&self) -> String {
        if let Some(error) = &self.error {
            return format!("Frame error: {error}");
        }
        let Some(view) = self.current else {
            return "Preparing viewport".into();
        };
        let route = if self.gpu(view) {
            "Metal GPU"
        } else {
            "f64 CPU"
        };
        let dimensions = self
            .front
            .as_ref()
            .map(|f| format!("{}×{}", f.request.dimensions[0], f.request.dimensions[1]))
            .unwrap_or_else(|| "starting".into());
        format!(
            "{route} • {dimensions} • {} iterations • {}",
            iterations(view.zoom),
            if self.refined {
                "refined"
            } else {
                "interactive"
            }
        )
    }
    fn gpu(&self, view: View) -> bool {
        let center = (view.center.x / 300. - 0.5)
            .abs()
            .max((view.center.y / 300.).abs())
            .max(1.);
        let pixel_step = 1. / (view.zoom * 300. * f64::from(view.scale));
        cfg!(target_os = "macos")
            && !self.force_cpu
            && view.zoom <= GPU_MAX_ZOOM
            && center * f64::from(f32::EPSILON) * 16. < pixel_step
    }
}
// Only a complete frame may become the front. Obsolete coarse results can still
// give useful coverage while input is moving, but stale detail is never revealed.
fn accept(request: Request, current: View) -> bool {
    if request.detail {
        return request.view == current;
    }
    let ratio = current.zoom / request.view.zoom;
    (1. / MAX_REPROJECTION..=MAX_REPROJECTION).contains(&ratio)
        && request.bounds().intersects(current.bounds())
}
// Do not replace a sharp completed image with a lower-density preview while
// that image still covers the view. Reproject it until a replacement helps.
fn retain_sharper_front(front: Request, next: Request, current: View) -> bool {
    if next.gpu || next.detail {
        return false;
    }
    let ratio = current.zoom / front.view.zoom;
    if !(1. / MAX_REPROJECTION..=MAX_REPROJECTION).contains(&ratio) {
        return false;
    }
    let half = WorldPoint::new(
        current.viewport.x / (2. * current.zoom),
        current.viewport.y / (2. * current.zoom),
    );
    let coverage = front.bounds();
    let covers = coverage.contains(WorldPoint::new(
        current.center.x - half.x,
        current.center.y - half.y,
    )) && coverage.contains(WorldPoint::new(
        current.center.x + half.x,
        current.center.y + half.y,
    ));
    let density =
        |r: Request| f64::from(r.dimensions[0]) * r.view.zoom / (r.view.viewport.x * r.margin());
    covers && density(front) > density(next)
}

impl Lab {
    pub(super) fn check_mandelbrot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut check) = self.fractal.check.take()
            && check.tick(self, window, cx)
        {
            self.fractal.check = Some(check);
        }
    }
    pub(super) fn schedule_mandelbrot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let viewport = self.viewport();
        if !self.fractal.enabled || viewport.x <= 0. || viewport.y <= 0. {
            return;
        }
        // The native GPU producer has an explicit wide-SDR output contract.
        // MediaImage provides the mapped sRGB preview on other hosts.
        if window.color_status().effective() != WindowColorMode::WideGamutSdr {
            self.fractal.force_cpu = true;
        }
        let view = View::new(self.camera, viewport, window.scale_factor());
        if self.fractal.current != Some(view) {
            self.fractal.current = Some(view);
            self.fractal.sharpening = None;
            self.fractal.changed_at = Instant::now();
            self.fractal.refined = false;
            self.fractal.error = None;
            // Fine work is useful only for a settled view. Let the tiny coarse
            // frame finish across input events to avoid continuous cancellation.
            if self
                .fractal
                .busy
                .is_some_and(|r| r.detail || self.fractal.gpu(view))
            {
                self.fractal.generation.fetch_add(1, Ordering::Relaxed);
            }
        }
        if self.fractal.gpu(view)
            && self
                .fractal
                .pending
                .as_ref()
                .is_some_and(|frame| !frame.request.gpu)
        {
            self.fractal.pending = None;
        }
        if let Some(pending) = &self.fractal.pending {
            if let Some(error) = pending.image.failed() {
                eprintln!("Mandelbrot frame: {error}");
                if pending.request.gpu {
                    self.fractal.force_cpu = true;
                } else {
                    self.fractal.error = Some(error);
                }
                self.fractal.pending = None;
            } else if pending.image.ready() {
                let keep_sharp = self.fractal.front.as_ref().is_some_and(|front| {
                    retain_sharper_front(front.request, pending.request, view)
                });
                if keep_sharp {
                    // Keep a warmed replacement in reserve so zoom-out can use
                    // it immediately when the sharper image stops covering us.
                    // Refresh stale reserves, or release them for settled detail.
                    let age = view.zoom / pending.request.view.zoom;
                    if self.fractal.changed_at.elapsed() >= SETTLE || !(0.5..=2.).contains(&age) {
                        self.fractal.pending = None;
                    }
                } else {
                    let complete = self.fractal.pending.take().unwrap();
                    if accept(complete.request, view) {
                        if !complete.request.gpu && complete.request.detail {
                            self.fractal.sharpening = self
                                .fractal
                                .front
                                .as_ref()
                                .filter(|old| {
                                    (0.5..=2.).contains(&(view.zoom / old.request.view.zoom))
                                })
                                .cloned()
                                .map(|old| (old, Instant::now()));
                        }
                        self.fractal.front = Some(complete);
                        self.invalidate_preview();
                    }
                }
            }
        }
        if self
            .fractal
            .sharpening
            .as_ref()
            .is_some_and(|(_, started)| started.elapsed() >= SHARPEN_BLEND)
        {
            self.fractal.sharpening = None;
        }
        if self.fractal.sharpening.is_some() {
            window.request_animation_frame();
        }
        self.fractal.refined = self
            .fractal
            .front
            .as_ref()
            .is_some_and(|f| f.request.view == view && (f.request.detail || f.request.gpu));
        if self.fractal.refined || self.fractal.error.is_some() {
            return;
        }
        window.request_animation_frame();
        if self.fractal.busy.is_some() || self.fractal.pending.is_some() {
            return;
        }
        let settled = self.fractal.changed_at.elapsed() >= SETTLE;
        if !settled
            && self
                .fractal
                .front
                .as_ref()
                .is_some_and(|f| f.request.view == view)
        {
            return;
        }
        let request = Request::new(view, settled, self.fractal.gpu(view));
        #[cfg(target_os = "macos")]
        if request.gpu {
            self.fractal.pending = Some(Frame {
                request,
                image: Image::Gpu(Arc::new(metal::Frame::new(
                    request,
                    self.fractal.engine.clone(),
                ))),
            });
            return;
        }
        self.fractal.busy = Some(request);
        let generation = self.fractal.generation.clone();
        let token = generation.load(Ordering::Relaxed);
        let loader = self.fractal.loader.clone();
        #[cfg(target_os = "macos")]
        let native_upload = window.color_status().effective() == WindowColorMode::WideGamutSdr;
        cx.spawn(async move |this, cx| {
            let result = async {
                let cancel = generation.clone();
                let pixels = cx
                    .background_executor()
                    .spawn(async move { render_cpu(request, &cancel, token) })
                    .await;
                let Some(pixels) = pixels else {
                    return Ok(None);
                };
                if generation.load(Ordering::Relaxed) != token {
                    return Ok(None);
                }
                let [width, height] = request.dimensions;
                #[cfg(target_os = "macos")]
                if native_upload {
                    let data = cx
                        .background_executor()
                        .spawn(async move {
                            xgpui_media::pipeline::rgba16f_data(
                                width,
                                height,
                                pixels,
                                ImageColor::LINEAR_P3,
                            )
                        })
                        .await?;
                    return Ok(Some(Image::NativeCpu(Arc::new(metal::CpuFrame::new(data)))));
                }
                let mut job = loader.load_rgba16f(width, height, pixels, ImageColor::LINEAR_P3)?;
                loop {
                    if generation.load(Ordering::Relaxed) != token {
                        job.cancel();
                        return Ok(None);
                    }
                    if let Some(image) = job.try_take() {
                        return Ok(Some(Image::Cpu(image)));
                    }
                    match job.status() {
                        LoadStatus::Loading => (),
                        other => anyhow::bail!("frame conversion: {other:?}"),
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(3))
                        .await;
                }
            }
            .await;
            let _ = this.update(cx, |lab, cx| {
                lab.fractal.busy = None;
                if lab.fractal.enabled && generation.load(Ordering::Relaxed) == token {
                    match result {
                        Ok(Some(image))
                            if lab
                                .fractal
                                .current
                                .is_some_and(|view| accept(request, view)) =>
                        {
                            lab.fractal.pending = Some(Frame { request, image })
                        }
                        Err(error) => {
                            eprintln!("Mandelbrot CPU frame: {error}");
                            lab.fractal.error = Some(error.to_string());
                        }
                        _ => (),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn fractal_element(&self) -> AnyElement {
        let camera = self.camera;
        let viewport = self.viewport();
        let mut images = Vec::with_capacity(2);
        let opacity = if let Some((old, started)) = &self.fractal.sharpening {
            images.push((old.clone(), 1.));
            sharpen_weight(started.elapsed())
        } else {
            1.
        };
        if let Some(front) = &self.fractal.front {
            images.push((front.clone(), opacity));
        }
        let pending = self.fractal.pending.clone();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                window.paint_quad(fill(bounds, rgb(0x050810)));
                let wide = window.color_status().effective() == WindowColorMode::WideGamutSdr;
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    if let Some(frame) = pending {
                        // Prepare the complete successor without sampling it yet.
                        window.paint_custom(
                            CustomGeometry::RoundedRect {
                                bounds,
                                corner_radii: Corners::default(),
                            },
                            Arc::new(FrameContent {
                                frame,
                                visible: false,
                                wide,
                            }),
                        );
                    }
                    for (frame, opacity) in images {
                        // Whole-image reprojection has no tile boundaries. Limit
                        // stale snapshot geometry after very large camera jumps.
                        let ratio = camera.zoom() / frame.request.view.zoom;
                        if !(1. / MAX_REPROJECTION..=MAX_REPROJECTION).contains(&ratio) {
                            continue;
                        }
                        let world = frame.request.bounds();
                        let a = camera.world_to_screen(world.min, viewport);
                        let b = camera.world_to_screen(world.max, viewport);
                        let target = Bounds::from_corners(
                            bounds.origin + point(px(a.x as f32), px(a.y as f32)),
                            bounds.origin + point(px(b.x as f32), px(b.y as f32)),
                        );
                        window.with_element_opacity(Some(opacity), |window| {
                            window.paint_custom(
                                CustomGeometry::RoundedRect {
                                    bounds: target,
                                    corner_radii: Corners::default(),
                                },
                                Arc::new(FrameContent {
                                    frame,
                                    visible: true,
                                    wide,
                                }),
                            );
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    }
}

fn sharpen_weight(elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / SHARPEN_BLEND.as_secs_f32()).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view(zoom: f64) -> View {
        View::new(Camera2D::default(), WorldPoint::new(1100., 600.), 2.).with_zoom(zoom)
    }
    impl View {
        fn with_zoom(mut self, zoom: f64) -> Self {
            self.zoom = zoom;
            self
        }
    }
    #[::core::prelude::v1::test]
    fn cpu_navigation_keeps_sharp_coverage_instead_of_downgrading_to_preview() {
        let previous = Request::new(view(100.), true, false);
        let current = view(110.);
        assert!(retain_sharper_front(
            previous,
            Request::new(current, false, false),
            current
        ));
        assert!(!retain_sharper_front(
            previous,
            Request::new(current, true, false),
            current
        ));
        let far = view(10000.);
        assert!(!retain_sharper_front(
            previous,
            Request::new(far, false, false),
            far
        ));
        let out = view(20.);
        assert!(!retain_sharper_front(
            previous,
            Request::new(out, false, false),
            out
        ));
    }
    #[::core::prelude::v1::test]
    fn sharpening_blend_is_bounded_and_monotone() {
        assert_eq!(sharpen_weight(Duration::ZERO), 0.);
        assert_eq!(sharpen_weight(SHARPEN_BLEND), 1.);
        assert_eq!(sharpen_weight(SHARPEN_BLEND * 2), 1.);
        let weights: Vec<_> = (0..=180)
            .map(|ms| sharpen_weight(Duration::from_millis(ms)))
            .collect();
        assert!(weights.windows(2).all(|w| w[0] <= w[1]));
    }
    #[::core::prelude::v1::test]
    fn pixel_centers_and_presented_coverage_use_the_same_guard_band() {
        for detail in [false, true] {
            let request = Request::new(view(2.), detail, false);
            let bounds = request.bounds();
            let first = request.world_at(0, 0);
            let last = request.world_at(request.dimensions[0] - 1, request.dimensions[1] - 1);
            let step = (bounds.max.x - bounds.min.x) / f64::from(request.dimensions[0]);
            assert!((first.x - bounds.min.x - step / 2.).abs() < 1e-10);
            assert!((bounds.max.x - last.x - step / 2.).abs() < 1e-10);
        }
    }
    #[::core::prelude::v1::test]
    fn obsolete_detail_never_replaces_current_view() {
        let old = view(2.);
        let new = view(2.1);
        assert!(!accept(Request::new(old, true, false), new));
        assert!(accept(Request::new(old, false, false), new));
        assert!(!accept(Request::new(old, false, false), view(100.)));
    }
    #[::core::prelude::v1::test]
    fn resolution_is_bounded_through_the_entire_zoom_range() {
        for zoom in [0.01, 1., 64., 1e5, 1e10] {
            for gpu in [false, true] {
                let coarse = Request::new(view(zoom), false, gpu);
                let fine = Request::new(view(zoom), true, gpu);
                assert!(coarse.dimensions[0] <= 1920 && coarse.dimensions[1] <= 1920);
                assert!(fine.dimensions[0] <= 1920 && fine.dimensions[1] <= 1920);
                assert_eq!(
                    coarse.limit, fine.limit,
                    "refinement must not change iteration/color semantics"
                );
                if gpu {
                    assert_eq!(coarse.dimensions, fine.dimensions);
                    assert_eq!(coarse.bounds().min, fine.bounds().min);
                    assert_eq!(coarse.bounds().max, fine.bounds().max);
                } else {
                    assert!(fine.dimensions[0] > coarse.dimensions[0]);
                }
            }
        }
    }
    #[::core::prelude::v1::test]
    fn deep_zoom_pixels_remain_distinct_and_cpu_cancels() {
        let request = Request::new(view(1e10), true, false);
        assert_ne!(request.world_at(10, 10), request.world_at(11, 10));
        assert!(render_cpu(request, &AtomicU64::new(2), 1).is_none());
        assert_eq!(fractal_math::escape(0., 0., 256), 256);
    }
    #[::core::prelude::v1::test]
    fn whole_frame_samples_match_independent_pixel_reference() {
        let request = Request {
            dimensions: [9, 7],
            ..Request::new(view(1.), false, false)
        };
        let pixels = render_cpu(request, &AtomicU64::new(1), 1).unwrap();
        for (x, y) in [(0, 0), (4, 3), (8, 6)] {
            let p = request.world_at(x, y);
            let n = fractal_math::escape(p.x / 300. - 0.5, p.y / 300., request.limit);
            assert_eq!(pixels[(y * 9 + x) as usize], palette(n, request.limit));
        }
    }
}
