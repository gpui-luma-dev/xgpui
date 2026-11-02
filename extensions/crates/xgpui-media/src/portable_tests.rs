use super::*;
use gpui::content::*;

fn ordinary_scene(image: &MediaImage, side: f32) -> Scene {
    let mut draw = image_scene(image, side, false).custom_draws.remove(0);
    draw.content = Arc::new(image.clone());
    let mut scene = Scene::default();
    scene.insert_primitive(draw);
    scene.finish();
    scene
}
fn portable_ready(
    renderer: &mut dyn PlatformHeadlessRenderer,
    scene: &Scene,
) -> Result<image::RgbaImage> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match renderer.prepare_content_snapshot(scene) {
            Ok(()) => {
                return renderer
                    .render_scene_to_image(scene, size(DevicePixels(24), DevicePixels(24)));
            }
            Err(error)
                if error
                    .downcast_ref::<ContentError>()
                    .is_some_and(|e| e.kind == ContentErrorKind::Pending) => {}
            Err(error) => return Err(error),
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "portable image snapshot timed out"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn packed_mip_views_and_portable_pixels_share_one_allocation() -> Result<()> {
    let data = crate::mipmapped_rgba8(4, 2, vec![255; 32])?;
    let pixels = data.pixels();
    assert_eq!(data.levels()[0].as_ptr(), pixels.bytes().as_ptr());
    assert_eq!(data.levels()[1].as_ptr(), pixels.bytes()[32..].as_ptr());
    assert_eq!(pixels.descriptor().mip_levels(), 3);
    assert_eq!(pixels.bytes().len(), data.byte_len());
    let clone = data.clone();
    assert!(Arc::ptr_eq(pixels.bytes(), clone.pixels().bytes()));
    Ok(())
}
#[test]
fn graph_retains_loader_reservation_after_source_and_request_are_dropped() -> Result<()> {
    let fixture = Fixture::new();
    let loader = MediaLoader::new(MediaLimits {
        max_jobs: 1,
        max_resident_bytes: 32768,
        max_image_bytes: 32768,
        max_encoded_bytes: 32768,
    })?;
    let mut job = loader.load(&fixture.0)?;
    let image = finish(&mut job)?;
    let graph = ContentGraph::describe(
        &[ContentRoot {
            content: image.content(),
            request: ContentRequest::new(
                size(gpui::px(8.), gpui::px(8.)),
                ImageExtent::new(8, 8)?,
                1.,
            )?,
        }],
        ContentCapabilities::default(),
        GraphLimits::default(),
    )?;
    drop(job);
    drop(image);
    anyhow::ensure!(
        loader.load(&fixture.0).is_err(),
        "a retained graph must still charge the source payload"
    );
    drop(graph);
    let mut next = loader.load(&fixture.0)?;
    finish(&mut next)?;
    Ok(())
}
#[test]
fn ordinary_bridge_is_portable_source_sized_and_explicit_about_preview() -> Result<()> {
    use gpui::CustomContent as _;
    let fixture = Fixture::new();
    let mut job = MediaLoader::default().load(&fixture.0)?;
    let image = finish(&mut job)?;
    let request = gpui::PreparationRequest::new(
        size(gpui::px(8.), gpui::px(8.)),
        size(DevicePixels(8), DevicePixels(8)),
        1.,
    )?;
    let draw = image.portable_content(&request)?.unwrap();
    assert_eq!(
        draw.draw.presentation.color_output,
        ContentColorOutput::SrgbPreview
    );
    assert_eq!(draw.draw.content.id(), image.content().id());
    assert!(
        image.prepare(&(), request.raster_size()).is_err(),
        "ordinary images cannot dispatch through native producer preparation"
    );
    let graph = ContentGraph::describe(
        &[ContentRoot {
            content: draw.draw.content,
            request: draw.request,
        }],
        ContentCapabilities::default(),
        GraphLimits::default(),
    )?;
    let root = graph.roots()[0];
    let ResourceDescriptor::Image(descriptor) =
        &graph.nodes()[root.node()].outputs()[root.output()]
    else {
        panic!("image root")
    };
    assert_eq!(descriptor.extent(), ImageExtent::new(64, 64)?);
    assert_eq!(descriptor.mip_levels(), 7);
    assert_eq!(graph.payload_bytes(), image.payload_bytes() as u64);
    Ok(())
}
fn check_ordinary(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let fixture = Fixture::new();
    let loader = MediaLoader::default();
    let mut job = loader.load(&fixture.0)?;
    let image = finish(&mut job)?;
    for side in [8., 16., 8.] {
        let pixels = portable_ready(renderer, &ordinary_scene(&image, side))?;
        let pixel = pixels.get_pixel(6, 6).0;
        anyhow::ensure!(
            pixel[0].abs_diff(188) <= 3 && pixel[3] == 255,
            "portable mipmapped image: {pixel:?}"
        );
    }
    assert_eq!(
        image.upload_count(),
        0,
        "ordinary images must not start effect staging workers"
    );
    assert_eq!(loader.upload_stats().staging_bytes, 0);
    Ok(())
}
fn check_wide_preview(
    renderer: &mut dyn PlatformHeadlessRenderer,
    rec2020: bool,
    expected_red: u8,
) -> Result<()> {
    let loader = MediaLoader::default();
    let mut job = if rec2020 {
        loader.load_rec2020_rgba16f(
            8,
            4,
            vec![[1., 0., 0., 0.5]; 32],
            crate::pipeline::ImageColor::LINEAR_REC2020,
            crate::pipeline::GamutMapping::Clip,
        )?
    } else {
        loader.load_rgba16f(
            8,
            4,
            vec![[1., 0., 0., 0.5]; 32],
            crate::pipeline::ImageColor::LINEAR_P3,
        )?
    };
    let image = finish(&mut job)?;
    let pixels = portable_ready(renderer, &ordinary_scene(&image, 8.))?;
    let pixel = pixels.get_pixel(6, 6).0;
    anyhow::ensure!(
        pixel[0].abs_diff(expected_red) <= 2 && pixel[1] <= 2 && pixel[2] <= 2 && pixel[3] == 255,
        "portable FP16 preview: {pixel:?}"
    );
    let mut group = ContentGroup::new(ContentGroupId::new());
    group.push(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(
                point(gpui::px(0.), gpui::px(0.)),
                size(gpui::px(8.), gpui::px(8.)),
            ),
            corner_radii: Corners::default(),
        },
        image.draw().mapping(ImageMapping::new(ImageFit::Contain)),
    );
    let mut draw = ordinary_scene(&image, 8.).custom_draws.remove(0);
    draw.content = group.into_scene_content();
    let mut grouped = Scene::default();
    grouped.insert_primitive(draw);
    grouped.finish();
    let grouped_pixels = portable_ready(renderer, &grouped)?;
    assert_eq!(
        grouped_pixels.get_pixel(6, 4).0,
        [0, 0, 0, 255],
        "group mapping retains a transparent letterbox"
    );
    assert!(
        grouped_pixels.get_pixel(6, 6).0[0].abs_diff(expected_red) <= 2,
        "group must preserve explicit preview output"
    );
    assert_eq!(image.upload_count(), 0);
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_ordinary_images_use_portable_mips_without_producer_staging() -> Result<()> {
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    check_ordinary(&mut renderer)?;
    assert_eq!(
        renderer.content_presentation_stats().requests,
        1,
        "resize and replay must reuse the upload"
    );
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    Ok(())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_ordinary_images_use_portable_mips_without_producer_staging() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    check_ordinary(&mut renderer)?;
    assert_eq!(renderer.content_presentation_stats().requests, 1);
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_p3_and_rec2020_sources_have_explicit_srgb_previews() -> Result<()> {
    for rec2020 in [false, true] {
        check_wide_preview(
            &mut gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
            rec2020,
            128,
        )?;
    }
    Ok(())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_p3_and_rec2020_sources_have_explicit_srgb_previews() -> Result<()> {
    for rec2020 in [false, true] {
        check_wide_preview(&mut gpui_wgpu::WgpuHeadlessRenderer::new()?, rec2020, 128)?;
    }
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_srgb_preview_stays_clipped_in_a_wide_window() -> Result<()> {
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    renderer.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
    check_wide_preview(&mut renderer, false, 188)?;
    Ok(())
}

#[test]
fn provider_payload_is_checked_before_portable_asset_publication() -> Result<()> {
    let loader = MediaLoader::new(MediaLimits {
        max_jobs: 1,
        max_image_bytes: 16,
        max_resident_bytes: 16,
        max_encoded_bytes: 1024,
    })?;
    let mut job = loader.load_guarded(crate::pipeline::ImageColor::SRGB, None, |_, _| {
        gpui::ImageTextureData::page(4, 4, vec![255; 64], gpui::ImageTextureFormat::Rgba8Srgb)
    })?;
    assert!(finish(&mut job).is_err());
    assert!(matches!(job.status(), LoadStatus::Failed(_)));
    drop(job);
    let mut small = loader.load_rgba16f(
        1,
        1,
        vec![[1., 0., 0., 1.]],
        crate::pipeline::ImageColor::SRGB,
    )?;
    assert_eq!(finish(&mut small)?.payload_bytes(), 8);
    Ok(())
}
