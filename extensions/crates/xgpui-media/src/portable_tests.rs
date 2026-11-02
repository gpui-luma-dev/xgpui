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

#[derive(Debug)]
struct CountedSource {
    content: Content,
    descriptions: Arc<AtomicUsize>,
}
impl ContentSource for CountedSource {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> std::result::Result<ImageRef, ContentError> {
        self.descriptions.fetch_add(1, Ordering::SeqCst);
        cx.source(&self.content, request)
    }
}
// Deliberately exposes only the portable request, without legacy image hooks.
#[derive(Debug)]
struct PortableOnly(ContentDraw);
impl gpui::CustomContent for PortableOnly {
    fn portable_content(
        &self,
        request: &gpui::PreparationRequest,
    ) -> std::result::Result<Option<ContentDrawRequest>, ContentError> {
        self.0.portable_content(request)
    }
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        anyhow::bail!("portable request must bypass native preparation")
    }
}
fn draw_scene(image: &MediaImage, draw: ContentDraw, opacity: f32) -> Scene {
    let mut record = ordinary_scene(image, 16.).custom_draws.remove(0);
    record.content = Arc::new(PortableOnly(draw));
    record.opacity = opacity;
    let mut scene = Scene::default();
    scene.insert_primitive(record);
    scene.finish();
    scene
}
fn exercise_effect_changes(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    use crate::pipeline::{
        ColorMatrix, GamutMapping, ImageAdjustments, ImageColor, ImagePrimaries, Vignette,
    };
    let fixture = Fixture::new();
    image::RgbaImage::from_pixel(16, 8, image::Rgba([128, 64, 32, 128])).save(&fixture.0)?;
    let image = finish(&mut MediaLoader::default().load(&fixture.0)?)?;
    let descriptions = Arc::new(AtomicUsize::new(0));
    let counted = Content::new(CountedSource {
        content: image.content(),
        descriptions: descriptions.clone(),
    });
    let filters = [
        ImageFilter::new(),
        ImageFilter::new().brightness(1.8),
        ImageFilter::new().brightness(8.),
        ImageFilter::new().contrast(1.5).saturation(0.6),
        ImageFilter::new().hue_rotate(90.),
        ImageFilter::new().tint([1., 0.2, 0., 0.4]),
        ImageFilter::new(),
    ];
    let input = [128. / 255., 64. / 255., 32. / 255.];
    for filter in filters {
        let mut draw = image.filtered(filter).draw()?;
        draw.content = counted.clone();
        let pixels = portable_ready(renderer, &draw_scene(&image, draw, 0.65))?;
        let actual = pixels.get_pixel(12, 12).0;
        for (channel, row) in filter.rows().iter().enumerate() {
            let value = (row[0] as f64 * input[0]
                + row[1] as f64 * input[1]
                + row[2] as f64 * input[2]
                + row[3] as f64)
                .clamp(0., 1.);
            let expected = (value * 128. * 0.65).round() as u8;
            assert!(
                actual[channel].abs_diff(expected) <= 2,
                "encoded transform {filter:?}: {actual:?}, channel {channel} expected {expected}"
            );
        }
    }
    for working in [ImagePrimaries::Srgb, ImagePrimaries::DisplayP3] {
        for ev in [-1., 0., 1.] {
            let matrix = ColorMatrix::identity(working)
                .exposure(ev)?
                .contrast(1.2, 0.18)?
                .saturation(0.6)?;
            let vignette = Vignette::new(0.6, 0.2, 0.8, 1.)?;
            let mut draw = image
                .adjusted(ImageAdjustments::new(matrix, Some(vignette))?)
                .draw();
            draw.content = counted.clone();
            let pixels = portable_ready(renderer, &draw_scene(&image, draw, 0.65))?;
            let source =
                ImageColor::SRGB.decode([input[0], input[1], input[2], 128. / 255.], working)?;
            for (x, y) in [(12, 12), (4, 12), (18, 12)] {
                let uv = [(x as f64 + 0.5 - 4.) / 16., (y as f64 + 0.5 - 4.) / 16.];
                let expected = vignette
                    .apply(matrix.apply(source)?, uv, [16, 8])?
                    .output(ImageColor::SRGB, GamutMapping::Clip)?;
                let actual = pixels.get_pixel(x, y).0;
                for channel in 0..3 {
                    let byte = (expected[channel] * expected[3] * 0.65 * 255.).round() as u8;
                    assert!(
                        actual[channel].abs_diff(byte) <= 2,
                        "{working:?} ev={ev} {x},{y}: {actual:?}, expected {byte}"
                    );
                }
            }
        }
    }
    // Cropping must not recenter the source-space vignette on the destination.
    let matrix = ColorMatrix::identity(ImagePrimaries::DisplayP3).exposure(0.5)?;
    let vignette = Vignette::new(0.8, 0.1, 0.6, 1.)?;
    let mut draw = image
        .adjusted(ImageAdjustments::new(matrix, Some(vignette))?)
        .draw()
        .mapping(ImageMapping::default().crop(Some(ImageCrop::new([4., 2.], [8., 4.])?)));
    draw.content = counted;
    let pixels = portable_ready(renderer, &draw_scene(&image, draw, 1.))?;
    let input = ImageColor::SRGB.decode(
        [input[0], input[1], input[2], 128. / 255.],
        ImagePrimaries::DisplayP3,
    )?;
    for (x, y) in [(4, 12), (12, 12), (18, 12)] {
        let uv = [
            0.25 + (x as f64 + 0.5 - 4.) / 32.,
            0.25 + (y as f64 + 0.5 - 4.) / 32.,
        ];
        let expected = vignette
            .apply(matrix.apply(input)?, uv, [16, 8])?
            .output(ImageColor::SRGB, GamutMapping::Clip)?;
        let actual = pixels.get_pixel(x, y).0;
        for c in 0..3 {
            assert!(
                actual[c].abs_diff((expected[c] * expected[3] * 255.).round() as u8) <= 2,
                "cropped source-space vignette: {actual:?}"
            );
        }
    }
    assert_eq!(
        descriptions.load(Ordering::SeqCst),
        1,
        "parameter-only changes must not rebuild or rehash the source graph"
    );
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_filters_and_linear_adjustments_reuse_one_upload_and_one_description() -> Result<()> {
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    exercise_effect_changes(&mut renderer)?;
    assert_eq!(renderer.content_presentation_stats().requests, 1);
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    Ok(())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_filters_and_linear_adjustments_reuse_one_upload_and_one_description() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    exercise_effect_changes(&mut renderer)?;
    assert_eq!(renderer.content_presentation_stats().requests, 1);
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    Ok(())
}
fn exercise_atomic_effects(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let fixture = Fixture::new();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255])).save(&fixture.0)?;
    let image = finish(&mut MediaLoader::default().load(&fixture.0)?)?;
    let id = ContentGroupId::new();
    let group_scene = |content: ContentDraw| {
        let mut group = ContentGroup::new(id);
        group.push(
            CustomGeometry::RoundedRect {
                bounds: Bounds::new(
                    point(gpui::px(0.), gpui::px(0.)),
                    size(gpui::px(16.), gpui::px(16.)),
                ),
                corner_radii: Corners::default(),
            },
            content,
        );
        let mut draw = ordinary_scene(&image, 16.).custom_draws.remove(0);
        draw.content = group.into_scene_content();
        let mut scene = Scene::default();
        scene.insert_primitive(draw);
        scene.finish();
        scene
    };
    let old = group_scene(
        image
            .filtered(ImageFilter::new().tint([0., 1., 0., 1.]))
            .draw()?,
    );
    assert_eq!(
        portable_ready(renderer, &old)?.get_pixel(12, 12).0,
        [0, 255, 0, 255]
    );
    let (asset, done) = ImageAsset::pending();
    let next = group_scene(
        ContentDraw::new(Content::image(asset))
            .color_output(ContentColorOutput::SrgbPreview)
            .color_transform(ImageColorTransform::encoded_srgb(
                ImageFilter::new().tint([0., 0., 1., 1.]).rows(),
            )?),
    );
    assert_eq!(
        renderer
            .render_scene_to_image(&next, size(DevicePixels(24), DevicePixels(24)))?
            .get_pixel(12, 12)
            .0,
        [0, 255, 0, 255],
        "pending group must retain its old color transform"
    );
    assert!(renderer.prepare_content_snapshot(&next).is_err());
    let descriptor = ImageDescriptor::new(
        ImageExtent::new(1, 1)?,
        PixelFormat::Rgba8,
        ImageInterpretation {
            color: ColorEncoding::Srgb,
            alpha: AlphaMode::Straight,
        },
        1,
    )?;
    done.resolve(Ok(ImagePixels::new(
        descriptor,
        Arc::from([255, 0, 0, 255]),
        4,
    )?));
    assert_eq!(
        portable_ready(renderer, &next)?.get_pixel(12, 12).0,
        [0, 0, 255, 255]
    );
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_atomic_groups_commit_color_transforms_with_new_pixels() -> Result<()> {
    exercise_atomic_effects(&mut gpui_apple::metal_renderer::MetalHeadlessRenderer::new())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_atomic_groups_commit_color_transforms_with_new_pixels() -> Result<()> {
    exercise_atomic_effects(&mut gpui_wgpu::WgpuHeadlessRenderer::new()?)
}

#[cfg(target_os = "macos")]
#[test]
fn metal_rgba8_sources_support_explicit_wide_adjustments_and_encoded_filters() -> Result<()> {
    use crate::pipeline::{ColorMatrix, ImageAdjustments, ImagePrimaries};
    let fixture = Fixture::new();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 128])).save(&fixture.0)?;
    let image = finish(&mut MediaLoader::default().load(&fixture.0)?)?;
    let adjusted = image
        .wide(Some(ImageAdjustments::new(
            ColorMatrix::identity(ImagePrimaries::DisplayP3).exposure(2.)?,
            None,
        )?))?
        .draw();
    let scene = draw_scene(&image, adjusted, 1.);
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    let error = renderer.prepare_content_snapshot(&scene).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ContentError>().unwrap().kind,
        ContentErrorKind::UnsupportedCapability
    );
    renderer.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
    portable_ready(&mut renderer, &scene)?;
    let linear =
        renderer.render_scene_to_linear_p3(&scene, size(DevicePixels(24), DevicePixels(24)))?;
    let red = half::f16::from_bits(linear.pixels[12 * 24 + 12][0]).to_f32();
    assert!(
        red > 1.6 && red < 1.7,
        "wide adjustment was clipped or treated as encoded: {red}"
    );
    let encoded = image
        .filtered(ImageFilter::new().tint([0., 0., 1., 1.]))
        .draw()?
        .color_output(ContentColorOutput::WideSdr);
    let pixel = portable_ready(&mut renderer, &draw_scene(&image, encoded, 1.))?
        .get_pixel(12, 12)
        .0;
    assert!(
        pixel[0] <= 1 && pixel[1] <= 1 && pixel[2].abs_diff(188) <= 2,
        "encoded wide transform: {pixel:?}"
    );
    assert_eq!(renderer.content_presentation_stats().requests, 1);
    Ok(())
}
#[cfg(feature = "wgpu")]
#[test]
fn wgpu_rejects_explicit_wide_intent_for_rgba8_without_starting_an_upload() -> Result<()> {
    let image = finish(&mut MediaLoader::default().load(&Fixture::new().0)?)?;
    let scene = draw_scene(&image, image.wide(None)?.draw(), 1.);
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    let error = renderer.prepare_content_snapshot(&scene).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ContentError>().unwrap().kind,
        ContentErrorKind::UnsupportedCapability
    );
    assert_eq!(renderer.content_presentation_stats().requests, 0);
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn metal_color_mode_change_rejects_late_rgba8_wide_group_publication() -> Result<()> {
    use gpui_apple::content_executor::{MetalContentLimits, MetalContentTestGate};
    struct Release(Arc<MetalContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    let mut renderer = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
    renderer.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
    let (tx, rx) = std::sync::mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = tx.send(());
    }));
    let id = ContentGroupId::new();
    let make = |rgba: [u8; 4]| -> Result<Scene> {
        let descriptor = ImageDescriptor::new(
            ImageExtent::new(1, 1)?,
            PixelFormat::Rgba8,
            ImageInterpretation {
                color: ColorEncoding::Srgb,
                alpha: AlphaMode::Straight,
            },
            1,
        )?;
        let content = Content::image(ImageAsset::ready(ImagePixels::new(
            descriptor,
            Arc::from(rgba),
            4,
        )?));
        let mut group = ContentGroup::new(id);
        group.push(
            CustomGeometry::RoundedRect {
                bounds: Bounds::new(
                    point(gpui::px(0.), gpui::px(0.)),
                    size(gpui::px(16.), gpui::px(16.)),
                ),
                corner_radii: Corners::default(),
            },
            ContentDraw::new(content).color_output(ContentColorOutput::WideSdr),
        );
        let mut scene = Scene::default();
        let bounds = Bounds::new(
            point(ScaledPixels(4.), ScaledPixels(4.)),
            size(ScaledPixels(16.), ScaledPixels(16.)),
        );
        scene.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Corners::default(),
            opacity: 1.,
            content: group.into_scene_content(),
            coverage: None,
        });
        scene.finish();
        Ok(scene)
    };
    portable_ready(&mut renderer, &make([255, 0, 0, 255])?)?;
    for _ in rx.try_iter() {}
    let gate = Release(Arc::new(MetalContentTestGate::default()));
    renderer.set_content_validation_limits(MetalContentLimits {
        pause_before_node: Some((0, gate.0.clone())),
        ..Default::default()
    });
    let next = make([0, 255, 0, 255])?;
    assert!(renderer.prepare_content_snapshot(&next).is_err());
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    for _ in rx.try_iter() {}
    renderer.set_color_mode(gpui::WindowColorMode::default())?;
    assert!(renderer.prepare_content_snapshot(&next).is_err());
    assert_eq!(renderer.content_presentation_stats().committed_groups, 0);
    gate.0.release();
    rx.recv_timeout(Duration::from_secs(10))?;
    let image = renderer.render_scene_to_image(&next, size(DevicePixels(24), DevicePixels(24)))?;
    assert_eq!(
        image.get_pixel(12, 12).0,
        [0, 0, 0, 255],
        "a late wide result cannot be republished into an SDR window"
    );
    assert_eq!(renderer.content_presentation_stats().committed_groups, 0);
    assert!(!renderer.content_presentation_stats().pending);
    Ok(())
}
