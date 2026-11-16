//! WebKit Flowers: a ProPhoto RGB photograph through xgpui's named ProPhoto
//! pipeline (#107). JPEG channels are decoded raw (the `image` crate applies no
//! color management) and interpreted explicitly; nothing here relies on an ICC
//! engine. Storage stays bounded linear P3 or Rec.2020, so this is never full
//! ProPhoto output.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use xgpui_color::EncodedRgb;
use xgpui_media::pipeline::PixelColor;

const SOURCE: &str = "https://webkit.org/blog-files/color-gamut/";
const PROPHOTO_JPEG: &[u8] =
    include_bytes!("../../../extensions-web/assets/flowers/Flowers-ProPhoto.jpg");
const SRGB_JPEG: &[u8] = include_bytes!("../../../extensions-web/assets/flowers/Flowers-sRGB.jpg");
const OOG_JPEG: &[u8] = include_bytes!("../../../extensions-web/assets/flowers/Flowers-oog.jpg");
const WIDTH: u32 = 1200;
const HEIGHT: u32 = 795;
/// The embedded Kodak "ProPhoto RGB" v2 profile stores a pure gamma of
/// u8Fixed8 0x01CD = 461/256 with no linear toe; ROMM adds the toe below 16/512.
const PROFILE_GAMMA: f64 = 461. / 256.;
const STORAGE: [ImagePrimaries; 2] = [ImagePrimaries::DisplayP3, ImagePrimaries::Rec2020];
const MAPPING: [GamutMapping; 2] = [GamutMapping::Clip, GamutMapping::ReduceChroma];
const ZOOM: [f32; 4] = [1., 2., 4., 8.];
const PATCH: u32 = 8;
/// Initial inspected pixel: a red petal outside Display P3 but inside Rec.2020
/// (its 5 × 5 neighbourhood too), right of the default 50% wipe, so the
/// inspector and readback start on a mapped ProPhoto color.
const DEFAULT_SELECTION: (u32, u32) = (703, 250);
/// Synthetic companions: neutral/near-black ramps (including both sides of the
/// 16/512 knee), then primaries, secondaries, in/out-of-storage colors and
/// fractional alpha. Encoded ProPhoto RGBA.
const PATCHES: [[[f32; 4]; 13]; 2] = [
    [
        [0., 0., 0., 1.],
        [0.005, 0.005, 0.005, 1.],
        [0.01, 0.01, 0.01, 1.],
        [0.02, 0.02, 0.02, 1.],
        [0.03, 0.03, 0.03, 1.],
        [0.03125, 0.03125, 0.03125, 1.],
        [0.04, 0.04, 0.04, 1.],
        [0.1, 0.1, 0.1, 1.],
        [0.18, 0.18, 0.18, 1.],
        [0.4, 0.4, 0.4, 1.],
        [0.7, 0.7, 0.7, 1.],
        [1., 1., 1., 1.],
        [1., 1., 1., 0.5],
    ],
    [
        [1., 0., 0., 1.],
        [0., 1., 0., 1.],
        [0., 0., 1., 1.],
        [0., 1., 1., 1.],
        [1., 0., 1., 1.],
        [1., 1., 0., 1.],
        [0.6, 0.4, 0.3, 1.],
        [0.2, 0.8, 0.2, 1.],
        [0.5, 0.1, 0.6, 1.],
        [1., 0., 0., 0.75],
        [1., 0., 0., 0.5],
        [1., 0., 0., 0.25],
        [1., 0., 0., 0.],
    ],
];

/// Raw 8-bit channels exactly as stored in the JPEG.
pub(super) struct Decoded {
    width: u32,
    height: u32,
    codes: Vec<[u8; 3]>,
}
pub(super) fn decode(bytes: &[u8]) -> anyhow::Result<Decoded> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)?.to_rgb8();
    Ok(Decoded {
        width: image.width(),
        height: image.height(),
        codes: image.pixels().map(|p| p.0).collect(),
    })
}
/// The JPEG's embedded ICC profile bytes, for provenance checks only.
#[cfg(test)]
fn embedded_profile(bytes: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
    use image::ImageDecoder;
    Ok(image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(bytes))?.icc_profile()?)
}

