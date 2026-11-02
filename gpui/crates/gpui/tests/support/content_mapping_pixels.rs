// Same independent pixel references run against each native compositor.
use gpui::{
    Bounds, ContentMask, Corners, CustomDraw, CustomGeometry, DevicePixels,
    PlatformHeadlessRenderer, Quad, ScaledPixels, Scene, black, content::*, point, px, size,
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
const TARGET: i32 = 12;
fn bounds() -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(2.), ScaledPixels(2.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    )
}
fn setup() -> (Renderer, mpsc::Receiver<()>) {
    let mut renderer = new_renderer();
    let (tx, rx) = mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = tx.send(());
    }));
    (renderer, rx)
}
fn completed(rx: &mpsc::Receiver<()>) {
    rx.recv_timeout(Duration::from_secs(20))
        .expect("completion redraw");
}
fn scene(content: Arc<dyn gpui::CustomContent>) -> Scene {
    let mut scene = Scene::default();
    let viewport = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(TARGET as f32), ScaledPixels(TARGET as f32)),
    );
    scene.insert_primitive(Quad {
        bounds: viewport,
        content_mask: ContentMask { bounds: viewport },
        background: black().into(),
        ..Default::default()
    });
    scene.insert_primitive(CustomDraw {
        order: 0,
        bounds: bounds(),
        content_mask: ContentMask { bounds: viewport },
        corner_radii: Default::default(),
        opacity: 1.,
        content,
        coverage: None,
    });
    scene.finish();
    scene
}
fn read(renderer: &mut Renderer, scene: &Scene) -> image::RgbaImage {
    renderer
        .render_scene_to_image(scene, size(DevicePixels(TARGET), DevicePixels(TARGET)))
        .unwrap()
}
fn descriptor(w: u32, h: u32, mips: u32) -> ImageDescriptor {
    ImageDescriptor::new(
        ImageExtent::new(w, h).unwrap(),
        PixelFormat::Rgba8,
        ImageInterpretation {
            color: ColorEncoding::Srgb,
            alpha: AlphaMode::Straight,
        },
        mips,
    )
    .unwrap()
}
fn color(x: u32, y: u32) -> [u8; 4] {
    [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ][((x + y * 2) % 4) as usize]
}
fn source(w: u32, h: u32) -> Content {
    let bytes: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).flat_map(move |x| color(x, y)))
        .collect();
    Content::image(ImageAsset::ready(
        ImagePixels::new(descriptor(w, h, 1), bytes.into(), 4096).unwrap(),
    ))
}
fn nearest(fit: ImageFit) -> ImageMapping {
    ImageMapping::new(fit)
        .sampler(Sampler::nearest_clamp())
        .unwrap()
}
fn assert_canvas(image: &image::RgbaImage, expected: impl Fn(u32, u32) -> [u8; 4]) {
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                image.get_pixel(x + 2, y + 2).0,
                expected(x, y),
                "destination ({x},{y})"
            );
        }
    }
}
fn ready(renderer: &mut Renderer, rx: &mpsc::Receiver<()>, scene: &Scene) -> image::RgbaImage {
    read(renderer, scene);
    completed(rx);
    read(renderer, scene)
}
fn mapped_group(id: ContentGroupId, content: Content, mapping: ImageMapping) -> ContentGroup {
    let mut group = ContentGroup::new(id);
    group.push_mapped(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(8.), px(8.))),
            corner_radii: Corners::default(),
        },
        content,
        mapping,
    );
    group
}
#[test]
fn image_fit_alignment_crop_and_mapping_changes_match_pixel_reference() {
    let (mut renderer, rx) = setup();
    let content = source(4, 2);
    let initial = scene(
        ContentDraw::new(content.clone())
            .mapping(nearest(ImageFit::Stretch))
            .into_scene_content(),
    );
    let image = ready(&mut renderer, &rx, &initial);
    assert_canvas(&image, |x, y| color(x / 2, y / 4));
    for alignment in [0., 0.5, 1.] {
        let start = (alignment * 4.) as u32;
        let mapping = nearest(ImageFit::Contain)
            .alignment([0.5, alignment])
            .unwrap();
        let requested = scene(
            ContentDraw::new(content.clone())
                .mapping(mapping)
                .into_scene_content(),
        );
        let image = read(&mut renderer, &requested);
        assert_canvas(&image, |x, y| {
            if (start..start + 4).contains(&y) {
                color(x / 2, (y - start) / 2)
            } else {
                [0, 0, 0, 255]
            }
        });
        let mapping = nearest(ImageFit::Cover)
            .alignment([alignment, 0.5])
            .unwrap();
        let requested = scene(
            ContentDraw::new(content.clone())
                .mapping(mapping)
                .into_scene_content(),
        );
        let image = read(&mut renderer, &requested);
        assert_canvas(&image, |x, y| color((alignment * 2.) as u32 + x / 4, y / 4));
    }
    let mapping =
        nearest(ImageFit::Stretch).crop(Some(ImageCrop::new([1., 0.], [2., 2.]).unwrap()));
    let image = read(
        &mut renderer,
        &scene(
            ContentDraw::new(content)
                .mapping(mapping)
                .into_scene_content(),
        ),
    );
    assert_canvas(&image, |x, y| color(1 + x / 4, y / 4));
    assert_eq!(
        renderer.content_presentation_stats().requests,
        1,
        "mapping changes must not re-upload the source"
    );
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
#[test]
fn linear_sampling_is_independent_of_nearest_sampling_and_crop_edges() {
    let (mut renderer, rx) = setup();
    let content = Content::image(ImageAsset::ready(
        ImagePixels::new(
            descriptor(2, 1, 1),
            Arc::from([255, 0, 0, 255, 0, 0, 255, 255]),
            8,
        )
        .unwrap(),
    ));
    let requested = scene(ContentDraw::new(content.clone()).into_scene_content());
    let image = ready(&mut renderer, &rx, &requested);
    // At destination centers x=2 and x=5, source coordinates are .625 and 1.375.
    for (x, expected) in [(2, [223u8, 0, 32, 255]), (5, [32, 0, 223, 255])] {
        let actual = image.get_pixel(x + 2, 5).0;
        for c in 0..4 {
            assert!(
                (i16::from(actual[c]) - i16::from(expected[c])).abs() <= 1,
                "{actual:?} != {expected:?}"
            );
        }
    }
    let mapping = ImageMapping::default().crop(Some(ImageCrop::new([0., 0.], [1., 1.]).unwrap()));
    let image = read(
        &mut renderer,
        &scene(
            ContentDraw::new(content)
                .mapping(mapping)
                .into_scene_content(),
        ),
    );
    assert_canvas(&image, |_, _| [255, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
}
#[test]
fn pending_slot_mapping_uses_retained_image_dimensions() {
    let (mut renderer, rx) = setup();
    let slot = PresentationSlotId::new();
    let old = scene(
        ContentDraw::new(source(4, 2))
            .slot(slot)
            .into_scene_content(),
    );
    ready(&mut renderer, &rx, &old);
    let (asset, done) = ImageAsset::pending();
    let requested = scene(
        ContentDraw::new(Content::image(asset))
            .slot(slot)
            .mapping(nearest(ImageFit::Contain))
            .into_scene_content(),
    );
    let retained = read(&mut renderer, &requested);
    assert_canvas(&retained, |x, y| {
        if (2..6).contains(&y) {
            color(x / 2, (y - 2) / 2)
        } else {
            [0, 0, 0, 255]
        }
    });
    // New square source must not affect the retained wide source's fit.
    done.resolve(Ok(ImagePixels::new(
        descriptor(1, 1, 1),
        Arc::from([0, 255, 0, 255]),
        4,
    )
    .unwrap()));
    completed(&rx);
    read(&mut renderer, &requested);
    completed(&rx);
    assert_canvas(&read(&mut renderer, &requested), |_, _| [0, 255, 0, 255]);
}
#[test]
fn atomic_group_retains_committed_mapping_until_replacement_is_ready() {
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let old = scene(
        mapped_group(
            id,
            source(4, 2),
            nearest(ImageFit::Contain).alignment([0., 0.]).unwrap(),
        )
        .into_scene_content(),
    );
    let previous = ready(&mut renderer, &rx, &old);
    let (asset, done) = ImageAsset::pending();
    let requested = scene(
        mapped_group(
            id,
            Content::image(asset),
            nearest(ImageFit::Contain).alignment([0., 1.]).unwrap(),
        )
        .into_scene_content(),
    );
    assert_eq!(read(&mut renderer, &requested), previous);
    assert!(renderer.prepare_content_snapshot(&requested).is_err());
    let bytes: Vec<u8> = (0..8).flat_map(|_| [0, 255, 0, 255]).collect();
    done.resolve(Ok(
        ImagePixels::new(descriptor(4, 2, 1), bytes.into(), 32).unwrap()
    ));
    completed(&rx);
    read(&mut renderer, &requested);
    completed(&rx);
    let image = read(&mut renderer, &requested);
    assert_canvas(&image, |_, y| {
        if y >= 4 {
            [0, 255, 0, 255]
        } else {
            [0, 0, 0, 255]
        }
    });
    renderer.prepare_content_snapshot(&requested).unwrap();
}
#[test]
fn explicit_mip_sampling_and_default_level_zero_are_distinct() {
    let (mut renderer, rx) = setup();
    let mut bytes = Vec::new();
    for (count, color) in [
        (256, [255, 0, 0, 255]),
        (64, [0, 255, 0, 255]),
        (16, [0, 0, 255, 255]),
        (4, [255, 255, 0, 255]),
        (1, [255, 0, 255, 255]),
    ] {
        for _ in 0..count {
            bytes.extend(color);
        }
    }
    let content = Content::image(ImageAsset::ready(
        ImagePixels::new(descriptor(16, 16, 5), bytes.into(), 4096).unwrap(),
    ));
    let requested = scene(ContentDraw::new(content.clone()).into_scene_content());
    assert_canvas(&ready(&mut renderer, &rx, &requested), |_, _| {
        [255, 0, 0, 255]
    });
    let mut sampler = Sampler::nearest_clamp();
    sampler.mip_filter = Some(FilterMode::Nearest);
    let requested = scene(
        ContentDraw::new(content)
            .mapping(ImageMapping::default().sampler(sampler).unwrap())
            .into_scene_content(),
    );
    assert_canvas(&read(&mut renderer, &requested), |_, _| [0, 255, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
}

#[test]
fn invalid_group_crop_uses_group_policy_and_recovers() {
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let content = source(4, 2);
    let initial =
        scene(mapped_group(id, content.clone(), nearest(ImageFit::Contain)).into_scene_content());
    let previous = ready(&mut renderer, &rx, &initial);
    let invalid =
        nearest(ImageFit::Stretch).crop(Some(ImageCrop::new([3., 0.], [2., 2.]).unwrap()));
    let group = mapped_group(id, content.clone(), invalid);
    let keep = scene(group.clone().into_scene_content());
    assert_eq!(read(&mut renderer, &keep), previous);
    assert_eq!(
        renderer
            .content_presentation_stats()
            .last_error
            .unwrap()
            .kind,
        ContentErrorKind::InvalidDescriptor
    );
    assert!(renderer.prepare_content_snapshot(&keep).is_err());
    let clear = scene(
        group
            .clone()
            .presentation(
                PendingPresentation::RetainPrevious,
                ContentErrorPresentation::ReportAndTransparent,
            )
            .into_scene_content(),
    );
    assert_canvas(&read(&mut renderer, &clear), |_, _| [0, 0, 0, 255]);
    let fail = scene(
        group
            .presentation(
                PendingPresentation::RetainPrevious,
                ContentErrorPresentation::FailFrame,
            )
            .into_scene_content(),
    );
    assert!(
        renderer
            .render_scene_to_image(&fail, size(DevicePixels(TARGET), DevicePixels(TARGET)))
            .is_err()
    );
    let fixed = scene(mapped_group(id, content, nearest(ImageFit::Stretch)).into_scene_content());
    let before = renderer.content_presentation_stats().requests;
    read(&mut renderer, &fixed);
    if renderer.content_presentation_stats().requests > before {
        completed(&rx);
    }
    assert_canvas(&read(&mut renderer, &fixed), |x, y| color(x / 2, y / 4));
    assert!(renderer.content_presentation_stats().last_error.is_none());
    renderer.prepare_content_snapshot(&fixed).unwrap();
}

#[test]
fn mapped_image_keeps_full_geometry_coverage_clip_and_opacity() {
    let (mut renderer, rx) = setup();
    let content = Content::image(ImageAsset::ready(
        ImagePixels::new(
            descriptor(4, 2, 1),
            (0..8)
                .flat_map(|_| [255, 0, 0, 255])
                .collect::<Vec<u8>>()
                .into(),
            32,
        )
        .unwrap(),
    ));
    let mut requested = scene(
        ContentDraw::new(content)
            .mapping(nearest(ImageFit::Cover))
            .into_scene_content(),
    );
    requested.custom_draws[0].opacity = 0.5;
    requested.custom_draws[0].corner_radii = Corners::all(ScaledPixels(4.));
    requested.custom_draws[0].content_mask.bounds = Bounds::new(
        point(ScaledPixels(2.), ScaledPixels(2.)),
        size(ScaledPixels(6.), ScaledPixels(8.)),
    );
    let image = ready(&mut renderer, &rx, &requested);
    assert_eq!(
        image.get_pixel(2, 2).0,
        [0, 0, 0, 255],
        "rounded corner uses full destination"
    );
    assert_eq!(
        image.get_pixel(5, 5).0,
        [128, 0, 0, 255],
        "opacity applied once after mapping"
    );
    assert_eq!(
        image.get_pixel(8, 5).0,
        [0, 0, 0, 255],
        "clip does not resize source mapping"
    );
}

struct Release(Arc<ContentGate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
#[test]
fn paused_gpu_retains_group_mapping_then_commits_it_with_new_pixels() {
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let old = scene(
        mapped_group(
            id,
            source(4, 2),
            nearest(ImageFit::Contain).alignment([0., 0.]).unwrap(),
        )
        .into_scene_content(),
    );
    let previous = ready(&mut renderer, &rx, &old);
    let new = Content::image(ImageAsset::ready(
        ImagePixels::new(
            descriptor(4, 2, 1),
            (0..8)
                .flat_map(|_| [0, 255, 0, 255])
                .collect::<Vec<u8>>()
                .into(),
            32,
        )
        .unwrap(),
    ));
    let requested = scene(
        mapped_group(
            id,
            new,
            nearest(ImageFit::Contain).alignment([0., 1.]).unwrap(),
        )
        .into_scene_content(),
    );
    let gate = Release(Arc::new(ContentGate::default()));
    renderer.set_content_validation_limits(ContentLimits {
        pause_before_node: Some((0, gate.0.clone())),
        ..Default::default()
    });
    assert_eq!(read(&mut renderer, &requested), previous);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    assert_eq!(read(&mut renderer, &requested), previous);
    gate.0.release();
    completed(&rx);
    assert_canvas(&read(&mut renderer, &requested), |_, y| {
        if y >= 4 {
            [0, 255, 0, 255]
        } else {
            [0, 0, 0, 255]
        }
    });
}

#[test]
fn pending_replacement_crop_incompatible_with_history_stays_transparent() {
    let (mut renderer, rx) = setup();
    let slot = PresentationSlotId::new();
    let old = scene(
        ContentDraw::new(source(4, 2))
            .slot(slot)
            .into_scene_content(),
    );
    ready(&mut renderer, &rx, &old);
    let (asset, done) = ImageAsset::pending();
    let mapping =
        nearest(ImageFit::Stretch).crop(Some(ImageCrop::new([4., 0.], [4., 2.]).unwrap()));
    let requested = scene(
        ContentDraw::new(Content::image(asset))
            .slot(slot)
            .mapping(mapping)
            .into_scene_content(),
    );
    assert_canvas(&read(&mut renderer, &requested), |_, _| [0, 0, 0, 255]);
    assert!(renderer.content_presentation_stats().last_error.is_none());
    done.resolve(Ok(ImagePixels::new(
        descriptor(8, 2, 1),
        (0..16)
            .flat_map(|_| [0, 255, 0, 255])
            .collect::<Vec<u8>>()
            .into(),
        64,
    )
    .unwrap()));
    completed(&rx);
    read(&mut renderer, &requested);
    completed(&rx);
    assert_canvas(&read(&mut renderer, &requested), |_, _| [0, 255, 0, 255]);
}

#[test]
fn cropped_mip_sampling_clamps_to_selected_level_texel_centers() {
    let (mut renderer, rx) = setup();
    let mut bytes = Vec::new();
    for (w, h) in [(32, 16), (16, 8), (8, 4)] {
        for _ in 0..h {
            for x in 0..w {
                bytes.extend(if w == 8 && (1..7).contains(&x) {
                    [0, 255, 0, 255]
                } else {
                    [255, 0, 0, 255]
                });
            }
        }
    }
    let content = Content::image(ImageAsset::ready(
        ImagePixels::new(descriptor(32, 16, 3), bytes.into(), 4096).unwrap(),
    ));
    let mut sampler = Sampler::linear_clamp();
    sampler.mip_filter = Some(FilterMode::Nearest);
    let mapping = ImageMapping::default()
        .crop(Some(ImageCrop::new([4., 0.], [24., 16.]).unwrap()))
        .sampler(sampler)
        .unwrap();
    let requested = scene(
        ContentDraw::new(content)
            .mapping(mapping)
            .into_scene_content(),
    );
    // The selected 8x4 mip has red neighbors outside the crop. Edge filtering
    // must clamp to its green texel centers, without changing source derivatives.
    assert_canvas(&ready(&mut renderer, &rx, &requested), |_, _| {
        [0, 255, 0, 255]
    });
}

#[test]
fn linear_mip_filter_blends_levels_using_unclamped_source_derivatives() {
    let (mut renderer, rx) = setup();
    let mut bytes = Vec::new();
    for (count, color) in [
        (576, [255, 0, 0, 255]),
        (144, [0, 255, 0, 255]),
        (36, [0, 0, 255, 255]),
    ] {
        for _ in 0..count {
            bytes.extend(color);
        }
    }
    let content = Content::image(ImageAsset::ready(
        ImagePixels::new(descriptor(24, 24, 3), bytes.into(), 4096).unwrap(),
    ));
    let mut sampler = Sampler::linear_clamp();
    sampler.mip_filter = Some(FilterMode::Linear);
    let requested = scene(
        ContentDraw::new(content)
            .mapping(ImageMapping::default().sampler(sampler).unwrap())
            .into_scene_content(),
    );
    let image = ready(&mut renderer, &rx, &requested);
    // 24 -> 8 means LOD log2(3): blend the green and blue mip levels.
    for y in 2..10 {
        for x in 2..10 {
            let actual = image.get_pixel(x, y).0;
            for (channel, expected) in actual.into_iter().zip([0i16, 106, 149, 255]) {
                assert!((i16::from(channel) - expected).abs() <= 2, "{actual:?}");
            }
        }
    }
}
