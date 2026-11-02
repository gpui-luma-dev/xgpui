#![cfg(all(target_os = "macos", feature = "test-support"))]
//! Atomic native-draw presentation. Only the final validation target is read back.
use gpui::{
    Bounds, ContentMask, Corners, CustomDraw, CustomGeometry, DevicePixels,
    PlatformHeadlessRenderer, Quad, ScaledPixels, Scene, black, content::*, point, px, rgb, size,
};
use gpui_apple::{content_executor::*, metal_renderer::MetalHeadlessRenderer};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
const WIDTH: i32 = 80;
const HEIGHT: i32 = 48;
fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(w), ScaledPixels(h)),
    )
}
#[derive(Debug)]
struct Cell {
    index: usize,
    cool: bool,
}
impl ContentSource for Cell {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> std::result::Result<ImageRef, ContentError> {
        let v = self.index as f32 / 511.;
        let image = cx.solid(
            request.extent(),
            ColorValue::srgb(if self.cool {
                [0., v, 1., 1.]
            } else {
                [1., v, 0., 1.]
            })?,
        )?;
        cx.copy_image(image)
    }
}
fn group(id: ContentGroupId, dense: bool, cool: bool) -> ContentGroup {
    let mut group = ContentGroup::new(id);
    for index in 0..if dense { 512 } else { 1 } {
        group.push(
            CustomGeometry::RoundedRect {
                bounds: if dense {
                    Bounds::new(
                        point(px((index % 32) as f32 * 2.), px((index / 32) as f32 * 2.)),
                        size(px(1.), px(1.)),
                    )
                } else {
                    Bounds::new(point(px(0.), px(0.)), size(px(64.), px(32.)))
                },
                corner_radii: Corners::default(),
            },
            Content::new(Cell { index, cool }),
        );
    }
    group
}
fn scene(group: ContentGroup, blue_ui: bool) -> Scene {
    let mut scene = Scene::default();
    let viewport = bounds(0., 0., WIDTH as f32, HEIGHT as f32);
    scene.insert_primitive(Quad {
        bounds: viewport,
        content_mask: ContentMask { bounds: viewport },
        background: black().into(),
        ..Default::default()
    });
    scene.insert_primitive(Quad {
        bounds: bounds(72., 0., 4., 4.),
        content_mask: ContentMask { bounds: viewport },
        background: if blue_ui {
            rgb(0x0000ff)
        } else {
            rgb(0xffffff)
        }
        .into(),
        ..Default::default()
    });
    scene.insert_primitive(CustomDraw {
        order: 0,
        bounds: bounds(4., 4., 64., 32.),
        content_mask: ContentMask { bounds: viewport },
        corner_radii: Corners::default(),
        opacity: 1.,
        content: group.into_scene_content(),
        coverage: None,
    });
    scene.finish();
    scene
}
fn setup() -> (MetalHeadlessRenderer, mpsc::Receiver<()>) {
    let mut renderer = MetalHeadlessRenderer::new();
    let (tx, rx) = mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = tx.send(());
    }));
    (renderer, rx)
}
fn read(renderer: &mut MetalHeadlessRenderer, scene: &Scene) -> image::RgbaImage {
    renderer
        .render_scene_to_image(scene, size(DevicePixels(WIDTH), DevicePixels(HEIGHT)))
        .unwrap()
}
fn completed(rx: &mpsc::Receiver<()>) {
    rx.recv_timeout(Duration::from_secs(20))
        .expect("producer must request redraw");
}
struct Release(Arc<MetalContentTestGate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
fn pause(renderer: &mut MetalHeadlessRenderer) -> Release {
    let gate = Arc::new(MetalContentTestGate::default());
    renderer.set_content_validation_limits(MetalContentLimits {
        pause_before_node: Some((0, gate.clone())),
        ..Default::default()
    });
    Release(gate)
}
fn canvas_eq(actual: &image::RgbaImage, expected: &image::RgbaImage) {
    for y in 4..36 {
        for x in 4..68 {
            assert_eq!(
                actual.get_pixel(x, y),
                expected.get_pixel(x, y),
                "({x},{y})"
            );
        }
    }
}
fn canvas_solid(image: &image::RgbaImage, color: [u8; 4]) {
    for y in 4..36 {
        for x in 4..68 {
            assert_eq!(image.get_pixel(x, y).0, color, "({x},{y})");
        }
    }
}

#[test]
fn paused_gpu_keeps_complete_groups_in_both_density_directions() {
    if isolated("paused_gpu_keeps_complete_groups_in_both_density_directions") {
        return;
    }
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let dense = scene(group(id, true, false), false);
    // The first group has no history, so no partial root is displayed.
    canvas_solid(&read(&mut renderer, &dense), [0, 0, 0, 255]);
    completed(&rx);
    let grid = read(&mut renderer, &dense);
    assert_eq!(grid.get_pixel(4, 4).0, [255, 0, 0, 255]);
    assert_eq!(grid.get_pixel(5, 5).0, [0, 0, 0, 255]);
    assert_eq!(
        renderer.content_presentation_stats().execution.operations,
        1024
    );

    let gate = pause(&mut renderer);
    let single = scene(group(id, false, true), true);
    let pending = read(&mut renderer, &single);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    canvas_eq(&pending, &grid);
    assert_eq!(
        pending.get_pixel(72, 0).0,
        [0, 0, 255, 255],
        "unrelated UI must repaint while producer is paused"
    );
    for _ in 0..3 {
        canvas_eq(&read(&mut renderer, &single), &grid);
    }
    assert!(renderer.prepare_content_snapshot(&single).is_err());
    gate.0.release();
    completed(&rx);
    let large = read(&mut renderer, &single);
    canvas_solid(&large, [0, 0, 255, 255]);

    let gate = pause(&mut renderer);
    let restored = scene(group(id, true, false), false);
    canvas_eq(&read(&mut renderer, &restored), &large);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    for _ in 0..3 {
        canvas_eq(&read(&mut renderer, &restored), &large);
    }
    gate.0.release();
    completed(&rx);
    canvas_eq(&read(&mut renderer, &restored), &grid);
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.requests, 3);
    assert_eq!(stats.completions, 3);
    assert_eq!(stats.retained_slots, 0);
    assert_eq!(stats.committed_groups, 1);
    assert_eq!(stats.execution.operations, 1024);
    assert_eq!(stats.execution.readbacks, 0);
}