/// Pre-mapping classification. Containment uses the same matrices as the
/// pipeline; `TOLERANCE` absorbs binary64 rounding at the cube faces only.
const TOLERANCE: f64 = 1e-9;
pub(super) struct Analysis {
    pub(super) pixels: usize,
    /// Pixels outside sRGB, Display P3 and Rec.2020 before any mapping.
    pub(super) outside: [usize; 3],
    /// Largest linear difference between the named ROMM decode and the embedded
    /// profile's pure-gamma decode: (8-bit codes 0..=7, codes 8..=255).
    pub(super) profile_delta: (f64, f64),
}
fn romm_linear() -> [f64; 256] {
    std::array::from_fn(|code| {
        EncodedRgb::new(RgbSpace::ProPhotoRgb, [code as f64 / 255.; 3])
            .and_then(EncodedRgb::decode)
            .map(|c| c.components()[0])
            .unwrap_or(0.)
    })
}
fn mul(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}
fn inside(v: [f64; 3]) -> bool {
    v.iter().all(|c| (-TOLERANCE..=1. + TOLERANCE).contains(c))
}
pub(super) fn analyze(source: &Decoded) -> Analysis {
    let linear = romm_linear();
    let matrices = [RgbSpace::Srgb, RgbSpace::DisplayP3, RgbSpace::Rec2020]
        .map(|space| RgbSpace::ProPhotoRgb.linear_to_linear_matrix(space));
    let mut outside = [0; 3];
    for codes in &source.codes {
        let v = codes.map(|c| linear[c as usize]);
        for (count, matrix) in outside.iter_mut().zip(matrices) {
            *count += usize::from(!inside(mul(matrix, v)));
        }
    }
    let mut profile_delta = (0_f64, 0_f64);
    for (code, romm) in linear.iter().enumerate() {
        let delta = (romm - (code as f64 / 255.).powf(PROFILE_GAMMA)).abs();
        if code < 8 {
            profile_delta.0 = profile_delta.0.max(delta);
        } else {
            profile_delta.1 = profile_delta.1.max(delta);
        }
    }
    Analysis {
        pixels: source.codes.len(),
        outside,
        profile_delta,
    }
}
/// Our own pre-mapping overlay: a dimmed luminance photograph, cyan where the
/// source leaves sRGB but fits storage, magenta where it leaves storage too.
fn overlay(source: &Decoded, storage: ImagePrimaries) -> Vec<[f32; 4]> {
    let linear = romm_linear();
    let to_srgb = RgbSpace::ProPhotoRgb.linear_to_linear_matrix(RgbSpace::Srgb);
    let space = if storage == ImagePrimaries::Rec2020 {
        RgbSpace::Rec2020
    } else {
        RgbSpace::DisplayP3
    };
    let to_storage = RgbSpace::ProPhotoRgb.linear_to_linear_matrix(space);
    let y = RgbSpace::ProPhotoRgb.linear_to_xyz_matrix()[1];
    source
        .codes
        .iter()
        .map(|codes| {
            let v = codes.map(|c| linear[c as usize]);
            if !inside(mul(to_storage, v)) {
                [1., 0., 1., 1.]
            } else if !inside(mul(to_srgb, v)) {
                [0., 1., 1., 1.]
            } else {
                let luminance = (y[0] * v[0] + y[1] * v[1] + y[2] * v[2]).clamp(0., 1.);
                let gray = (0.25 + 0.5 * luminance.powf(1. / 2.2)) as f32;
                [gray, gray, gray, 1.]
            }
        })
        .collect()
}
fn encoded(source: &Decoded) -> Vec<[f32; 4]> {
    source
        .codes
        .iter()
        .map(|c| [c[0], c[1], c[2]].map(|v| v as f32 / 255.))
        .map(|[r, g, b]| [r, g, b, 1.])
        .collect()
}
fn patch_pixels() -> Vec<[f32; 4]> {
    let mut pixels = Vec::new();
    for row in PATCHES {
        for _ in 0..PATCH {
            for patch in row {
                pixels.extend(std::iter::repeat_n(patch, PATCH as usize));
            }
        }
    }
    pixels
}
/// Image rectangle for a zoom factor, centered on the selected pixel and
/// clamped so the image always covers the view.
fn placement(view: Bounds<Pixels>, zoom: f32, (x, y): (u32, u32)) -> Bounds<Pixels> {
    let size = size(view.size.width * zoom, view.size.height * zoom);
    let focus = point(
        size.width * ((x as f32 + 0.5) / WIDTH as f32),
        size.height * ((y as f32 + 0.5) / HEIGHT as f32),
    );
    let clamp =
        |offset: Pixels, extent: Pixels, full: Pixels| offset.min(px(0.)).max(extent - full);
    let origin = point(
        clamp(view.size.width / 2. - focus.x, view.size.width, size.width),
        clamp(
            view.size.height / 2. - focus.y,
            view.size.height,
            size.height,
        ),
    );
    Bounds::new(view.origin + origin, size)
}
fn variant(storage: usize, mapping: usize) -> usize {
    storage * 2 + mapping
}

/// Loader slots. 0 sRGB reference; 1..=4 ProPhoto variants; 5 WebKit's
/// out-of-gamut visualization; 6..=7 our overlays; 8..=11 patch variants.
const SRGB: usize = 0;
const PROPHOTO: usize = 1;
const OOG: usize = 5;
const OVERLAY: usize = 6;
const PATCHES_SLOT: usize = 8;
const SLOTS: usize = 12;
pub(super) struct Prepared {
    source: Arc<Decoded>,
    reference: Arc<Decoded>,
    oog_size: (u32, u32),
    analysis: Analysis,
    jobs: Vec<ImageJob>,
}
pub(super) fn prepare(cancelled: &AtomicBool) -> anyhow::Result<Prepared> {
    let stop = || -> anyhow::Result<()> {
        anyhow::ensure!(
            !cancelled.load(Ordering::Relaxed),
            "Flowers preparation cancelled"
        );
        Ok(())
    };
    let source = decode(PROPHOTO_JPEG)?;
    anyhow::ensure!(
        (source.width, source.height) == (WIDTH, HEIGHT),
        "unexpected Flowers size"
    );
    let reference = decode(SRGB_JPEG)?;
    anyhow::ensure!(
        (reference.width, reference.height) == (WIDTH, HEIGHT),
        "unexpected Flowers sRGB size"
    );
    let oog = decode(OOG_JPEG)?;
    stop()?;
    let analysis = analyze(&source);
    stop()?;
    // Each admitted job reserves `max_image_bytes`; size it to the largest
    // actual payload (WebKit's 1644 × 1088 visualization) so all slots fit.
    let largest = xgpui_media::pipeline::rgba16f_payload_bytes(oog.width, oog.height)?;
    let loader = MediaLoader::new(xgpui_media::MediaLimits {
        max_jobs: SLOTS,
        max_image_bytes: largest,
        max_resident_bytes: largest * SLOTS,
        ..Default::default()
    })?;
    let mut jobs = Vec::with_capacity(SLOTS);
    let srgb = || PixelColor::from(ImageColor::SRGB);
    jobs.push(loader.load_pixel_buffer(
        WIDTH,
        HEIGHT,
        encoded(&reference),
        srgb(),
        ImagePrimaries::DisplayP3,
        GamutMapping::Clip,
    )?);
    let prophoto = encoded(&source);
    for storage in STORAGE {
        for mapping in MAPPING {
            stop()?;
            jobs.push(loader.load_pixel_buffer(
                WIDTH,
                HEIGHT,
                prophoto.clone(),
                ImageColor::PROPHOTO_RGB.into(),
                storage,
                mapping,
            )?);
        }
    }
    // WebKit's visualization embeds a display profile from a screen capture.
    // It is a reference picture only, shown as encoded sRGB.
    jobs.push(loader.load_pixel_buffer(
        oog.width,
        oog.height,
        encoded(&oog),
        srgb(),
        ImagePrimaries::DisplayP3,
        GamutMapping::Clip,
    )?);
    for storage in STORAGE {
        stop()?;
        jobs.push(loader.load_pixel_buffer(
            WIDTH,
            HEIGHT,
            overlay(&source, storage),
            srgb(),
            ImagePrimaries::DisplayP3,
            GamutMapping::Clip,
        )?);
    }
    let patches = patch_pixels();
    for storage in STORAGE {
        for mapping in MAPPING {
            jobs.push(loader.load_pixel_buffer(
                13 * PATCH,
                2 * PATCH,
                patches.clone(),
                ImageColor::PROPHOTO_RGB.into(),
                storage,
                mapping,
            )?);
        }
    }
    Ok(Prepared {
        source: Arc::new(source),
        reference: Arc::new(reference),
        oog_size: (oog.width, oog.height),
        analysis,
        jobs,
    })
}

