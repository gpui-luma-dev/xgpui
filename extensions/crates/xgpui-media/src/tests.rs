use super::*;
use anyhow::{Result, bail};
use gpui::{
    ContentMask, CustomDraw, DevicePixels, DrawOrder, PlatformHeadlessRenderer, ScaledPixels,
    Scene, Size, point, size,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "xgpui-media-{}-{}.png",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        image::RgbaImage::from_fn(64, 64, |x, y| {
            let c = if (x + y) % 2 == 0 { 0 } else { 255 };
            image::Rgba([c, c, c, 255])
        })
        .save(&path)
        .unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn finish(job: &mut ImageJob) -> Result<MediaImage> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(image) = job.try_take() {
            return Ok(image);
        }
        match job.status() {
            LoadStatus::Failed(error) => bail!(error),
            LoadStatus::Cancelled => bail!("cancelled"),
            _ => {}
        }
        if Instant::now() > deadline {
            bail!("image load timed out");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn loader_payload_admission_follows_last_snapshot() -> Result<()> {
    let fixture = Fixture::new();
    let loader = MediaLoader::new(MediaLimits {
        max_jobs: 1,
        max_resident_bytes: 32768,
        max_image_bytes: 32768,
        max_encoded_bytes: 32768,
    })?;
    let mut job = loader.load(&fixture.0)?;
    let image = finish(&mut job)?;
    drop(job);
    assert_eq!(
        (image.width(), image.height(), image.mip_count()),
        (64, 64, 7)
    );
    assert_eq!(image.payload_bytes(), 21844);
    let clone = image.clone();
    assert!(loader.load(&fixture.0).is_err());
    drop(image);
    assert!(loader.load(&fixture.0).is_err());
    drop(clone);
    let mut next = loader.load(&fixture.0)?;
    finish(&mut next)?;
    Ok(())
}
#[test]
fn loader_failure_and_cancellation_release_reservations() -> Result<()> {
    let fixture = Fixture::new();
    let loader = MediaLoader::new(MediaLimits {
        max_jobs: 1,
        max_resident_bytes: 16,
        max_image_bytes: 16,
        max_encoded_bytes: 32768,
    })?;
    let mut oversized = loader.load(&fixture.0)?;
    assert!(finish(&mut oversized).is_err());
    assert!(matches!(oversized.status(), LoadStatus::Failed(_)));
    drop(oversized);
    let cancelled = loader.load(&fixture.0)?;
    cancelled.cancel();
    assert_eq!(cancelled.status(), LoadStatus::Cancelled);
    drop(cancelled);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut missing = loop {
        match loader.load(fixture.0.with_extension("missing")) {
            Ok(job) => break job,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
            Err(error) => return Err(error),
        }
    };
    assert!(finish(&mut missing).is_err());
    Ok(())
}
fn image_scene(image: &MediaImage, side: f32, clipped: bool) -> Scene {
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(4.), ScaledPixels(4.)),
        size(ScaledPixels(side), ScaledPixels(side)),
    );
    let mask = if clipped {
        Bounds::new(
            point(ScaledPixels(8.), ScaledPixels(4.)),
            size(ScaledPixels(side - 4.), ScaledPixels(side)),
        )
    } else {
        bounds
    };
    scene.insert_primitive(CustomDraw {
        order: DrawOrder::default(),
        bounds,
        content_mask: ContentMask { bounds: mask },
        corner_radii: Corners::all(ScaledPixels(if clipped { 3. } else { 0. })),
        opacity: 1.,
        content: Arc::new(image.clone()),
        coverage: None,
    });
    scene.finish();
    scene
}
fn render_ready(
    renderer: &mut dyn PlatformHeadlessRenderer,
    image: &MediaImage,
    scene: &Scene,
    extent: Size<DevicePixels>,
) -> Result<image::RgbaImage> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pixels = renderer.render_scene_to_image(scene, extent)?;
        match image.upload_status() {
            UploadStatus::Ready => return Ok(pixels),
            UploadStatus::Failed(error) => bail!(error),
            _ => {}
        }
        if Instant::now() > deadline {
            bail!("image upload timed out");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn check_rendering(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<MediaImage> {
    let fixture = Fixture::new();
    let mut job = MediaLoader::default().load(&fixture.0)?;
    let image = finish(&mut job)?;
    let alpha_fixture = Fixture::new();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 0, 128])).save(&alpha_fixture.0)?;
    let mut alpha_job = MediaLoader::default().load(&alpha_fixture.0)?;
    let alpha_image = finish(&mut alpha_job)?;
    let extent = size(DevicePixels(24), DevicePixels(24));
    let mut alpha_scene = image_scene(&alpha_image, 8., false);
    alpha_scene.custom_draws[0].content =
        Arc::new(alpha_image.filtered(ImageFilter::new().saturation(0.)));
    let pixels = render_ready(renderer, &alpha_image, &alpha_scene, extent)?;
    let p = pixels.get_pixel(6, 6).0;
    for channel in &p[..3] {
        assert!(
            channel.abs_diff(27) <= 2,
            "filter preserves translucent alpha: {p:?}"
        );
    }
    let first = renderer.render_scene_to_image(&image_scene(&image, 8., false), extent)?;
    assert_eq!(
        first.get_pixel(6, 6).0,
        [0, 0, 0, 255],
        "first paint uses transparent placeholder"
    );
    for side in [8., 16., 8.] {
        let pixels = render_ready(renderer, &image, &image_scene(&image, side, false), extent)?;
        let p = pixels.get_pixel(6, 6).0;
        assert!(p[0].abs_diff(188) <= 3, "mipmapped checkerboard: {p:?}");
        assert_eq!(p[3], 255);
        assert_eq!(pixels.get_pixel(1, 1).0, [0, 0, 0, 255]);
    }
    assert_eq!(
        image.upload_count(),
        1,
        "resizing/replay must reuse source texture"
    );
    // Different placements share source uploads but retain independent uniforms.
    let mut filtered = image_scene(&image, 8., false);
    filtered.custom_draws[0].content =
        Arc::new(image.filtered(ImageFilter::new().tint([1., 0., 0., 1.])));
    filtered.custom_draws[0].opacity = 0.5;
    let pixels = renderer.render_scene_to_image(&filtered, extent)?;
    let p = pixels.get_pixel(6, 6).0;
    assert!(
        p[0].abs_diff(128) <= 2 && p[1] <= 2 && p[2] <= 2,
        "filtered RGB with coverage/opacity: {p:?}"
    );
    let pixels = renderer.render_scene_to_image(&image_scene(&image, 8., false), extent)?;
    assert!(
        pixels.get_pixel(6, 6).0[1].abs_diff(188) <= 3,
        "filter must not mutate cached source"
    );
    assert_eq!(image.upload_count(), 1, "filter updates do not reupload");
    let pixels = renderer.render_scene_to_image(&image_scene(&image, 8., true), extent)?;
    assert_eq!(
        pixels.get_pixel(6, 8).0,
        [0, 0, 0, 255],
        "native content mask"
    );
    assert_eq!(pixels.get_pixel(9, 8).0[3], 255);
    assert!(
        pixels.get_pixel(11, 4).0[0] < 180,
        "rounded corner coverage"
    );
    assert_eq!(image.upload_count(), 1);
    let mut translucent = image_scene(&image, 8., false);
    translucent.custom_draws[0].opacity = 0.5;
    let pixels = renderer.render_scene_to_image(&translucent, extent)?;
    assert!(
        pixels.get_pixel(6, 6).0[0].abs_diff(94) <= 3,
        "inherited opacity applied once"
    );
    let mut ordered = image_scene(&image, 8., false);
    let bounds = Bounds::new(
        point(ScaledPixels(8.), ScaledPixels(8.)),
        size(ScaledPixels(2.), ScaledPixels(2.)),
    );
    ordered.insert_primitive(gpui::Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: gpui::rgb(0xff0000).into(),
        ..Default::default()
    });
    ordered.finish();
    let pixels = renderer.render_scene_to_image(&ordered, extent)?;
    assert_eq!(
        pixels.get_pixel(8, 8).0,
        [255, 0, 0, 255],
        "native quad retains scene order"
    );
    Ok(image)
}
#[cfg(target_os = "macos")]
#[test]
fn metal_mips_clipping_and_resize_reuse() -> Result<()> {
    let image = check_rendering(&mut gpui_apple::metal_renderer::MetalHeadlessRenderer::new())?;
    render_ready(
        &mut gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
        &image,
        &image_scene(&image, 8., false),
        size(DevicePixels(24), DevicePixels(24)),
    )?;
    assert_eq!(image.upload_count(), 2, "new queue rebuilds texture");
    Ok(())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_mips_clipping_and_resize_reuse() -> Result<()> {
    let image = check_rendering(&mut gpui_wgpu::WgpuHeadlessRenderer::new()?)?;
    gpui_wgpu::WgpuHeadlessRenderer::new()?.render_scene_to_image(
        &image_scene(&image, 8., false),
        size(DevicePixels(24), DevicePixels(24)),
    )?;
    assert_eq!(
        image.upload_count(),
        1,
        "shared headless queue reuses texture"
    );
    Ok(())
}

#[cfg(feature = "wgpu")]
#[test]
fn legacy_wgpu_outputs_still_require_display_extent() -> Result<()> {
    #[derive(Debug)]
    struct WrongExtent;
    impl gpui::CustomContent for WrongExtent {
        fn prepare(
            &self,
            backend: &dyn std::any::Any,
            _: Size<DevicePixels>,
        ) -> Result<Box<dyn std::any::Any>> {
            let context = backend
                .downcast_ref::<gpui_wgpu::custom_draw::WgpuCustomContext>()
                .unwrap();
            Ok(Box::new(
                context.create_output(size(DevicePixels(1), DevicePixels(1)))?,
            ))
        }
    }
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    let mut scene = Scene::default();
    scene.insert_primitive(CustomDraw {
        order: DrawOrder::default(),
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::default(),
        opacity: 1.,
        content: Arc::new(WrongExtent),
        coverage: None,
    });
    scene.finish();
    let result = gpui_wgpu::WgpuHeadlessRenderer::new()?
        .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(16)));
    assert!(result.is_err());
    Ok(())
}

