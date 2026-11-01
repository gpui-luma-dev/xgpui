#![cfg(target_os = "macos")]
//! Real producer acceptance. CPU reference math is independent from WGSL lowering.
use gpui::{
    Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, Quad, ScaledPixels,
    Scene, content::*, point, px, size,
};
use gpui_apple::{content_executor::*, metal_renderer::MetalHeadlessRenderer};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
use xgpui_shader::{Gradient, GradientKind, GradientStop};
fn reference(
    kind: GradientKind,
    stops: &[GradientStop],
    w: u32,
    h: u32,
    x: u32,
    y: u32,
) -> [u8; 4] {
    let p = [x as f64 + 0.5, y as f64 + 0.5];
    let size = [w as f64, h as f64];
    let (center, radius, angle) = match kind {
        GradientKind::Linear { angle } => ([0.5; 2], [0.5; 2], angle),
        GradientKind::Radial { center, radius } => (center, radius, 0.),
        GradientKind::Conic { center, angle } => (center, [0.5; 2], angle),
        GradientKind::Diamond {
            center,
            radius,
            angle,
        } => (center, radius, angle),
    };
    let center = center.map(f64::from);
    let radius = radius.map(f64::from);
    let angle = f64::from(angle).rem_euclid(360.).to_radians();
    let (s, c) = angle.sin_cos();
    let ratio = match kind {
        GradientKind::Linear { .. } => {
            0.5 + ((p[0] - size[0] * 0.5) * s - (p[1] - size[1] * 0.5) * c)
                / (s.abs() * size[0] + c.abs() * size[1])
        }
        GradientKind::Radial { .. } => ((p[0] / size[0] - center[0]) / radius[0])
            .hypot((p[1] / size[1] - center[1]) / radius[1]),
        GradientKind::Conic { .. } => {
            let dx = p[0] - center[0] * size[0];
            let dy = p[1] - center[1] * size[1];
            if dx == 0. && dy == 0. {
                0.
            } else {
                ((dy.atan2(dx) + std::f64::consts::FRAC_PI_2 - angle) / std::f64::consts::TAU)
                    .rem_euclid(1.)
            }
        }
        GradientKind::Diamond { .. } => {
            let vx = (p[0] / size[0] - center[0]) / radius[0];
            let vy = (p[1] / size[1] - center[1]) / radius[1];
            (vx * c + vy * s).abs() + (-vx * s + vy * c).abs()
        }
    };
    let mut result = stops.last().unwrap().color.map(f64::from);
    if ratio < f64::from(stops[0].offset) {
        result = stops[0].color.map(f64::from)
    } else {
        for pair in stops.windows(2) {
            if ratio < f64::from(pair[1].offset) {
                let t = (ratio - f64::from(pair[0].offset))
                    / f64::from(pair[1].offset - pair[0].offset);
                let a = pair[0].color.map(f64::from);
                let b = pair[1].color.map(f64::from);
                let alpha = a[3] + (b[3] - a[3]) * t;
                result = std::array::from_fn(|i| {
                    if i == 3 {
                        alpha
                    } else if alpha > 0. {
                        (a[i] * a[3] * (1. - t) + b[i] * b[3] * t) / alpha
                    } else {
                        0.
                    }
                });
                break;
            }
        }
    }
    result.map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
}
fn bw() -> Vec<GradientStop> {
    vec![
        GradientStop::new(0., [0., 0., 0., 1.]),
        GradientStop::new(1., [1.; 4]),
    ]
}
#[test]
fn every_gradient_kind_matches_independent_pixel_reference_and_reuses_one_pipeline() {
    let ex = MetalContentExecutor::new(MetalContentOptions {
        queue_capacity: 1,
        ..Default::default()
    })
    .unwrap();
    let kinds = [
        GradientKind::Linear { angle: 0. },
        GradientKind::Linear { angle: 405. },
        GradientKind::Radial {
            center: [0.2, 0.7],
            radius: [0.3, 0.8],
        },
        GradientKind::Conic {
            center: [0.5; 2],
            angle: 0.,
        },
        GradientKind::Conic {
            center: [0.2, 0.7],
            angle: 450.,
        },
        GradientKind::Diamond {
            center: [0.5; 2],
            radius: [0.3, 0.8],
            angle: 45.,
        },
    ];
    let palettes = [
        bw(),
        vec![
            GradientStop::new(0.2, [1., 0., 0., 1.]),
            GradientStop::new(0.4137, [0., 1., 0., 0.5]),
            GradientStop::new(0.4137, [0., 0., 1., 0.]),
            GradientStop::new(0.8, [1.; 4]),
        ],
        (0..32)
            .map(|i| {
                GradientStop::new(
                    i as f32 / 31.,
                    [
                        (i % 3) as f32 / 2.,
                        (i % 5) as f32 / 4.,
                        (i % 7) as f32 / 6.,
                        (i % 4) as f32 / 3.,
                    ],
                )
            })
            .collect(),
    ];
    let mut first = true;
    for kind in kinds {
        for stops in &palettes {
            for (w, h) in [(33, 17), (65, 31)] {
                let gradient = Gradient::new(kind, stops.clone()).unwrap();
                let extent = ImageExtent::new(w, h).unwrap();
                let graph = ContentGraph::describe(
                    &[ContentRoot {
                        content: gradient.content(),
                        request: ContentRequest::new(size(px(w as f32), px(h as f32)), extent, 1.)
                            .unwrap(),
                    }],
                    ex.capabilities().clone(),
                    GraphLimits::default(),
                )
                .unwrap();
                let frame = ex
                    .submit(Arc::new(graph), MetalContentLimits::default())
                    .unwrap()
                    .wait_for_readback(Duration::from_secs(20))
                    .unwrap();
                assert_eq!(frame.stats.operations, 2);
                assert_eq!(frame.stats.pipeline_compilations, usize::from(first));
                first = false;
                assert!(frame.stats.max_in_flight <= 1);
                let bytes = &frame.images[0].levels[0];
                for y in 0..h {
                    for x in 0..w {
                        let expected = reference(kind, stops, w, h, x, y);
                        let offset = ((y * w + x) * 4) as usize;
                        for i in 0..4 {
                            assert!(
                                bytes[offset + i].abs_diff(expected[i]) <= 2,
                                "{kind:?} {w}x{h} ({x},{y}) {:?} != {expected:?}",
                                &bytes[offset..offset + 4]
                            );
                        }
                    }
                }
            }
        }
    }
}
fn rect(x: f32, w: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(0.)),
        size(ScaledPixels(w), ScaledPixels(8.)),
    )
}
fn draw(gradient: &Gradient, bounds: Bounds<ScaledPixels>, slot: PresentationSlotId) -> CustomDraw {
    CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Default::default(),
        opacity: 1.,
        coverage: None,
        content: ContentDraw::new(gradient.content())
            .slot(slot)
            .into_scene_content(),
    }
}
fn wake(renderer: &mut MetalHeadlessRenderer) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel();
    renderer.set_content_redraw(Arc::new(move || {
        let _ = tx.send(());
    }));
    rx
}
fn ready(renderer: &mut MetalHeadlessRenderer, scene: &Scene, rx: &mpsc::Receiver<()>) {
    if let Err(error) = PlatformHeadlessRenderer::prepare_content_snapshot(renderer, scene) {
        assert_eq!(
            error.downcast_ref::<ContentError>().unwrap().kind,
            ContentErrorKind::Pending
        );
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        PlatformHeadlessRenderer::prepare_content_snapshot(renderer, scene).unwrap();
    }
}
#[test]
fn compositor_shares_realizations_and_caches_replay_edits_and_resize() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let gradient = Gradient::linear(90., bw()).unwrap();
    let slots = [PresentationSlotId::new(), PresentationSlotId::new()];
    let make = |g: &Gradient, width: f32| {
        let mut scene = Scene::default();
        let bounds = rect(0., 32.);
        scene.insert_primitive(Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: gpui::black().into(),
            ..Default::default()
        });
        scene.insert_primitive(draw(g, rect(0., width), slots[0]));
        scene.insert_primitive(draw(g, rect(16., width), slots[1]));
        scene.finish();
        scene
    };
    let scene = make(&gradient, 8.);
    let extent = size(DevicePixels(32), DevicePixels(8));
    ready(&mut renderer, &scene, &rx);
    let image = renderer.render_scene_to_image(&scene, extent).unwrap();
    assert_eq!(image.get_pixel(4, 4), image.get_pixel(20, 4));
    assert!(image.get_pixel(4, 4).0[0] > 0);
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.execution.operations, 2);
    assert_eq!(stats.cached_images, 1);
    assert_eq!(stats.execution.readbacks, 0);
    let mut replay = Scene::default();
    replay.replay(0..scene.len(), &scene);
    replay.finish();
    assert_eq!(
        renderer.render_scene_to_image(&replay, extent).unwrap(),
        image
    );
    assert_eq!(
        renderer.content_presentation_stats().requests,
        stats.requests
    );
    let edited = gradient
        .with_stops(vec![
            GradientStop::new(0., [0., 1., 0., 1.]),
            GradientStop::new(1., [0., 1., 0., 1.]),
        ])
        .unwrap();
    let edit_scene = make(&edited, 8.);
    ready(&mut renderer, &edit_scene, &rx);
    assert_eq!(
        renderer
            .render_scene_to_image(&edit_scene, extent)
            .unwrap()
            .get_pixel(4, 4)
            .0,
        [0, 255, 0, 255]
    );
    assert_eq!(
        renderer
            .content_presentation_stats()
            .execution
            .pipeline_compilations,
        0
    );
    let resized = make(&gradient, 13.);
    ready(&mut renderer, &resized, &rx);
    renderer.render_scene_to_image(&resized, extent).unwrap();
    assert_eq!(
        renderer.content_presentation_stats().requests,
        stats.requests + 2
    );
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
#[test]
fn dense_gradient_submissions_finish_in_a_bounded_subprocess() {
    use std::{process::Command, time::Instant};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "dense_gradient_worker",
            "--nocapture",
        ])
        .env("XGPUI_GRADIENT_CHILD", "1")
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(45) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("dense gradient submission timed out")
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "subprocess helper; invoked by dense_gradient_submissions_finish_in_a_bounded_subprocess"]
fn dense_gradient_worker() {
    assert_eq!(std::env::var("XGPUI_GRADIENT_CHILD").as_deref(), Ok("1"));
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let stops = bw();
    let base = Gradient::linear(0., stops.clone()).unwrap();
    let slots: Vec<_> = (0..512).map(|_| PresentationSlotId::new()).collect();
    for cycle in 0..3 {
        for count in [512, 1, 512] {
            let mut scene = Scene::default();
            let mut kinds = Vec::new();
            for (index, slot) in slots.iter().enumerate().take(count) {
                let kind = GradientKind::Linear {
                    angle: index as f32 * 360. / count as f32 + cycle as f32 * 13.,
                };
                let gradient = base.with_kind(kind).unwrap();
                kinds.push(kind);
                scene.insert_primitive(draw(&gradient, rect(index as f32, 1.), *slot));
            }
            scene.finish();
            let before = renderer.content_presentation_stats().requests;
            ready(&mut renderer, &scene, &rx);
            let extent = size(DevicePixels(count as i32), DevicePixels(8));
            let requests = renderer.content_presentation_stats().requests;
            for _ in 0..2 {
                let image = renderer.render_scene_to_image(&scene, extent).unwrap();
                for (x, y, pixel) in image.enumerate_pixels() {
                    let expected = reference(kinds[x as usize], &stops, 1, 8, 0, y);
                    for (a, b) in pixel.0.into_iter().zip(expected) {
                        assert!(a.abs_diff(b) <= 1, "dense pixel mismatch");
                    }
                }
                let stats = renderer.content_presentation_stats();
                assert_eq!(stats.requests, requests);
                if requests > before {
                    assert_eq!(stats.execution.operations, count + 1);
                }
                assert_eq!(stats.execution.readbacks, 0);
                assert!(stats.execution.max_in_flight <= 2);
                eprintln!(
                    "gradient cycle={cycle} count={count}: pixels verified, cached_replay=true, producer_readbacks=0, max_in_flight={}",
                    stats.execution.max_in_flight
                );
            }
        }
    }
}