#[test]
fn retained_group_obeys_current_placement_clip_opacity_and_scene_order() {
    if isolated("retained_group_obeys_current_placement_clip_opacity_and_scene_order") {
        return;
    }
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let initial = scene(group(id, false, false), false);
    read(&mut renderer, &initial);
    completed(&rx);
    read(&mut renderer, &initial);
    let gate = pause(&mut renderer);
    let mut changed = scene(group(id, true, true), true);
    // Replay a changed placement through actual scene insertion, including a
    // foreground quad and an explicit layer, so draw order must survive expansion.
    let mut pending = Scene::default();
    let viewport = bounds(0., 0., 80., 48.);
    pending.insert_primitive(Quad {
        bounds: viewport,
        content_mask: ContentMask { bounds: viewport },
        background: black().into(),
        ..Default::default()
    });
    pending.push_layer(viewport);
    let mut draw = changed.custom_draws.remove(0);
    draw.bounds.origin = point(ScaledPixels(8.), ScaledPixels(8.));
    draw.content_mask = ContentMask {
        bounds: bounds(8., 8., 16., 16.),
    };
    draw.opacity = 0.5;
    pending.insert_primitive(draw);
    pending.pop_layer();
    pending.insert_primitive(Quad {
        bounds: bounds(8., 8., 2., 2.),
        content_mask: ContentMask { bounds: viewport },
        background: rgb(0x00ff00).into(),
        ..Default::default()
    });
    pending.finish();
    let image = read(&mut renderer, &pending);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    assert_eq!(
        image.get_pixel(8, 8).0,
        [0, 255, 0, 255],
        "foreground order"
    );
    assert!(
        (i16::from(image.get_pixel(12, 12).0[0]) - 128).abs() <= 1,
        "current opacity"
    );
    assert_eq!(
        image.get_pixel(4, 4).0,
        [0, 0, 0, 255],
        "old absolute origin must not persist"
    );
    assert_eq!(image.get_pixel(25, 12).0, [0, 0, 0, 255], "current clip");
    gate.0.release();
    completed(&rx);
    read(&mut renderer, &pending);
}

