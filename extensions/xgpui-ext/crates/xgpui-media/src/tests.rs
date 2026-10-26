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
            max_bytes: 21844,
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