// Validate the browser demo's generated source through the real shader parser,
// including native pipeline creation and the maximum supported stop count.
#[path = "../../../../apps/xgpui-ext-lab/extensions-web/src/gradient_preview.rs"]
mod gradient_preview;
#[test]
fn browser_preview_accepts_every_kind_and_32_stops() {
    let stops = (0..32)
        .map(|i| {
            GradientStop::new(
                i as f32 / 31.,
                [i as f32 / 31., 0.3, 1. - i as f32 / 31., 1.],
            )
        })
        .collect();
    let base = Gradient::linear(45., stops).unwrap();
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    for kind in [
        GradientKind::Linear { angle: 45. },
        GradientKind::Radial {
            center: [0.5; 2],
            radius: [0.5; 2],
        },
        GradientKind::Conic {
            center: [0.5; 2],
            angle: 45.,
        },
        GradientKind::Diamond {
            center: [0.5; 2],
            radius: [0.5; 2],
            angle: 45.,
        },
    ] {
        let gradient = base.with_kind(kind).unwrap();
        let xgpui_shader::Paint::Custom(content) = gradient_preview::paint(&gradient) else {
            panic!("preview must use shader paint")
        };
        let mut legacy = Scene::default();
        let mut primitive = draw(&gradient, rect(0., 32.), PresentationSlotId::new());
        primitive.content = content;
        legacy.insert_primitive(primitive);
        legacy.finish();
        let mut portable = Scene::default();
        portable.insert_primitive(draw(&gradient, rect(0., 32.), PresentationSlotId::new()));
        portable.finish();
        let extent = size(DevicePixels(32), DevicePixels(8));
        let actual = renderer.render_scene_to_image(&legacy, extent).unwrap();
        ready(&mut renderer, &portable, &rx);
        let expected = renderer.render_scene_to_image(&portable, extent).unwrap();
        assert!(
            actual
                .as_raw()
                .iter()
                .zip(expected.as_raw())
                .all(|(a, b)| a.abs_diff(*b) <= 2),
            "preview differs for {kind:?}: first {:?} expected {:?}, center {:?} expected {:?}",
            actual.get_pixel(0, 0),
            expected.get_pixel(0, 0),
            actual.get_pixel(16, 4),
            expected.get_pixel(16, 4)
        );
    }
}