#[test]
fn removal_and_empty_groups_retire_history_and_reject_late_completion() {
    if isolated("removal_and_empty_groups_retire_history_and_reject_late_completion") {
        return;
    }
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let initial = scene(group(id, false, false), false);
    read(&mut renderer, &initial);
    completed(&rx);
    read(&mut renderer, &initial);
    let gate = pause(&mut renderer);
    let replacement = scene(group(id, true, true), false);
    read(&mut renderer, &replacement);
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    read(&mut renderer, &Scene::default());
    assert_eq!(renderer.content_presentation_stats().committed_groups, 0);
    let returning = scene(group(id, true, true), false);
    canvas_solid(&read(&mut renderer, &returning), [0, 0, 0, 255]);
    gate.0.release();
    // Cancellation can complete without a wake; bounded polling only in this
    // teardown test waits for the newly admitted generation's own completion.
    completed(&rx);
    let image = read(&mut renderer, &returning);
    assert_eq!(image.get_pixel(4, 4).0, [0, 0, 255, 255]);
    let empty = scene(ContentGroup::new(id), false);
    canvas_solid(&read(&mut renderer, &empty), [0, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().committed_groups, 0);
    read(&mut renderer, &Scene::default());
    assert_eq!(renderer.content_presentation_stats().retained_gpu_bytes, 0);
}

#[test]
fn malformed_group_rejects_all_members_and_applies_error_policy() {
    if isolated("malformed_group_rejects_all_members_and_applies_error_policy") {
        return;
    }
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let initial = scene(group(id, false, false), false);
    read(&mut renderer, &initial);
    completed(&rx);
    let previous = read(&mut renderer, &initial);
    let mut invalid = group(id, true, true);
    invalid.push(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(f32::NAN), px(1.))),
            corner_radii: Corners::default(),
        },
        Content::solid(ColorValue::srgb([0., 1., 0., 1.]).unwrap()),
    );
    let keep = scene(invalid.clone(), false);
    canvas_eq(&read(&mut renderer, &keep), &previous);
    assert_eq!(
        renderer.content_presentation_stats().requests,
        1,
        "no valid subset may be admitted"
    );
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
        invalid.clone().presentation(
            PendingPresentation::RetainPrevious,
            ContentErrorPresentation::ReportAndTransparent,
        ),
        false,
    );
    canvas_solid(&read(&mut renderer, &clear), [0, 0, 0, 255]);
    let strict = scene(
        invalid.presentation(
            PendingPresentation::RetainPrevious,
            ContentErrorPresentation::FailFrame,
        ),
        false,
    );
    assert!(
        renderer
            .render_scene_to_image(&strict, size(DevicePixels(WIDTH), DevicePixels(HEIGHT)))
            .is_err()
    );
}

#[test]
fn admitted_group_makes_progress_while_newer_layouts_coalesce() {
    if isolated("admitted_group_makes_progress_while_newer_layouts_coalesce") {
        return;
    }
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let initial = scene(group(id, false, false), false);
    read(&mut renderer, &initial);
    completed(&rx);
    let red = read(&mut renderer, &initial);
    let first = pause(&mut renderer);
    let admitted = scene(group(id, true, true), false);
    canvas_eq(&read(&mut renderer, &admitted), &red);
    assert!(first.0.wait_until_entered(Duration::from_secs(5)));
    let mut latest = scene(group(id, false, false), false);
    for _ in 0..12 {
        latest = scene(group(id, false, false), true);
        canvas_eq(&read(&mut renderer, &latest), &red);
        assert_eq!(
            renderer.content_presentation_stats().requests,
            2,
            "editing must not cancel admitted work"
        );
    }
    let second = pause(&mut renderer);
    first.0.release();
    completed(&rx);
    let progress = read(&mut renderer, &latest);
    assert!(second.0.wait_until_entered(Duration::from_secs(5)));
    // The admitted blue generation has 512 small members. Its geometry and all
    // images publish together, even though the newest request is one large red tile.
    for y in 0..32 {
        for x in 0..64 {
            let p = progress.get_pixel(x + 4, y + 4).0;
            if x % 2 == 0 && y % 2 == 0 {
                assert_eq!(p[0], 0);
                assert_eq!(p[2], 255);
                assert_eq!(p[3], 255);
            } else {
                assert_eq!(p, [0, 0, 0, 255]);
            }
        }
    }
    assert_eq!(renderer.content_presentation_stats().requests, 3);
    second.0.release();
    completed(&rx);
    canvas_eq(&read(&mut renderer, &latest), &red);
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}