/// CPU reference values for one source pixel, through the same named pipeline.
pub(super) struct Inspect {
    pub(super) codes: [u8; 3],
    pub(super) reference: [u8; 3],
    pub(super) linear: [f64; 3],
    pub(super) profile: [f64; 3],
    pub(super) xyz: [f64; 3],
    /// Destination linear values before mapping: sRGB, P3, Rec.2020.
    pub(super) before: [[f64; 3]; 3],
    /// Associated values written to the selected storage.
    pub(super) stored: [f64; 4],
    /// Stored values as extended linear P3, the native composition space.
    pub(super) composed: [f64; 3],
}
pub(super) fn inspect(
    source: &Decoded,
    reference: &Decoded,
    x: u32,
    y: u32,
    storage: ImagePrimaries,
    mapping: GamutMapping,
) -> anyhow::Result<Inspect> {
    let index = (y * source.width + x) as usize;
    let codes = source.codes[index];
    let encoded = codes.map(|c| c as f64 / 255.);
    let linear = EncodedRgb::new(RgbSpace::ProPhotoRgb, encoded)?.decode()?;
    let before = [RgbSpace::Srgb, RgbSpace::DisplayP3, RgbSpace::Rec2020]
        .map(|space| linear.convert(space).map(|c| c.components()));
    let canonical = ImageColor {
        alpha: AlphaMode::Premultiplied,
        primaries: storage,
        transfer: TransferFunction::Linear,
    };
    let stored = ImageColor::PROPHOTO_RGB
        .decode([encoded[0], encoded[1], encoded[2], 1.], storage)?
        .output(canonical, mapping)?;
    let space = if storage == ImagePrimaries::Rec2020 {
        RgbSpace::Rec2020
    } else {
        RgbSpace::DisplayP3
    };
    let composed = LinearRgb::new(space, [stored[0], stored[1], stored[2]])?
        .convert(RgbSpace::DisplayP3)?
        .components();
    Ok(Inspect {
        codes,
        reference: reference.codes[index],
        linear: linear.components(),
        profile: encoded.map(|v| v.powf(PROFILE_GAMMA)),
        xyz: linear.to_xyz()?.components(),
        before: [before[0]?, before[1]?, before[2]?],
        stored,
        composed,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Wipe,
    Overlay,
    WebKit,
}
pub(super) struct Flowers {
    receiver: Option<mpsc::Receiver<anyhow::Result<Prepared>>>,
    cancelled: Arc<AtomicBool>,
    source: Option<Arc<Decoded>>,
    reference: Option<Arc<Decoded>>,
    analysis: Option<Analysis>,
    oog_aspect: f32,
    jobs: Vec<Option<ImageJob>>,
    images: Vec<Option<MediaImage>>,
    status: String,
    error: Option<String>,
    storage: usize,
    mapping: usize,
    preview: bool,
    view: View,
    zoom: usize,
    wipe: f32,
    blur: bool,
    selected: (u32, u32),
    inspection: Option<anyhow::Result<Inspect>>,
    viewport: Arc<Mutex<Option<Bounds<Pixels>>>>,
    report: String,
}
impl Drop for Flowers {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
impl Flowers {
    pub(super) fn new() -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(prepare(&flag));
        });
        Self {
            receiver: Some(receiver),
            cancelled,
            source: None,
            reference: None,
            analysis: None,
            oog_aspect: WIDTH as f32 / HEIGHT as f32,
            jobs: (0..SLOTS).map(|_| None).collect(),
            images: vec![None; SLOTS],
            status: "Decoding WebKit’s Flowers JPEGs and preparing ProPhoto storage variants…"
                .into(),
            error: None,
            storage: 0,
            mapping: 1,
            preview: false,
            view: View::Wipe,
            zoom: 0,
            wipe: 0.5,
            blur: false,
            selected: DEFAULT_SELECTION,
            inspection: None,
            viewport: Arc::new(Mutex::new(None)),
            report: "Native readback: not checked.".into(),
        }
    }
    fn storage(&self) -> ImagePrimaries {
        STORAGE[self.storage]
    }
    fn mapping(&self) -> GamutMapping {
        MAPPING[self.mapping]
    }
    fn reinspect(&mut self) {
        if let (Some(source), Some(reference)) = (&self.source, &self.reference) {
            let (x, y) = self.selected;
            self.inspection = Some(inspect(
                source,
                reference,
                x,
                y,
                self.storage(),
                self.mapping(),
            ));
        }
    }
    fn poll(&mut self, window: &mut Window) {
        if let Some(receiver) = &self.receiver {
            match receiver.try_recv() {
                Ok(Ok(prepared)) => {
                    self.status = "WebKit’s Flowers: ProPhoto and sRGB JPEGs, 1200 × 795, raw 8-bit channels decoded without color management; WebKit’s out-of-sRGB visualization for reference.".into();
                    self.source = Some(prepared.source);
                    self.reference = Some(prepared.reference);
                    self.analysis = Some(prepared.analysis);
                    self.oog_aspect = prepared.oog_size.0 as f32 / prepared.oog_size.1 as f32;
                    self.jobs = prepared.jobs.into_iter().map(Some).collect();
                    self.receiver = None;
                    self.reinspect();
                }
                Ok(Err(error)) => {
                    self.status = format!("Flowers preparation failed: {error:#}");
                    self.error = Some(self.status.clone());
                    self.receiver = None;
                }
                Err(mpsc::TryRecvError::Empty) => window.request_animation_frame(),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = "Flowers preparation failed: worker stopped".into();
                    self.error = Some(self.status.clone());
                    self.receiver = None;
                }
            }
        }
        for (slot, image) in self.jobs.iter_mut().zip(&mut self.images) {
            let Some(job) = slot else { continue };
            if let Some(ready) = job.try_take() {
                *image = Some(ready);
                *slot = None;
                continue;
            }
            match job.status() {
                xgpui_media::LoadStatus::Loading => window.request_animation_frame(),
                xgpui_media::LoadStatus::Failed(error) => self.error = Some(error),
                _ => {}
            }
        }
    }
    fn placement(&self, view: Bounds<Pixels>) -> Bounds<Pixels> {
        placement(view, ZOOM[self.zoom], self.selected)
    }
    fn select_at(&mut self, position: Point<Pixels>) -> bool {
        let Some(view) = *self.viewport.lock().unwrap() else {
            return false;
        };
        if !view.contains(&position) {
            return false;
        }
        let image = self.placement(view);
        let fx = f32::from(position.x - image.origin.x) / f32::from(image.size.width);
        let fy = f32::from(position.y - image.origin.y) / f32::from(image.size.height);
        self.selected = (
            ((fx * WIDTH as f32) as u32).min(WIDTH - 1),
            ((fy * HEIGHT as f32) as u32).min(HEIGHT - 1),
        );
        self.reinspect();
        true
    }
    fn paint_image(image: &MediaImage, geometry: Bounds<Pixels>, wide: bool, window: &mut Window) {
        wake_for_upload(image, geometry, window);
        let geometry = CustomGeometry::RoundedRect {
            bounds: geometry,
            corner_radii: Default::default(),
        };
        if wide && let Ok(image) = image.wide(None) {
            window.paint_custom(geometry, Arc::new(image));
            return;
        }
        window.paint_custom(geometry, Arc::new(image.clone()));
    }
    fn main_view(&self, wide: bool) -> impl IntoElement {
        let images = self.images.clone();
        let viewport = self.viewport.clone();
        let prophoto = PROPHOTO + variant(self.storage, self.mapping);
        let overlay = OVERLAY + self.storage;
        let view = self.view;
        let wipe = self.wipe;
        let native = wide && !self.preview;
        let (zoom, selected) = (ZOOM[self.zoom], self.selected);
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                *viewport.lock().unwrap() = Some(bounds);
                let image_bounds = placement(bounds, zoom, selected);
                window.with_content_mask(Some(ContentMask { bounds }), |window| match view {
                    View::Wipe => {
                        let split = bounds.origin.x + bounds.size.width * wipe;
                        let left = Bounds::from_corners(
                            bounds.origin,
                            point(split, bounds.bottom_right().y),
                        );
                        let right = Bounds::from_corners(
                            point(split, bounds.origin.y),
                            bounds.bottom_right(),
                        );
                        if let Some(image) = &images[SRGB] {
                            window.with_content_mask(Some(ContentMask { bounds: left }), |w| {
                                Flowers::paint_image(image, image_bounds, false, w)
                            });
                        }
                        if let Some(image) = &images[prophoto] {
                            window.with_content_mask(Some(ContentMask { bounds: right }), |w| {
                                Flowers::paint_image(image, image_bounds, native, w)
                            });
                        }
                        window.paint_quad(fill(
                            Bounds::new(
                                point(split - px(1.), bounds.origin.y),
                                size(px(2.), bounds.size.height),
                            ),
                            rgb(0xfacc15),
                        ));
                    }
                    View::Overlay => {
                        if let Some(image) = &images[overlay] {
                            Flowers::paint_image(image, image_bounds, false, window);
                        }
                    }
                    View::WebKit => {
                        if let Some(image) = &images[OOG] {
                            Flowers::paint_image(image, image_bounds, false, window);
                        }
                    }
                });
                // Selected-pixel marker.
                let (x, y) = selected;
                let center = point(
                    image_bounds.origin.x
                        + image_bounds.size.width * ((x as f32 + 0.5) / WIDTH as f32),
                    image_bounds.origin.y
                        + image_bounds.size.height * ((y as f32 + 0.5) / HEIGHT as f32),
                );
                if bounds.contains(&center) {
                    let r = px(6.);
                    window.paint_quad(outline(
                        Bounds::new(center - point(r, r), size(r * 2., r * 2.)),
                        rgb(0xfacc15),
                        BorderStyle::Solid,
                    ));
                }
            },
        )
        .size_full()
    }
    fn thumbnail(&self, slot: usize, wide: bool) -> impl IntoElement {
        let image = self.images[slot].clone();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                if let Some(image) = &image {
                    // Thumbnails minify ~6.7x and sample the associated mip chain.
                    Flowers::paint_image(image, bounds, wide, window);
                }
            },
        )
        .size_full()
    }
    fn patches(&self, wide: bool) -> impl IntoElement {
        let image = self.images[PATCHES_SLOT + variant(self.storage, self.mapping)].clone();
        let native = wide && !self.preview && !self.blur;
        let blur = self
            .blur
            .then(|| xgpui_media::pipeline::ImageBlur::new(px(2.)));
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let Some(image) = &image else { return };
                match &blur {
                    Some(Ok(blur)) => {
                        if let Err(error) = blur.paint(bounds, window, |w| {
                            Flowers::paint_image(image, bounds, false, w)
                        }) {
                            eprintln!("Flowers patch blur: {error:#}");
                        }
                    }
                    _ => Flowers::paint_image(image, bounds, native, window),
                }
            },
        )
        .size_full()
    }
    /// Compare the displayed native composition at the selected pixel with the
    /// CPU stored value converted to linear P3.
    fn check(&mut self, window: &mut Window) {
        let result = (|| -> anyhow::Result<String> {
            anyhow::ensure!(
                window.color_status().effective() == WindowColorMode::WideGamutSdr && !self.preview,
                "Select Wide gamut and native output for Metal FP16 readback"
            );
            anyhow::ensure!(self.view == View::Wipe, "Use the wipe view");
            anyhow::ensure!(
                self.zoom == 3,
                "Set zoom to 8× so one source pixel covers many device pixels"
            );
            anyhow::ensure!(
                self.images[PROPHOTO + variant(self.storage, self.mapping)]
                    .as_ref()
                    .is_some_and(|i| i.upload_status() == xgpui_media::UploadStatus::Ready),
                "Wait for the ProPhoto upload"
            );
            let view = self
                .viewport
                .lock()
                .unwrap()
                .ok_or_else(|| anyhow::anyhow!("Comparison has not painted"))?;
            let placed = self.placement(view);
            let (x, y) = self.selected;
            let center = point(
                placed.origin.x + placed.size.width * ((x as f32 + 0.5) / WIDTH as f32),
                placed.origin.y + placed.size.height * ((y as f32 + 0.5) / HEIGHT as f32),
            );
            anyhow::ensure!(
                center.x > view.origin.x + view.size.width * self.wipe + px(4.),
                "Move the wipe left of the selected pixel so it shows the ProPhoto side"
            );
            let expected = match &self.inspection {
                Some(Ok(inspection)) => inspection.composed,
                _ => anyhow::bail!("No valid selected pixel"),
            };
            let frame = window.render_to_linear_p3()?;
            let scale = window.scale_factor();
            let (px_x, px_y) = (
                (f32::from(center.x) * scale) as usize,
                (f32::from(center.y) * scale) as usize,
            );
            anyhow::ensure!(
                px_x < frame.size.width.0 as usize && px_y < frame.size.height.0 as usize,
                "Selected pixel is outside the window"
            );
            let actual = frame.pixels[px_y * frame.size.width.0 as usize + px_x].map(half);
            let delta = (0..3)
                .map(|i| (actual[i] - expected[i]).abs())
                .fold(0., f64::max);
            Ok(format!(
                "Metal FP16 at ({x}, {y}): {:.5?}; CPU stored→linear P3 {:.5?}; max difference {delta:.5} ({}). Composition values before OS display mapping, not monitor measurements.",
                &actual[..3],
                expected,
                if delta <= 2e-3 {
                    "within FP16 storage tolerance"
                } else {
                    "OUTSIDE 2e-3 tolerance"
                }
            ))
        })();
        self.report = result.unwrap_or_else(|error| format!("Native readback: {error:#}"));
    }
}
fn fmt3(v: [f64; 3]) -> String {
    format!("[{:.5}, {:.5}, {:.5}]", v[0], v[1], v[2])
}
impl Lab {
    fn flowers_button(
        &self,
        label: String,
        id: &'static str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Flowers, &mut Window) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x263448))
            .text_color(rgb(0xe2eaf3))
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |s, _, window, cx| {
                if let Some(flowers) = &mut s.flowers {
                    action(flowers, window);
                }
                cx.notify();
            }))
            .into_any_element()
    }
    pub(super) fn render_flowers(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let wide = window.color_status().effective() == WindowColorMode::WideGamutSdr;
        if let Some(flowers) = &mut self.flowers {
            flowers.poll(window);
        }
        let mut body = div()
            .id("flowers-scroll")
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .bg(rgb(0x202833))
            .text_color(rgb(0xe2eaf3))
            .flex()
            .flex_col()
            .gap_3()
            .when(self.standalone_navigation, |body| {
                body.child(self.button("← Chromaticity & metadata".into(), 29, cx))
            })
            .child(
                div()
                    .text_2xl()
                    .child("Flowers — WebKit ProPhoto RGB comparison"),
            )
            .child(self.window_mode_controls(window, cx));
        let Some(f) = &self.flowers else { return body };
        let storage = ["bounded linear Display P3", "bounded linear Rec.2020"][f.storage];
        let mapping = ["Clip", "ReduceChroma"][f.mapping];
        let output = if f.preview || !wide {
            "explicit sRGB preview"
        } else {
            "native wide (extended linear P3 composition)"
        };
        body = body
            .when(!wide, |body| body.child(div().p_3().bg(rgb(0x422006)).text_color(rgb(0xfde68a))
                .child("Standard SDR output is active: the ProPhoto photograph is shown as an sRGB preview of its mapped storage.")))
            .child(div().flex().flex_wrap().gap_2()
                .child(self.flowers_button(format!("Storage: {}", ["P3", "Rec.2020"][f.storage]), "flowers-storage", cx, |f, _| { f.storage ^= 1; f.reinspect(); }))
                .child(self.flowers_button(format!("Mapping: {mapping}"), "flowers-mapping", cx, |f, _| { f.mapping ^= 1; f.reinspect(); }))
                .child(self.flowers_button(format!("Output: {}", if f.preview { "sRGB preview" } else { "native" }), "flowers-output", cx, |f, _| f.preview = !f.preview))
                .child(self.flowers_button(format!("View: {}", match f.view { View::Wipe => "sRGB | ProPhoto wipe", View::Overlay => "our pre-mapping overlay", View::WebKit => "WebKit out-of-sRGB" }), "flowers-view", cx, |f, _| f.view = match f.view { View::Wipe => View::Overlay, View::Overlay => View::WebKit, View::WebKit => View::Wipe }))
                .child(self.flowers_button(format!("Zoom: {}×", ZOOM[f.zoom]), "flowers-zoom", cx, |f, _| f.zoom = (f.zoom + 1) % ZOOM.len()))
                .child(self.flowers_button(format!("Wipe: {:.0}%", f.wipe * 100.), "flowers-wipe", cx, |f, _| f.wipe = if f.wipe >= 1. { 0. } else { f.wipe + 0.25 }))
                .child(self.flowers_button(format!("Patch blur (sRGB preview): {}", if f.blur { "2 px" } else { "off" }), "flowers-blur", cx, |f, _| f.blur = !f.blur))
                .child(self.flowers_button("Check native readback (Metal FP16)".into(), "flowers-check", cx, |f, window| f.check(window))))
            .child(format!("Source: ProPhoto RGB / ROMM RGB, named xgpui interpretation of raw 8-bit JPEG channels • Storage: {storage} • Mapping: {mapping} • Output: {output} • Effective window mode {:?}", window.color_status().effective()))
            .child(f.status.clone());
        if let Some(a) = &f.analysis {
            let pct = |n: usize| 100. * n as f64 / a.pixels as f64;
            body = body.child(format!(
                "Before mapping: {:.2}% of {} pixels outside sRGB, {:.2}% outside Display P3, {:.2}% outside Rec.2020. Out-of-storage pixels are mapped ({mapping}); their ProPhoto values are not retained.",
                pct(a.outside[0]), a.pixels, pct(a.outside[1]), pct(a.outside[2])
            )).child(format!(
                "Embedded profile: Kodak “ProPhoto RGB” v2, pure gamma 461/256 and no linear toe. The named ROMM decode differs from it by at most {:.2e} (linear) at 8-bit codes 0–7 and {:.2e} above.",
                a.profile_delta.0, a.profile_delta.1
            ));
        }
        let mapped_slot = PROPHOTO + variant(f.storage, f.mapping);
        let labels = [
            "sRGB reference (WebKit)",
            "ProPhoto → storage",
            "Our overlay",
            "WebKit out-of-sRGB",
        ];
        body = body
            .child(div().flex().flex_wrap().gap_4().children([SRGB, mapped_slot, OVERLAY + f.storage, OOG].into_iter().zip(labels).map(|(slot, label)| {
                let aspect = if slot == OOG { f.oog_aspect } else { WIDTH as f32 / HEIGHT as f32 };
                div().flex().flex_col().gap_1()
                    .child(div().w(px(180.)).h(px(180. / aspect)).child(f.thumbnail(slot, wide && !f.preview && slot == mapped_slot)))
                    .child(div().text_sm().child(label))
            })))
            .child(div().text_sm().child("Click the photograph to select a pixel. Wipe: WebKit’s sRGB JPEG on the left, the ProPhoto JPEG through xgpui on the right. Overlay: cyan leaves sRGB but fits storage; magenta leaves storage and is mapped."))
            .child(
                div()
                    .id("flowers-view")
                    .w_full()
                    .max_w(px(900.))
                    .aspect_ratio(WIDTH as f32 / HEIGHT as f32)
                    .flex_shrink_0()
                    .cursor_crosshair()
                    .on_mouse_down(MouseButton::Left, cx.listener(|s, event: &MouseDownEvent, _, cx| {
                        if let Some(flowers) = &mut s.flowers
                            && flowers.select_at(event.position)
                        {
                            flowers.report = "Native readback: selection changed; check again.".into();
                            cx.notify();
                        }
                    }))
                    .child(f.main_view(wide)),
            );
        let (x, y) = f.selected;
        body = body.child(match &f.inspection {
            Some(Ok(i)) => {
                let flag = |v: [f64; 3]| if inside(v) { "inside" } else { "OUTSIDE" };
                div().flex().flex_col().gap_1().p_3().bg(rgb(0x1a2230)).text_sm()
                    .child(format!("Pixel ({x}, {y}) • ProPhoto codes {:?} • WebKit sRGB codes {:?}", i.codes, i.reference))
                    .child(format!("Linear ProPhoto (ROMM) {} • embedded-profile gamma {}", fmt3(i.linear), fmt3(i.profile)))
                    .child(format!("XYZ/D65 {}", fmt3(i.xyz)))
                    .child(format!("Before mapping: sRGB {} {} • P3 {} {} • Rec.2020 {} {}", fmt3(i.before[0]), flag(i.before[0]), fmt3(i.before[1]), flag(i.before[1]), fmt3(i.before[2]), flag(i.before[2])))
                    .child(format!("Stored ({storage}, {mapping}, associated) [{:.5}, {:.5}, {:.5}, {:.2}] • as linear P3 {}", i.stored[0], i.stored[1], i.stored[2], i.stored[3], fmt3(i.composed)))
                    .into_any_element()
            }
            Some(Err(error)) => div().child(format!("Pixel ({x}, {y}): {error:#}")).into_any_element(),
            None => div().child("Pixel inspection is available once decoding finishes.").into_any_element(),
        });
        body.child(div().text_sm().child(f.report.clone()))
            .child(div().text_lg().child("Synthetic ProPhoto companions"))
            .child(div().text_sm().child("Row 1: neutral ramp with near-black codes on both sides of the 16/512 knee; last patch 50% alpha. Row 2: ProPhoto primaries and secondaries, a color inside P3, a green outside P3 but inside Rec.2020, a deep purple, then red at 75/50/25/0% alpha over white. Same storage and mapping as the photograph. Blur applies to the sRGB preview only."))
            .child(div().w(px(360.)).h(px(360. * 2. / 13.)).bg(rgb(0xffffff)).child(f.patches(wide)))
            .child(f.error.clone().unwrap_or_default())
            .child(div().id("webkit-flowers-credit").cursor_pointer().text_color(rgb(0x82caff))
                .child("Flowers images courtesy of WebKit’s Wide Gamut demo · original examples and source ↗")
                .on_click(|_, _, cx| cx.open_url(SOURCE)))
            .child("Original WebKit JPEGs retained unchanged. The lab decodes raw channels and applies xgpui’s ProPhoto math; storage is bounded P3 or Rec.2020, so this is not full ProPhoto output. Visible differences depend on window mode and display; readbacks are composition values, not monitor measurements.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::prelude::v1::test;
    use std::time::{Duration, Instant};
    use xgpui_color::{BradfordAdaptation, WhitePoint};

    fn source() -> Decoded {
        decode(PROPHOTO_JPEG).unwrap()
    }

    #[test]
    fn webkit_flowers_assets_are_pinned_and_decode_raw() {
        for (bytes, expected, dims) in [
            (
                PROPHOTO_JPEG,
                "37fcbf81abe112b92ad8b26be3ec0b7346048f31888183547ad48ba07f36f88e",
                (1200, 795),
            ),
            (
                SRGB_JPEG,
                "6f5cdc2807550508eae156ff6e449d37b21010aa53553572e38600fc303864b6",
                (1200, 795),
            ),
            (
                OOG_JPEG,
                "eb8a6c7601d0bb85c04d41441563acd1596f1e7f2568e6433fc70187d5ed3017",
                (1644, 1088),
            ),
        ] {
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected);
            let decoded = decode(bytes).unwrap();
            assert_eq!((decoded.width, decoded.height), dims);
            assert_eq!(decoded.codes.len(), (dims.0 * dims.1) as usize);
        }
    }

    #[test]
    fn embedded_profile_is_kodak_prophoto_pure_gamma_with_romm_colorants() {
        let icc = embedded_profile(PROPHOTO_JPEG)
            .unwrap()
            .expect("embedded profile");
        let tag = |sig: &[u8; 4]| {
            let count = u32::from_be_bytes(icc[128..132].try_into().unwrap()) as usize;
            (0..count)
                .map(|i| &icc[132 + 12 * i..144 + 12 * i])
                .find(|entry| &entry[..4] == sig)
                .map(|entry| {
                    let offset = u32::from_be_bytes(entry[4..8].try_into().unwrap()) as usize;
                    let size = u32::from_be_bytes(entry[8..12].try_into().unwrap()) as usize;
                    &icc[offset..offset + size]
                })
                .expect("tag")
        };
        // Three single-entry curves: u8Fixed8 gamma 0x01CD = 461/256, no toe.
        for sig in [b"rTRC", b"gTRC", b"bTRC"] {
            let curve = tag(sig);
            assert_eq!(&curve[..4], b"curv");
            assert_eq!(u32::from_be_bytes(curve[8..12].try_into().unwrap()), 1);
            assert_eq!(
                u16::from_be_bytes(curve[12..14].try_into().unwrap()),
                0x01cd
            );
        }
        assert_eq!(PROFILE_GAMMA, f64::from(0x01cd_u16) / 256.);
        // D50 colorants agree with ROMM to the profile's s15Fixed16 precision.
        let native = BradfordAdaptation::new(WhitePoint::D65, WhitePoint::D50)
            .apply_colorants(RgbSpace::ProPhotoRgb.linear_to_xyz_matrix())
            .unwrap();
        for (column, sig) in [b"rXYZ", b"gXYZ", b"bXYZ"].into_iter().enumerate() {
            let xyz = tag(sig);
            for row in 0..3 {
                let v = i32::from_be_bytes(xyz[8 + 4 * row..12 + 4 * row].try_into().unwrap())
                    as f64
                    / 65536.;
                assert!((v - native[row][column]).abs() < 2e-4, "{sig:?}[{row}] {v}");
            }
        }
        // xgpui's strict ICC adapter rejects the profile's Apple `mmod` tag by
        // design; the lab therefore uses explicit named metadata.
        assert!(xgpui_color::icc::IccRgbProfile::parse(&icc).is_err());
    }

    #[test]
    fn analysis_measures_pre_mapping_gamut_and_profile_toe() {
        let analysis = analyze(&source());
        let [srgb, p3, rec2020] = analysis.outside;
        eprintln!(
            "Flowers pre-mapping: {} pixels; outside sRGB {srgb}, P3 {p3}, Rec.2020 {rec2020}; profile delta {:?}",
            analysis.pixels, analysis.profile_delta
        );
        assert_eq!(analysis.pixels, 1200 * 795);
        assert!(srgb > 0, "Flowers should leave sRGB");
        assert!(srgb >= p3 && p3 >= rec2020, "nested containment counts");
        // The toe only matters near black; above it, only the 461/256 vs 9/5
        // exponent rounding remains.
        assert!(analysis.profile_delta.0 > analysis.profile_delta.1 * 0.1);
        assert!(analysis.profile_delta.1 < 2e-3);
    }

    #[test]
    fn overlay_marks_exactly_the_pre_mapping_classification() {
        let source = source();
        let analysis = analyze(&source);
        for (storage, outside) in [
            (ImagePrimaries::DisplayP3, analysis.outside[1]),
            (ImagePrimaries::Rec2020, analysis.outside[2]),
        ] {
            let pixels = overlay(&source, storage);
            assert_eq!(
                pixels.iter().filter(|p| **p == [1., 0., 1., 1.]).count(),
                outside
            );
            let marked = pixels.iter().filter(|p| p[0] != p[1]).count();
            assert_eq!(marked, analysis.outside[0]);
        }
    }

    #[test]
    fn inspection_follows_the_named_pipeline_and_mapping_boundary() {
        let source = source();
        let reference = decode(SRGB_JPEG).unwrap();
        let (x, y) = DEFAULT_SELECTION;
        let i = inspect(
            &source,
            &reference,
            x,
            y,
            ImagePrimaries::DisplayP3,
            GamutMapping::Clip,
        )
        .unwrap();
        assert!(
            !inside(i.before[1]) && inside(i.before[2]),
            "default pixel should need P3 mapping only"
        );
        assert!(
            x as f32 > WIDTH as f32 * 0.5,
            "default pixel should be on the ProPhoto side of the wipe"
        );
        for storage in STORAGE {
            for mapping in MAPPING {
                let i = inspect(&source, &reference, x, y, storage, mapping).unwrap();
                assert!(i.stored[..3].iter().all(|v| (0. ..=1.).contains(v)));
                assert_eq!(i.stored[3], 1.);
                let before = if storage == ImagePrimaries::Rec2020 {
                    i.before[2]
                } else {
                    i.before[1]
                };
                if inside(before) {
                    for (s, b) in i.stored.iter().zip(before) {
                        assert!((s - b).abs() < 1e-12);
                    }
                }
            }
        }
    }

    #[test]
    fn all_flowers_variants_finish_preparation() -> anyhow::Result<()> {
        let mut prepared = prepare(&AtomicBool::new(false))?;
        assert_eq!(prepared.jobs.len(), SLOTS);
        let deadline = Instant::now() + Duration::from_secs(60);
        for job in &mut prepared.jobs {
            loop {
                if job.try_take().is_some() {
                    break;
                }
                if let xgpui_media::LoadStatus::Failed(error) = job.status() {
                    anyhow::bail!(error);
                }
                anyhow::ensure!(Instant::now() < deadline, "Flowers preparation timed out");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        assert!(prepare(&AtomicBool::new(true)).is_err());
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn prophoto_native_composition_matches_cpu_storage_on_metal() -> anyhow::Result<()> {
        // Saturated ProPhoto colors through each storage/mapping, composed by the
        // native Metal wide-SDR path, against the CPU stored value as linear P3.
        let colors = [
            [0.95, 0.25, 0.2, 1.],
            [0.2, 0.8, 0.2, 1.],
            [0.18, 0.18, 0.18, 0.5],
        ];
        let mut cases = Vec::new();
        for storage in STORAGE {
            for mapping in MAPPING {
                for color in colors {
                    let canonical = ImageColor {
                        alpha: AlphaMode::Premultiplied,
                        primaries: storage,
                        transfer: TransferFunction::Linear,
                    };
                    let stored = ImageColor::PROPHOTO_RGB
                        .decode(color.map(f64::from), storage)?
                        .output(canonical, mapping)?;
                    let space = if storage == ImagePrimaries::Rec2020 {
                        RgbSpace::Rec2020
                    } else {
                        RgbSpace::DisplayP3
                    };
                    let p3 = LinearRgb::new(space, [stored[0], stored[1], stored[2]])?
                        .convert(RgbSpace::DisplayP3)?
                        .components();
                    let job = MediaLoader::default().load_pixel_buffer(
                        1,
                        1,
                        vec![color],
                        ImageColor::PROPHOTO_RGB.into(),
                        storage,
                        mapping,
                    )?;
                    cases.push((job, p3, color[3]));
                }
            }
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut images = Vec::new();
        for (job, _, _) in &mut cases {
            loop {
                if let Some(image) = job.try_take() {
                    images.push(image);
                    break;
                }
                if let xgpui_media::LoadStatus::Failed(error) = job.status() {
                    anyhow::bail!(error);
                }
                anyhow::ensure!(Instant::now() < deadline, "preparation timed out");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        let mut scene = gpui::Scene::default();
        for (i, image) in images.iter().enumerate() {
            let bounds = Bounds::new(
                point(ScaledPixels(i as f32 * 16.), ScaledPixels(0.)),
                size(ScaledPixels(16.), ScaledPixels(16.)),
            );
            scene.insert_primitive(gpui::CustomDraw {
                order: gpui::DrawOrder::default(),
                bounds,
                content_mask: gpui::ContentMask { bounds },
                corner_radii: Default::default(),
                opacity: 1.,
                coverage: None,
                content: Arc::new(image.wide(None)?),
            });
        }
        scene.finish();
        let width = images.len() as i32 * 16;
        let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new()?;
        renderer.set_color_mode(WindowColorMode::WideGamutSdr)?;
        loop {
            renderer.render_scene_to_linear_p3(
                &scene,
                size(gpui::DevicePixels(width), gpui::DevicePixels(16)),
            )?;
            if images
                .iter()
                .all(|i| i.upload_status() == xgpui_media::UploadStatus::Ready)
            {
                break;
            }
            anyhow::ensure!(Instant::now() < deadline, "upload timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        let frame = renderer.render_scene_to_linear_p3(
            &scene,
            size(gpui::DevicePixels(width), gpui::DevicePixels(16)),
        )?;
        let mut worst = 0_f64;
        for (i, (_, p3, _)) in cases.iter().enumerate() {
            // Opaque black frame: composed RGB equals the associated stored value,
            // so fractional alpha is checked through its RGB contribution.
            let actual = frame.pixels[8 * width as usize + i * 16 + 8].map(half);
            for c in 0..3 {
                let delta = (actual[c] - p3[c]).abs();
                worst = worst.max(delta);
                anyhow::ensure!(delta <= 2e-3, "case {i} channel {c}: {actual:?} vs {p3:?}");
            }
        }
        eprintln!(
            "ProPhoto Metal composition: {} cases, worst channel delta {worst:e}",
            cases.len()
        );
        Ok(())
    }
}