#[cfg(feature = "wgpu")]
#[test]
fn upload_admission_waits_for_host_handoff_and_releases_budget() -> Result<()> {
    let fixture = Fixture::new();
    // 64..1 rows, padded to 256 bytes each: exactly one source fits.
    let loader = MediaLoader::with_upload_limits(
        MediaLimits::default(),
        UploadLimits {
            max_jobs: 1,
            max_staging_bytes: 32512,
        },
    )?;
    let first = finish(&mut loader.load(&fixture.0)?)?;
    let second = finish(&mut loader.load(&fixture.0)?)?;
    let extent = size(DevicePixels(24), DevicePixels(24));
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    renderer.render_scene_to_image(&image_scene(&first, 8., false), extent)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while first.upload_status() != UploadStatus::Staged {
        if let UploadStatus::Failed(error) = first.upload_status() {
            bail!(error);
        }
        if Instant::now() > deadline {
            bail!("staging timed out");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(loader.upload_stats().staging_bytes, 32512);
    renderer.render_scene_to_image(&image_scene(&second, 8., false), extent)?;
    assert_eq!(second.upload_status(), UploadStatus::Queued);
    assert_eq!(second.upload_count(), 0);
    render_ready(
        &mut renderer,
        &first,
        &image_scene(&first, 8., false),
        extent,
    )?;
    assert_eq!(loader.upload_stats().staging_bytes, 0);
    render_ready(
        &mut renderer,
        &second,
        &image_scene(&second, 8., false),
        extent,
    )?;
    assert_eq!(loader.upload_stats(), UploadStats::default());
    assert_eq!(second.upload_count(), 1);
    Ok(())
}

#[cfg(feature = "wgpu")]
#[test]
fn upload_budget_failure_is_observable_and_retry_resets_status() -> Result<()> {
    let fixture = Fixture::new();
    let loader = MediaLoader::with_upload_limits(
        MediaLimits::default(),
        UploadLimits {
            max_jobs: 1,
            max_staging_bytes: 1,
        },
    )?;
    let image = finish(&mut loader.load(&fixture.0)?)?;
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    renderer.render_scene_to_image(
        &image_scene(&image, 8., false),
        size(DevicePixels(24), DevicePixels(24)),
    )?;
    assert!(matches!(image.upload_status(), UploadStatus::Failed(_)));
    assert_eq!(image.upload_count(), 0);
    assert_eq!(loader.upload_stats(), UploadStats::default());
    image.retry_upload();
    assert_eq!(image.upload_status(), UploadStatus::Idle);
    let frame_limited = MediaLoader::with_upload_policy(
        MediaLimits::default(),
        UploadLimits::default(),
        FrameUploadLimits {
            max_uploads: 1,
            max_bytes: 1,
        },
    )?;
    let too_large = finish(&mut frame_limited.load(&fixture.0)?)?;
    renderer.render_scene_to_image(
        &image_scene(&too_large, 8., false),
        size(DevicePixels(24), DevicePixels(24)),
    )?;
    assert!(matches!(too_large.upload_status(), UploadStatus::Failed(_)));
    assert_eq!(
        too_large.upload_count(),
        0,
        "oversized source must fail before allocation"
    );
    assert_eq!(frame_limited.staging_pool_stats().allocations, 0);
    Ok(())
}

fn stage_only(renderer: &mut dyn PlatformHeadlessRenderer, image: &MediaImage) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match image.upload_status() {
            UploadStatus::Staged => return Ok(()),
            UploadStatus::Idle | UploadStatus::Queued => {
                renderer.render_scene_to_image(
                    &image_scene(image, 8., false),
                    size(DevicePixels(24), DevicePixels(24)),
                )?;
            }
            UploadStatus::Failed(error) => bail!(error),
            _ => {}
        }
        if Instant::now() > deadline {
            bail!("worker staging timed out");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn verify_pool_reuse(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let fixture = Fixture::new();
    let loader = MediaLoader::default();
    let first = finish(&mut loader.load(&fixture.0)?)?;
    let extent = size(DevicePixels(24), DevicePixels(24));
    let pixels = render_ready(renderer, &first, &image_scene(&first, 8., false), extent)?;
    assert!(pixels.get_pixel(6, 6).0[0].abs_diff(188) <= 3);
    let deadline = Instant::now() + Duration::from_secs(10);
    while loader.staging_pool_stats().cached_bytes == 0 {
        if Instant::now() > deadline {
            bail!("completion did not return staging buffer");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(loader.staging_pool_stats().allocations, 1);
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 0, 255])).save(&fixture.0)?;
    let second = finish(&mut loader.load(&fixture.0)?)?;
    let pixels = render_ready(renderer, &second, &image_scene(&second, 8., false), extent)?;
    assert_eq!(
        pixels.get_pixel(6, 6).0,
        [255, 0, 0, 255],
        "reused storage must contain the new pixels"
    );
    let stats = loader.staging_pool_stats();
    assert_eq!(stats.allocations, 1);
    assert_eq!(stats.reuses, 1);
    assert!(stats.allocated_bytes <= 128 * 1024 * 1024);
    Ok(())
}
fn verify_frame_limits(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    // Exercise count and byte limits independently. Repeated placements of one
    // image must not spend another admission from the same scene frame.
    for limits in [
        FrameUploadLimits {
            max_uploads: 1,
            max_bytes: 64 * 1024 * 1024,
        },
        FrameUploadLimits {
            max_uploads: 10,
            max_bytes: 32512,
        },
    ] {
        let fixture = Fixture::new();
        let loader = MediaLoader::with_upload_policy(
            MediaLimits::default(),
            UploadLimits::default(),
            limits,
        )?;
        let mut images = Vec::new();
        for _ in 0..3 {
            let image = finish(&mut loader.load(&fixture.0)?)?;
            stage_only(renderer, &image)?;
            images.push(image);
        }
        let mut scene = Scene::default();
        for index in [0, 0, 1, 2] {
            let mut draw = image_scene(&images[index], 8., false)
                .custom_draws
                .remove(0);
            draw.bounds.origin.x = ScaledPixels(4. + index as f32 * 12.);
            draw.content_mask.bounds = draw.bounds;
            scene.insert_primitive(draw);
        }
        scene.finish();
        for expected in 1..=3 {
            renderer.render_scene_to_image(&scene, size(DevicePixels(48), DevicePixels(24)))?;
            assert_eq!(
                images
                    .iter()
                    .filter(|image| image.upload_status() == UploadStatus::Ready)
                    .count(),
                expected,
                "exactly one new source handoff per scene frame"
            );
        }
        assert!(images.iter().all(|image| image.upload_count() == 1));
    }
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_completion_pool_refills_and_frame_limits() -> Result<()> {
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    verify_pool_reuse(&mut renderer)?;
    verify_frame_limits(&mut renderer)
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_completion_pool_refills_and_frame_limits() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    verify_pool_reuse(&mut renderer)?;
    verify_frame_limits(&mut renderer)
}

fn check_linear_adjustments(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    use crate::pipeline::{
        ColorMatrix, GamutMapping, ImageAdjustments, ImageColor, ImagePrimaries, Vignette,
    };
    let fixture = Fixture::new();
    // Rectangular source deliberately painted in a square: vignette must use
    // source pixels, not widget aspect ratio. Constant pixels isolate the math.
    image::RgbaImage::from_pixel(64, 32, image::Rgba([128, 64, 32, 128])).save(&fixture.0)?;
    let mut job = MediaLoader::default().load(&fixture.0)?;
    let image = finish(&mut job)?;
    let input = ImageColor::SRGB.decode(
        [128. / 255., 64. / 255., 32. / 255., 128. / 255.],
        ImagePrimaries::Srgb,
    )?;
    let extent = size(DevicePixels(28), DevicePixels(28));
    for ev in [0., 1., -1., 2., 0.] {
        let matrix = ColorMatrix::identity(ImagePrimaries::Srgb)
            .exposure(ev)?
            .contrast(1.2, 0.18)?
            .saturation(0.6)?;
        let vignette = Vignette::new(0.7, 0.25, 0.7, 1.)?;
        let mut scene = image_scene(&image, 16., false);
        scene.custom_draws[0].content =
            Arc::new(image.adjusted(ImageAdjustments::new(matrix, Some(vignette))?));
        scene.custom_draws[0].opacity = 0.65;
        let pixels = render_ready(renderer, &image, &scene, extent)?;
        for (x, y) in [(12, 12), (5, 12), (18, 12), (12, 5), (12, 18)] {
            let uv = [(x as f64 + 0.5 - 4.) / 16., (y as f64 + 0.5 - 4.) / 16.];
            let expected = vignette
                .apply(matrix.apply(input)?, uv, [64, 32])?
                .output(ImageColor::SRGB, GamutMapping::Clip)?;
            let actual = pixels.get_pixel(x, y).0;
            for c in 0..3 {
                let byte = (expected[c] * expected[3] * 0.65 * 255.).round() as u8;
                assert!(
                    actual[c].abs_diff(byte) <= 2,
                    "linear/vignette CPU reference ev={ev} at {x},{y}: {actual:?}, expected channel {c}={byte}"
                );
            }
        }
        assert_eq!(
            pixels.get_pixel(2, 2).0,
            [0, 0, 0, 255],
            "native content mask"
        );
        assert_eq!(
            image.upload_count(),
            1,
            "adjustments must reuse source uploads"
        );
    }
    // Switching back to the old encoded path must not retain new uniforms.
    let mut legacy = image_scene(&image, 16., false);
    legacy.custom_draws[0].content = Arc::new(image.filtered(ImageFilter::new().brightness(2.)));
    let pixels = renderer.render_scene_to_image(&legacy, extent)?;
    let p = pixels.get_pixel(12, 12).0;
    assert!(
        p[0].abs_diff(128) <= 2 && p[1].abs_diff(64) <= 2,
        "legacy encoded path: {p:?}"
    );
    assert_eq!(image.upload_count(), 1);
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_linear_adjustments_match_cpu_and_reuse_uploads() -> Result<()> {
    check_linear_adjustments(&mut gpui_apple::metal_renderer::MetalHeadlessRenderer::new())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_linear_adjustments_match_cpu_and_reuse_uploads() -> Result<()> {
    check_linear_adjustments(&mut gpui_wgpu::WgpuHeadlessRenderer::new()?)
}

fn check_float_preview(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    use pipeline::*;
    let loader = MediaLoader::default();
    for rgba in [
        [1., 0.12, 0.02, 1.],
        [0.1, 0.7, 0.3, 0.5],
        [0.4, 0.1, 0.7, 0.],
    ] {
        let mut job = loader.load_rgba16f(64, 32, vec![rgba; 64 * 32], ImageColor::LINEAR_P3)?;
        let image = finish(&mut job)?;
        assert_eq!(image.source_color(), ImageColor::LINEAR_P3.into());
        assert_eq!(
            image.format(),
            gpui::ImageTextureFormat::LinearP3Rgba16Float
        );
        for working in [ImagePrimaries::Srgb, ImagePrimaries::DisplayP3] {
            let input = ImageColor::LINEAR_P3.decode(rgba.map(f64::from), working)?;
            for ev in [0., 1., -1.] {
                let matrix = ColorMatrix::identity(working)
                    .exposure(ev)?
                    .saturation(0.8)?;
                let vignette = Vignette::new(0.6, 0.25, 0.7, 1.)?;
                let mut scene = image_scene(&image, 16., false);
                scene.custom_draws[0].content =
                    Arc::new(image.adjusted(ImageAdjustments::new(matrix, Some(vignette))?));
                scene.custom_draws[0].opacity = 0.65;
                let pixels = render_ready(
                    renderer,
                    &image,
                    &scene,
                    size(DevicePixels(28), DevicePixels(28)),
                )?;
                for (x, y) in [(12, 12), (5, 12), (12, 5)] {
                    let uv = [(x as f64 + 0.5 - 4.) / 16., (y as f64 + 0.5 - 4.) / 16.];
                    let expected = vignette
                        .apply(matrix.apply(input)?, uv, [64, 32])?
                        .output(ImageColor::SRGB, GamutMapping::Clip)?;
                    let actual = pixels.get_pixel(x, y).0;
                    for c in 0..3 {
                        let byte = (expected[c] * expected[3] * 0.65 * 255.).round() as u8;
                        assert!(
                            actual[c].abs_diff(byte) <= 2,
                            "RGBA16F {working:?} {rgba:?} {ev}: {actual:?} expected {byte} channel {c}"
                        );
                    }
                }
            }
        }
        // No adjustment must still decode the float source and map P3 to sRGB.
        let scene = image_scene(&image, 16., false);
        let pixels =
            renderer.render_scene_to_image(&scene, size(DevicePixels(28), DevicePixels(28)))?;
        let expected = ImageColor::LINEAR_P3
            .decode(rgba.map(f64::from), ImagePrimaries::DisplayP3)?
            .output(ImageColor::SRGB, GamutMapping::Clip)?;
        for c in 0..3 {
            assert!(
                pixels.get_pixel(12, 12).0[c]
                    .abs_diff((expected[c] * expected[3] * 255.).round() as u8)
                    <= 2
            );
        }
        assert_eq!(image.upload_count(), 1);
        assert!(
            image
                .filtered(ImageFilter::new().brightness(2.))
                .validate()
                .is_err()
        );
    }
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_rgba16f_matches_cpu_and_reuses_source() -> Result<()> {
    check_float_preview(&mut gpui_apple::metal_renderer::MetalHeadlessRenderer::new())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_rgba16f_matches_cpu_and_reuses_source() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    check_float_preview(&mut renderer)?;
    let mut job = MediaLoader::default().load_rgba16f(
        1,
        1,
        vec![[0.5; 4]],
        pipeline::ImageColor::LINEAR_P3,
    )?;
    let image = finish(&mut job)?;
    let mut scene = image_scene(&image, 16., false);
    scene.custom_draws[0].content = Arc::new(image.wide(None)?);
    assert!(
        renderer
            .render_scene_to_image(&scene, size(DevicePixels(28), DevicePixels(28)))
            .is_err()
    );
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_rgba16f_wide_requires_opt_in_and_maps_after_linear_blending() -> Result<()> {
    use pipeline::*;
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    let rgba = [0.8, 0.12, 0.02, 0.5];
    let mut job =
        MediaLoader::default().load_rgba16f(64, 32, vec![rgba; 64 * 32], ImageColor::LINEAR_P3)?;
    let image = finish(&mut job)?;
    let extent = size(DevicePixels(28), DevicePixels(28));
    let mut scene = image_scene(&image, 16., false);
    scene.custom_draws[0].content = Arc::new(image.wide(None)?);
    assert!(renderer.render_scene_to_image(&scene, extent).is_err());
    renderer.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
    for working in [ImagePrimaries::Srgb, ImagePrimaries::DisplayP3] {
        let matrix = ColorMatrix::identity(working).exposure(0.5)?;
        scene.custom_draws[0].content =
            Arc::new(image.wide(Some(ImageAdjustments::new(matrix, None)?))?);
        scene.custom_draws[0].opacity = 0.65;
        let pixels = render_ready(&mut renderer, &image, &scene, extent)?;
        let input = ImageColor::LINEAR_P3.decode(rgba.map(f64::from), working)?;
        let adjusted = matrix.apply(input)?.rgba();
        let space = if working == ImagePrimaries::Srgb {
            xgpui_color::RgbSpace::Srgb
        } else {
            xgpui_color::RgbSpace::DisplayP3
        };
        let rgb = xgpui_color::LinearRgb::new(space, [adjusted[0], adjusted[1], adjusted[2]])?
            .convert(xgpui_color::RgbSpace::DisplayP3)?
            .components();
        let mapped = [rgb[0], rgb[1], rgb[2], adjusted[3]];
        // Wide compositor blends in associated linear P3 before its sRGB capture.
        let expected = WorkingPixel::new(
            ImagePrimaries::DisplayP3,
            [
                mapped[0] * mapped[3] * 0.65,
                mapped[1] * mapped[3] * 0.65,
                mapped[2] * mapped[3] * 0.65,
                1.,
            ],
        )?
        .output(ImageColor::SRGB, GamutMapping::Clip)?;
        for (c, value) in expected.iter().take(3).enumerate() {
            let actual = pixels.get_pixel(12, 12).0[c];
            let byte = (value * 255.).round() as u8;
            assert!(
                actual.abs_diff(byte) <= 2,
                "wide {working:?} {actual} != {byte}"
            );
        }
    }
    assert_eq!(image.upload_count(), 1);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn native_surface_adjustments_follow_conversion_preserve_identity_and_import_once() -> Result<()> {
    use gpui_apple::custom_draw::core_video::{Nv12Matrix, test_frame, test_nv12_frame};
    use pipeline::*;
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    let bounds = gpui::Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(320.), ScaledPixels(180.)),
    );
    for format in 0..5 {
        let native = match format {
            0 => test_frame(320, 180, 0)?,
            n => test_nv12_frame(
                320,
                180,
                0,
                if n <= 2 {
                    Nv12Matrix::Bt601
                } else {
                    Nv12Matrix::Bt709
                },
                n % 2 == 0,
            )?,
        };
        let video = VideoFrame::core_video(native.clone(), Duration::from_millis(42));
        let mut scene = Scene::default();
        scene.insert_primitive(CustomDraw {
            bounds,
            content_mask: ContentMask { bounds },
            content: video.content(),
            opacity: 1.,
            order: Default::default(),
            corner_radii: Default::default(),
            coverage: None,
        });
        scene.finish();
        let extent = size(DevicePixels(320), DevicePixels(180));
        let baseline = renderer.render_scene_to_image(&scene, extent)?;
        for working in [ImagePrimaries::Srgb, ImagePrimaries::DisplayP3] {
            for ev in [-1., 0., 1.] {
                let matrix = ColorMatrix::identity(working)
                    .exposure(ev)?
                    .saturation(0.6)?;
                let vignette = Vignette::new(0.6, 0.3, 0.7, 1.)?;
                let adjusted = video.adjusted(ImageAdjustments::new(matrix, Some(vignette))?);
                assert_eq!(adjusted.timestamp(), video.timestamp());
                assert_eq!(adjusted.size(), video.size());
                scene.custom_draws[0].content = adjusted.content();
                scene.custom_draws[0].opacity = 0.65;
                let pixels = renderer.render_scene_to_image(&scene, extent)?;
                for (x, y) in [(24, 24), (104, 24), (184, 24), (264, 24), (160, 100)] {
                    let input = baseline.get_pixel(x, y).0.map(|c| f64::from(c) / 255.);
                    let expected = vignette
                        .apply(
                            matrix.apply(ImageColor::SRGB.decode(input, working)?)?,
                            [(x as f64 + 0.5) / 320., (y as f64 + 0.5) / 180.],
                            [320, 180],
                        )?
                        .output(ImageColor::SRGB, GamutMapping::Clip)?;
                    let actual = pixels.get_pixel(x, y).0;
                    for c in 0..3 {
                        assert!(
                            actual[c].abs_diff((expected[c] * 0.65 * 255.).round() as u8) <= 3,
                            "format {format} {working:?} ev {ev} at {x},{y}: {actual:?} {expected:?}"
                        );
                    }
                }
            }
        }
        assert_eq!(
            native.import_count(),
            1,
            "adjustment edits must reuse native imports"
        );
    }
    Ok(())
}

fn check_adobe_rgb_preview(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    use pipeline::*;
    let loader = MediaLoader::default();
    for mapping in [GamutMapping::Clip, GamutMapping::ReduceChroma] {
        for rgb in [[0., 1., 0.], [0., 1., 1.], [0.5; 3]] {
            let rgba = [rgb[0], rgb[1], rgb[2], 0.5];
            let mut job = loader.load_rgba16f_with_mapping(
                64,
                32,
                vec![rgba; 64 * 32],
                ImageColor::ADOBE_RGB_1998,
                mapping,
            )?;
            let image = finish(&mut job)?;
            assert_eq!(image.source_color(), ImageColor::ADOBE_RGB_1998.into());
            assert_eq!(
                image.format(),
                gpui::ImageTextureFormat::LinearP3Rgba16Float
            );
            let scene = image_scene(&image, 16., false);
            let pixels = render_ready(
                renderer,
                &image,
                &scene,
                size(DevicePixels(28), DevicePixels(28)),
            )?;
            let stored = ImageColor::ADOBE_RGB_1998
                .decode(rgba.map(f64::from), ImagePrimaries::DisplayP3)?
                .output(ImageColor::LINEAR_P3, mapping)?;
            let expected = ImageColor::LINEAR_P3
                .decode(stored, ImagePrimaries::DisplayP3)?
                .output(ImageColor::SRGB, GamutMapping::Clip)?;
            for (c, value) in expected.iter().take(3).enumerate() {
                let byte = (value * expected[3] * 255.).round() as u8;
                let actual = pixels.get_pixel(12, 12).0[c];
                assert!(
                    actual.abs_diff(byte) <= 2,
                    "{mapping:?} {rgb:?}: {actual} != {byte}"
                );
            }
            assert_eq!(image.upload_count(), 1);
        }
    }
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_adobe_rgb_input_with_explicit_mapping() -> Result<()> {
    check_adobe_rgb_preview(&mut gpui_apple::metal_renderer::MetalHeadlessRenderer::new())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_adobe_rgb_input_with_explicit_mapping() -> Result<()> {
    check_adobe_rgb_preview(&mut gpui_wgpu::WgpuHeadlessRenderer::new()?)
}

fn check_rec2020_preview(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    use pipeline::*;
    let loader = MediaLoader::default();
    for color in [
        ImageColor::ADOBE_RGB_1998,
        ImageColor::LINEAR_P3,
        ImageColor::LINEAR_REC2020,
    ] {
        for rgba in [
            [0., 1., 0., 1.],
            [0., 1., 1., 0.5],
            [0.18, 0.3, 0.7, 0.25],
            [0.5, 0.2, 0.1, 0.],
        ] {
            let mut job =
                loader.load_rec2020_rgba16f(3, 1, vec![rgba; 3], color, GamutMapping::Clip)?;
            let image = finish(&mut job)?;
            assert_eq!(image.source_color(), color.into());
            assert_eq!(
                image.format(),
                gpui::ImageTextureFormat::LinearRec2020Rgba16Float
            );
            assert_eq!(
                gpui::CustomContent::image_color_mode(&image.wide(None)?),
                gpui::ImageColorMode::LinearRec2020Wide
            );
            let scene = image_scene(&image, 16., false);
            let pixels = render_ready(
                renderer,
                &image,
                &scene,
                size(DevicePixels(28), DevicePixels(28)),
            )?;
            let expected = color
                .decode(rgba.map(f64::from), ImagePrimaries::Rec2020)?
                .output(ImageColor::SRGB, GamutMapping::Clip)?;
            for c in 0..3 {
                let byte = (expected[c] * expected[3] * 255.).round() as u8;
                assert!(
                    pixels.get_pixel(12, 12).0[c].abs_diff(byte) <= 2,
                    "{color:?} {rgba:?}: {:?}, expected {expected:?}",
                    pixels.get_pixel(12, 12)
                );
            }
            for working in [ImagePrimaries::Srgb, ImagePrimaries::DisplayP3] {
                let matrix = ColorMatrix::identity(working)
                    .exposure(-1.)?
                    .saturation(0.8)?;
                let mut adjusted = image_scene(&image, 16., false);
                adjusted.custom_draws[0].content =
                    Arc::new(image.adjusted(ImageAdjustments::new(matrix, None)?));
                let pixels = renderer
                    .render_scene_to_image(&adjusted, size(DevicePixels(28), DevicePixels(28)))?;
                let expected = matrix
                    .apply(color.decode(rgba.map(f64::from), working)?)?
                    .output(ImageColor::SRGB, GamutMapping::Clip)?;
                for c in 0..3 {
                    let byte = (expected[c] * expected[3] * 255.).round() as u8;
                    assert!(
                        pixels.get_pixel(12, 12).0[c].abs_diff(byte) <= 2,
                        "{working:?} {color:?} {rgba:?}"
                    );
                }
            }
            assert_eq!(image.upload_count(), 1);
        }
    }
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_rec2020_storage_and_preview() -> Result<()> {
    check_rec2020_preview(&mut gpui_apple::metal_renderer::MetalHeadlessRenderer::new())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_rec2020_storage_and_preview() -> Result<()> {
    let mut r = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    check_rec2020_preview(&mut r)?;
    let mut job = MediaLoader::default().load_rec2020_rgba16f(
        1,
        1,
        vec![[0., 1., 0., 1.]],
        pipeline::ImageColor::LINEAR_REC2020,
        pipeline::GamutMapping::Clip,
    )?;
    let image = finish(&mut job)?;
    let mut s = image_scene(&image, 16., false);
    s.custom_draws[0].content = Arc::new(image.wide(None)?);
    assert!(
        r.render_scene_to_image(&s, size(DevicePixels(28), DevicePixels(28)))
            .is_err()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn adobe_rec2020_wide_blends_before_mapping_and_reuses_uploads() -> Result<()> {
    use pipeline::*;
    let mut r = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    let mut job = MediaLoader::default().load_rec2020_rgba16f(
        3,
        1,
        vec![[0., 1., 0., 0.5]; 3],
        ImageColor::ADOBE_RGB_1998,
        GamutMapping::Clip,
    )?;
    let image = finish(&mut job)?;
    let mut scene = image_scene(&image, 16., false);
    scene.custom_draws[0].content = Arc::new(image.wide(None)?);
    let extent = size(DevicePixels(28), DevicePixels(28));
    assert!(r.render_scene_to_image(&scene, extent).is_err());
    r.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
    // White arrives through the ordinary sRGB UI path before the wide image.
    scene.quads.push(gpui::Quad {
        order: 0,
        bounds: scene.custom_draws[0].bounds,
        content_mask: scene.custom_draws[0].content_mask,
        background: gpui::white().into(),
        ..Default::default()
    });
    scene.quads[0].order = 0;
    scene.custom_draws[0].order = 1;
    scene.finish();
    let pixels = render_ready(&mut r, &image, &scene, extent)?;
    let input = ImageColor::ADOBE_RGB_1998
        .decode([0., 1., 0., 0.5], ImagePrimaries::Srgb)?
        .rgba();
    let expected = WorkingPixel::new(
        ImagePrimaries::Srgb,
        [
            0.5 + input[0] * 0.5,
            0.5 + input[1] * 0.5,
            0.5 + input[2] * 0.5,
            1.,
        ],
    )?
    .output(ImageColor::SRGB, GamutMapping::Clip)?;
    for c in 0..3 {
        assert!(
            pixels.get_pixel(12, 12).0[c].abs_diff((expected[c] * 255.).round() as u8) <= 2,
            "{:?} != {expected:?}",
            pixels.get_pixel(12, 12)
        );
    }
    assert_eq!(image.upload_count(), 1);
    let too_large = ImageAdjustments::new(
        ColorMatrix::identity(ImagePrimaries::DisplayP3).exposure(3.)?,
        None,
    )?;
    assert!(image.wide(Some(too_large)).is_err());
    Ok(())
}

#[test]
fn resolved_pixel_metadata_survives_background_preparation() -> Result<()> {
    use pipeline::*;
    let definition = xgpui_color::RgbDefinition::new_d65(
        xgpui_color::RgbSpace::AdobeRgb1998.linear_to_xyz_matrix(),
        std::array::from_fn(|_| xgpui_color::RgbCurve::Gamma(563. / 256.)),
    )?;
    for source in [
        PixelColor::Custom {
            rgb: definition.into(),
            alpha: AlphaMode::Straight,
        },
        PixelColor::ExtendedLinear {
            primaries: ImagePrimaries::DisplayP3,
            alpha: AlphaMode::Straight,
        },
    ] {
        let loader = MediaLoader::default();
        let mut job = loader.load_pixel_buffer(
            3,
            1,
            vec![[0., 1., 0., 0.5]; 3],
            source.clone(),
            ImagePrimaries::Rec2020,
            GamutMapping::Clip,
        )?;
        let image = finish(&mut job)?;
        assert_eq!(image.source_color(), source);
        assert_eq!(
            image.format(),
            gpui::ImageTextureFormat::LinearRec2020Rgba16Float
        );
        assert!(
            loader
                .load_pixel_buffer(
                    1,
                    1,
                    vec![],
                    source.clone(),
                    ImagePrimaries::Rec2020,
                    GamutMapping::Clip
                )
                .is_err()
        );
        assert!(
            loader
                .load_pixel_buffer(
                    1,
                    1,
                    vec![[0.; 4]],
                    source.clone(),
                    ImagePrimaries::Srgb,
                    GamutMapping::Clip
                )
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn custom_sample_metadata_is_admitted_and_released_with_image() -> Result<()> {
    use pipeline::*;
    let table = xgpui_color::SampledCurve::new(vec![0, 32768, 65535])?;
    let definition = xgpui_color::RgbDefinition::new_d65(
        xgpui_color::RgbSpace::DisplayP3.linear_to_xyz_matrix(),
        std::array::from_fn(|_| xgpui_color::RgbCurve::Sampled(table.clone())),
    )?;
    let source = PixelColor::Custom {
        rgb: definition.into(),
        alpha: AlphaMode::Straight,
    };
    let bytes = source.retained_metadata_bytes();
    let loader = MediaLoader::new(MediaLimits {
        max_jobs: 1,
        max_image_bytes: 8,
        max_resident_bytes: 8 + bytes,
        max_encoded_bytes: 4096,
    })?;
    let load = || {
        loader.load_pixel_buffer(
            1,
            1,
            vec![[0.25, 0.5, 1., 0.5]],
            source.clone(),
            ImagePrimaries::DisplayP3,
            GamutMapping::Clip,
        )
    };
    let image = finish(&mut load()?)?;
    assert_eq!(image.source_color(), source);
    assert!(load().is_err());
    drop(image);
    let next = finish(&mut load()?)?;
    assert_eq!(next.source_color(), source);
    let tight = MediaLoader::new(MediaLimits {
        max_jobs: 1,
        max_image_bytes: 8,
        max_resident_bytes: 8,
        max_encoded_bytes: 4096,
    })?;
    assert!(
        tight
            .load_pixel_buffer(
                1,
                1,
                vec![[0.; 4]],
                source,
                ImagePrimaries::DisplayP3,
                GamutMapping::Clip
            )
            .is_err()
    );
    Ok(())
}