#[test]
fn cached_geometry_changes_and_hidden_groups_keep_their_history() {
    if isolated("cached_geometry_changes_and_hidden_groups_keep_their_history") {
        return;
    }
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let initial = scene(group(id, false, false), false);
    read(&mut renderer, &initial);
    completed(&rx);
    read(&mut renderer, &initial);
    let mut shifted = ContentGroup::new(id);
    shifted.push(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(point(px(8.), px(0.)), size(px(64.), px(32.))),
            corner_radii: Corners::default(),
        },
        Content::new(Cell {
            index: 0,
            cool: false,
        }),
    );
    let shifted = scene(shifted, false);
    let image = read(&mut renderer, &shifted);
    assert_eq!(image.get_pixel(4, 4).0, [0, 0, 0, 255]);
    assert_eq!(image.get_pixel(12, 4).0, [255, 0, 0, 255]);
    assert_eq!(
        renderer.content_presentation_stats().requests,
        1,
        "geometry-only update uses cached complete image"
    );
    let mut hidden = Scene::default();
    let mut draw = shifted.custom_draws[0].clone();
    draw.opacity = 0.;
    draw.content_mask = ContentMask {
        bounds: bounds(0., 0., 0., 0.),
    };
    hidden.insert_primitive(draw);
    hidden.finish();
    read(&mut renderer, &hidden);
    assert_eq!(
        renderer.content_presentation_stats().committed_groups,
        1,
        "hiding is not removal"
    );
    assert!(renderer.content_presentation_stats().retained_gpu_bytes > 0);
    canvas_eq(&read(&mut renderer, &shifted), &image);
    assert_eq!(renderer.content_presentation_stats().requests, 1);
    let mut duplicate = Scene::default();
    duplicate.insert_primitive(shifted.custom_draws[0].clone());
    duplicate.insert_primitive(shifted.custom_draws[0].clone());
    duplicate.finish();
    assert!(
        renderer
            .render_scene_to_image(&duplicate, size(DevicePixels(WIDTH), DevicePixels(HEIGHT)))
            .is_err()
    );
}

// Keep dense GPU regressions bounded even if a future driver/submission bug hangs.
fn isolated(name: &str) -> bool {
    if std::env::var("XGPUI_ATOMIC_GROUP_CHILD").as_deref() == Ok(name) {
        return false;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("XGPUI_ATOMIC_GROUP_CHILD", name)
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{name}");
            return true;
        }
        if start.elapsed() > Duration::from_secs(45) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("atomic group GPU test timed out: {name}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn group_color_policy_retries_and_never_retains_incompatible_wide_images() {
    if isolated("group_color_policy_retries_and_never_retains_incompatible_wide_images") {
        return;
    }
    let (mut renderer, rx) = setup();
    let mut g = ContentGroup::new(ContentGroupId::new());
    g.push(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(64.), px(32.))),
            corner_radii: Corners::default(),
        },
        Content::solid(
            ColorValue::new(
                [0.25, 0.125, 0., 0.5],
                ImageInterpretation {
                    color: ColorEncoding::LinearP3,
                    alpha: AlphaMode::Premultiplied,
                },
            )
            .unwrap(),
        ),
    );
    let s = scene(g, false);
    canvas_solid(&read(&mut renderer, &s), [0, 0, 0, 255]);
    assert_eq!(
        renderer
            .content_presentation_stats()
            .last_error
            .unwrap()
            .kind,
        ContentErrorKind::UnsupportedCapability
    );
    renderer
        .set_color_mode(gpui::WindowColorMode::WideGamutSdr)
        .unwrap();
    renderer
        .render_scene_to_linear_p3(&s, size(DevicePixels(WIDTH), DevicePixels(HEIGHT)))
        .unwrap();
    completed(&rx);
    let image = renderer
        .render_scene_to_linear_p3(&s, size(DevicePixels(WIDTH), DevicePixels(HEIGHT)))
        .unwrap();
    assert_eq!(
        image.pixels[(4 * WIDTH + 4) as usize],
        [0x3400, 0x3000, 0, 0x3c00]
    );
    renderer.prepare_content_snapshot(&s).unwrap();
    renderer
        .set_color_mode(gpui::WindowColorMode::Default)
        .unwrap();
    canvas_solid(&read(&mut renderer, &s), [0, 0, 0, 255]);
    assert_eq!(renderer.content_presentation_stats().committed_groups, 0);
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}