#[test]
fn parameter_edits_keep_completed_pixels_visible_while_gpu_work_is_pending() {
    struct Release(Arc<MetalContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    for kind in [
        GradientKind::Linear { angle: 45. },
        GradientKind::Radial {
            center: [0.5; 2],
            radius: [0.5; 2],
        },
        GradientKind::Conic {
            center: [0.5; 2],
            angle: 45.,
        },
        GradientKind::Diamond {
            center: [0.5; 2],
            radius: [0.5; 2],
            angle: 45.,
        },
    ] {
        let mut renderer = MetalHeadlessRenderer::new();
        let rx = wake(&mut renderer);
        let slots = std::array::from_fn::<_, 3, _>(|_| PresentationSlotId::new());
        let make = |gradient: &Gradient| {
            let mut scene = Scene::default();
            for (index, slot) in slots.iter().enumerate() {
                scene.insert_primitive(draw(gradient, rect(index as f32 * 16., 16.), *slot));
            }
            scene.finish();
            scene
        };
        let gradient = Gradient::new(kind, bw()).unwrap();
        let initial = make(&gradient);
        let extent = size(DevicePixels(48), DevicePixels(8));
        ready(&mut renderer, &initial, &rx);
        let completed = renderer.render_scene_to_image(&initial, extent).unwrap();
        let gate = Release(Arc::new(MetalContentTestGate::default()));
        renderer.set_content_validation_limits(MetalContentLimits {
            pause_before_node: Some((0, gate.0.clone())),
            ..Default::default()
        });
        let edited = gradient
            .with_stops(vec![
                GradientStop::new(0., [0., 1., 0., 1.]),
                GradientStop::new(1., [0., 1., 0., 1.]),
            ])
            .unwrap();
        let pending = make(&edited);
        assert_eq!(
            renderer.render_scene_to_image(&pending, extent).unwrap(),
            completed,
            "first pending edit flickered for {kind:?}"
        );
        assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
        for _ in 0..3 {
            assert_eq!(
                renderer.render_scene_to_image(&pending, extent).unwrap(),
                completed,
                "pending replay flickered for {kind:?}"
            );
        }
        gate.0.release();
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        PlatformHeadlessRenderer::prepare_content_snapshot(&mut renderer, &pending).unwrap();
        let updated = renderer.render_scene_to_image(&pending, extent).unwrap();
        assert!(updated.pixels().all(|p| p.0 == [0, 255, 0, 255]));
        assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
    }
}

#[test]
fn conic_edits_present_completed_work_before_accepting_the_next_angle() {
    struct Release(Arc<MetalContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slot = PresentationSlotId::new();
    let stops = bw();
    let make = |angle| {
        let gradient = Gradient::conic([0.5; 2], angle, stops.clone()).unwrap();
        let mut scene = Scene::default();
        scene.insert_primitive(draw(&gradient, rect(0., 32.), slot));
        scene.finish();
        scene
    };
    let extent = size(DevicePixels(32), DevicePixels(8));
    let initial = make(0.);
    ready(&mut renderer, &initial, &rx);
    let first = renderer.render_scene_to_image(&initial, extent).unwrap();
    let next = make(45.);
    assert_eq!(
        renderer.render_scene_to_image(&next, extent).unwrap(),
        first
    );
    rx.recv_timeout(Duration::from_secs(20)).unwrap(); // completed, not yet presented
    let gate = Release(Arc::new(MetalContentTestGate::default()));
    renderer.set_content_validation_limits(MetalContentLimits {
        pause_before_node: Some((0, gate.0.clone())),
        ..Default::default()
    });
    let newest = make(90.);
    let image = renderer.render_scene_to_image(&newest, extent).unwrap();
    for (x, y, pixel) in image.enumerate_pixels() {
        let expected = reference(
            GradientKind::Conic {
                center: [0.5; 2],
                angle: 45.,
            },
            &stops,
            32,
            8,
            x,
            y,
        );
        assert!(
            pixel
                .0
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "completed angle discarded on new input: ({x},{y}) {:?} != {expected:?}",
            pixel.0
        );
    }
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    gate.0.release();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    PlatformHeadlessRenderer::prepare_content_snapshot(&mut renderer, &newest).unwrap();
    let image = renderer.render_scene_to_image(&newest, extent).unwrap();
    for (x, y, pixel) in image.enumerate_pixels() {
        let expected = reference(
            GradientKind::Conic {
                center: [0.5; 2],
                angle: 90.,
            },
            &stops,
            32,
            8,
            x,
            y,
        );
        assert!(
            pixel
                .0
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 2)
        );
    }
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}

#[test]
fn continuous_conic_input_advances_completed_generations() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots = std::array::from_fn::<_, 3, _>(|_| PresentationSlotId::new());
    let base = Gradient::conic([0.5; 2], 0., bw()).unwrap();
    let start = std::time::Instant::now();
    let mut last = Scene::default();
    let mut samples = Vec::new();
    for frame in 0..60 {
        let began = std::time::Instant::now();
        let gradient = base
            .with_kind(GradientKind::Conic {
                center: [0.5; 2],
                angle: frame as f32 * 3.,
            })
            .unwrap();
        let mut scene = Scene::default();
        for (index, slot) in slots.iter().enumerate() {
            // Three different extents model separately realized paint roles.
            let mut primitive = draw(
                &gradient,
                rect(index as f32 * 340., 320. - index as f32),
                *slot,
            );
            primitive.bounds.size.height = ScaledPixels(240.);
            primitive.content_mask.bounds = primitive.bounds;
            scene.insert_primitive(primitive);
        }
        scene.finish();
        let pending =
            PlatformHeadlessRenderer::prepare_content_snapshot(&mut renderer, &scene).unwrap_err();
        assert_eq!(
            pending.downcast_ref::<ContentError>().unwrap().kind,
            ContentErrorKind::Pending
        );
        assert_eq!(
            renderer.content_presentation_stats().completions,
            frame,
            "new input discarded the last completed generation"
        );
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        samples.push(began.elapsed().as_secs_f64() * 1000.);
        last = scene;
    }
    PlatformHeadlessRenderer::prepare_content_snapshot(&mut renderer, &last).unwrap();
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.completions, 60);
    assert_eq!(stats.requests, 60);
    assert_eq!(stats.execution.readbacks, 0);
    assert_eq!(stats.execution.pipeline_compilations, 0);
    assert!(stats.execution.max_in_flight <= 2);
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "conic stream: 60/60 completed, 3 extents, total_ms={:.2}, median_ms={:.2}, p95_ms={:.2}, producer_readbacks=0",
        start.elapsed().as_secs_f64() * 1000.,
        samples[30],
        samples[57]
    );
}