#[test]
fn admission_failure_keeps_the_complete_previous_group_without_retrying_forever() {
    if isolated("admission_failure_keeps_the_complete_previous_group_without_retrying_forever") {
        return;
    }
    let (mut renderer, rx) = setup();
    let id = ContentGroupId::new();
    let initial = scene(group(id, false, false), false);
    read(&mut renderer, &initial);
    completed(&rx);
    let previous = read(&mut renderer, &initial);
    renderer.set_content_validation_limits(MetalContentLimits {
        max_gpu_bytes: 1,
        ..Default::default()
    });
    let replacement = scene(group(id, true, true), false);
    canvas_eq(&read(&mut renderer, &replacement), &previous);
    completed(&rx);
    for _ in 0..3 {
        canvas_eq(&read(&mut renderer, &replacement), &previous);
    }
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.requests, 2);
    assert!(!stats.pending);
    assert_eq!(
        stats.last_error.unwrap().kind,
        ContentErrorKind::ResourceLimit
    );
    renderer.set_content_validation_limits(Default::default());
    let retry = scene(group(id, true, true), false);
    read(&mut renderer, &retry);
    completed(&rx);
    let image = read(&mut renderer, &retry);
    assert_eq!(image.get_pixel(4, 4).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(5, 5).0, [0, 0, 0, 255]);
}

#[test]
fn group_members_keep_native_rounded_border_and_path_coverage() {
    if isolated("group_members_keep_native_rounded_border_and_path_coverage") {
        return;
    }
    let (mut renderer, rx) = setup();
    let mut g = ContentGroup::new(ContentGroupId::new());
    let red = Content::solid(ColorValue::srgb([1., 0., 0., 1.]).unwrap());
    g.push(
        CustomGeometry::RoundedRect {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(8.), px(8.))),
            corner_radii: Corners::all(px(3.)),
        },
        red.clone(),
    );
    let mut border = gpui::fill(
        Bounds::new(point(px(12.), px(0.)), size(px(8.), px(8.))),
        black(),
    );
    border.border_widths = gpui::Edges::all(px(2.));
    g.push(CustomGeometry::Border(border), red.clone());
    let mut path = gpui::PathBuilder::fill();
    path.move_to(point(px(24.), px(0.)));
    path.line_to(point(px(32.), px(0.)));
    path.line_to(point(px(24.), px(8.)));
    path.close();
    g.push(CustomGeometry::Path(path.build().unwrap()), red);
    let s = scene(g, false);
    read(&mut renderer, &s);
    completed(&rx);
    let image = read(&mut renderer, &s);
    for (x, y, expected) in [
        (4, 4, [0, 0, 0, 255]),
        (8, 8, [255, 0, 0, 255]),
        (16, 4, [255, 0, 0, 255]),
        (20, 8, [0, 0, 0, 255]),
        (28, 4, [255, 0, 0, 255]),
        (35, 11, [0, 0, 0, 255]),
    ] {
        assert_eq!(image.get_pixel(x, y).0, expected, "({x},{y})");
    }
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
