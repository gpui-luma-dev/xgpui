#![cfg(target_os = "macos")]
//! Real portable mesh execution, independent CPU tessellation and presentation.
use gpui::{
    Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, ScaledPixels, Scene,
    content::*, point, px, size,
};
use gpui_apple::{content_executor::*, metal_renderer::MetalHeadlessRenderer};
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use xgpui_shader::{MeshGradient, MeshGrid};
#[derive(Clone, Copy)]
struct Vertex {
    p: [f64; 2],
    c: [f64; 3],
}
fn evaluate(grid: &MeshGrid, x: usize, y: usize, u: f64, v: f64) -> Vertex {
    let basis = |t: f64| {
        [
            2. * t * t * t - 3. * t * t + 1.,
            -2. * t * t * t + 3. * t * t,
            t * t * t - 2. * t * t + t,
            t * t * t - t * t,
        ]
    };
    let a = basis(u);
    let b = basis(v);
    let w = grid.width() as usize;
    let points = [
        grid.points()[y * w + x],
        grid.points()[y * w + x + 1],
        grid.points()[(y + 1) * w + x],
        grid.points()[(y + 1) * w + x + 1],
    ];
    let p = std::array::from_fn(|c| {
        b[0] * (a[0] * f64::from(points[0].position[c])
            + a[1] * f64::from(points[1].position[c])
            + a[2] * f64::from(points[0].u_tangent[c])
            + a[3] * f64::from(points[1].u_tangent[c]))
            + b[1]
                * (a[0] * f64::from(points[2].position[c])
                    + a[1] * f64::from(points[3].position[c])
                    + a[2] * f64::from(points[2].u_tangent[c])
                    + a[3] * f64::from(points[3].u_tangent[c]))
            + b[2]
                * (a[0] * f64::from(points[0].v_tangent[c])
                    + a[1] * f64::from(points[1].v_tangent[c]))
            + b[3]
                * (a[0] * f64::from(points[2].v_tangent[c])
                    + a[1] * f64::from(points[3].v_tangent[c]))
    });
    let c = std::array::from_fn(|c| {
        b[0] * (a[0] * f64::from(points[0].color[c]) + a[1] * f64::from(points[1].color[c]))
            + b[1] * (a[0] * f64::from(points[2].color[c]) + a[1] * f64::from(points[3].color[c]))
    });
    Vertex { p, c }
}
fn triangles(grid: &MeshGrid, samples: u32) -> Vec<[Vertex; 3]> {
    let mut result = Vec::new();
    for y in 0..grid.height() as usize - 1 {
        for x in 0..grid.width() as usize - 1 {
            for j in 0..samples - 1 {
                for i in 0..samples - 1 {
                    let at = |i, j| {
                        evaluate(
                            grid,
                            x,
                            y,
                            f64::from(i) / f64::from(samples - 1),
                            f64::from(j) / f64::from(samples - 1),
                        )
                    };
                    let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                    result.extend([[a, b, c], [c, d, a]]);
                }
            }
        }
    }
    result
}
fn reference(triangles: &[[Vertex; 3]], x: u32, y: u32, w: u32, h: u32) -> [u8; 4] {
    let p = [
        (f64::from(x) + 0.5) / f64::from(w),
        (f64::from(y) + 0.5) / f64::from(h),
    ];
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (a[0] - c[0]) * (b[1] - c[1]) - (a[1] - c[1]) * (b[0] - c[0])
    };
    let mut result = [0; 4];
    for [a, b, c] in triangles {
        let area = cross(a.p, b.p, c.p);
        if area.abs() < 1e-12 {
            continue;
        }
        let u = cross(p, b.p, c.p) / area;
        let v = cross(a.p, p, c.p) / area;
        let t = 1. - u - v;
        // Top-left fill rule, including reversed triangles in folded patches.
        let inside = |weight: f64, mut from: [f64; 2], mut to: [f64; 2]| {
            if weight.abs() > 1e-9 {
                return weight > 0.;
            }
            if area < 0. {
                std::mem::swap(&mut from, &mut to);
            }
            to[1] < from[1] || (to[1] == from[1] && to[0] > from[0])
        };
        if inside(u, b.p, c.p) && inside(v, c.p, a.p) && inside(t, a.p, b.p) {
            result = std::array::from_fn(|i| {
                if i == 3 {
                    255
                } else {
                    ((a.c[i] * u + b.c[i] * v + c.c[i] * t).clamp(0., 1.) * 255.).round() as u8
                }
            });
        }
    }
    result
}
fn grid(w: u32, h: u32) -> MeshGrid {
    MeshGrid::regular(
        w,
        h,
        (0..w * h)
            .map(|i| {
                [
                    (i % 3) as f32 / 2.,
                    (i % 5) as f32 / 4.,
                    (i % 7) as f32 / 6.,
                ]
            })
            .collect(),
    )
    .unwrap()
}
#[test]
fn compute_and_render_match_independent_triangle_pixels() {
    let executor = MetalContentExecutor::new(MetalContentOptions {
        queue_capacity: 1,
        ..Default::default()
    })
    .unwrap();
    let mut distorted = grid(3, 3).points().to_vec();
    distorted[4].position = [0.63, 0.38];
    distorted[4].u_tangent = [0.4, 0.12];
    distorted[4].v_tangent = [-0.08, 0.45];
    let mut inset = grid(2, 2).points().to_vec();
    for p in &mut inset {
        p.position = p.position.map(|v| v * 0.6 + 0.2);
        p.u_tangent = p.u_tangent.map(|v| v * 0.6);
        p.v_tangent = p.v_tangent.map(|v| v * 0.6);
    }
    let mut folded = grid(3, 2).points().to_vec();
    for i in [2, 5] {
        folded[i].position[0] = 0.2;
    }
    let grids = [
        grid(2, 2),
        grid(3, 2),
        MeshGrid::new(3, 3, distorted).unwrap(),
        MeshGrid::new(2, 2, inset).unwrap(),
        MeshGrid::new(3, 2, folded).unwrap(),
    ];
    let mut first = true;
    for grid in grids {
        for samples in [2, 3, 8] {
            let mesh = MeshGradient::new(grid.clone(), samples).unwrap();
            let (w, h) = (65, 33);
            let extent = ImageExtent::new(w, h).unwrap();
            let graph = ContentGraph::describe(
                &[ContentRoot {
                    content: mesh.content(),
                    request: ContentRequest::new(size(px(w as f32), px(h as f32)), extent, 1.)
                        .unwrap(),
                }],
                executor.capabilities().clone(),
                GraphLimits::default(),
            )
            .unwrap();
            assert_eq!(graph.nodes().len(), 3);
            let frame = executor
                .submit(Arc::new(graph), MetalContentLimits::default())
                .unwrap()
                .wait_for_readback(Duration::from_secs(20))
                .unwrap();
            assert_eq!(frame.stats.pipeline_compilations, if first { 2 } else { 0 });
            first = false;
            assert!(frame.stats.max_in_flight <= 1);
            let triangles = triangles(&grid, samples);
            let image = &frame.images[0].levels[0];
            for y in 0..h {
                for x in 0..w {
                    let expected = reference(&triangles, x, y, w, h);
                    let offset = ((y * w + x) * 4) as usize;
                    assert!(
                        image[offset..offset + 4]
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| a.abs_diff(b) <= 2),
                        "{}x{} samples={samples} ({x},{y}) {:?} != {expected:?}",
                        grid.width(),
                        grid.height(),
                        &image[offset..offset + 4]
                    );
                }
            }
        }
    }
}
fn draw(mesh: &MeshGradient, x: f32, w: f32, slot: PresentationSlotId) -> CustomDraw {
    let bounds = Bounds::new(
        point(ScaledPixels(x), ScaledPixels(0.)),
        size(ScaledPixels(w), ScaledPixels(8.)),
    );
    CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Default::default(),
        opacity: 1.,
        coverage: None,
        content: ContentDraw::new(mesh.content())
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
    if let Err(e) = renderer.prepare_content_snapshot(scene) {
        assert_eq!(
            e.downcast_ref::<ContentError>().unwrap().kind,
            ContentErrorKind::Pending
        );
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        renderer.prepare_content_snapshot(scene).unwrap();
    }
}
#[test]
fn presentation_shares_tessellation_and_keeps_edits_visible() {
    struct Release(Arc<MetalContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots = std::array::from_fn::<_, 3, _>(|_| PresentationSlotId::new());
    let make = |mesh: &MeshGradient, width: f32| {
        let mut scene = Scene::default();
        for (i, slot) in slots.iter().enumerate() {
            scene.insert_primitive(draw(mesh, i as f32 * 20., width, *slot));
        }
        scene.finish();
        scene
    };
    let mesh = MeshGradient::new(grid(3, 3), 18).unwrap();
    let scene = make(&mesh, 16.);
    let extent = size(DevicePixels(60), DevicePixels(8));
    ready(&mut renderer, &scene, &rx);
    let initial = renderer.render_scene_to_image(&scene, extent).unwrap();
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.cached_images, 1);
    assert_eq!(stats.execution.operations, 3);
    assert_eq!(stats.execution.readbacks, 0);
    assert_eq!(
        renderer.render_scene_to_image(&scene, extent).unwrap(),
        initial
    );
    assert_eq!(
        renderer.content_presentation_stats().requests,
        stats.requests
    );
    let gate = Release(Arc::new(MetalContentTestGate::default()));
    renderer.set_content_validation_limits(MetalContentLimits {
        pause_before_node: Some((0, gate.0.clone())),
        ..Default::default()
    });
    let green = MeshGrid::regular(3, 3, vec![[0., 1., 0.]; 9]).unwrap();
    let edited = mesh.with_grid(green).unwrap().with_subdivisions(8).unwrap();
    let edited_scene = make(&edited, 16.);
    assert_eq!(
        renderer
            .render_scene_to_image(&edited_scene, extent)
            .unwrap(),
        initial
    );
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    assert_eq!(
        renderer
            .render_scene_to_image(&edited_scene, extent)
            .unwrap(),
        initial
    );
    gate.0.release();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    renderer.prepare_content_snapshot(&edited_scene).unwrap();
    assert_eq!(
        renderer
            .render_scene_to_image(&edited_scene, extent)
            .unwrap()
            .get_pixel(8, 4)
            .0,
        [0, 255, 0, 255]
    );
    renderer.set_content_validation_limits(Default::default());
    let resized = make(&edited, 19.);
    ready(&mut renderer, &resized, &rx);
    assert_eq!(
        renderer
            .render_scene_to_image(&resized, extent)
            .unwrap()
            .get_pixel(9, 4)
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
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
#[test]
fn continuous_mesh_edits_advance_without_discarding_completed_frames() {
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots = std::array::from_fn::<_, 3, _>(|_| PresentationSlotId::new());
    let base = MeshGradient::new(grid(3, 3), 18).unwrap();
    let mut last = Scene::default();
    let mut samples = Vec::new();
    for frame in 0..60 {
        let start = Instant::now();
        let mut points = base.grid().points().to_vec();
        points[4].position = [0.4 + frame as f32 / 300., 0.5];
        points[4].u_tangent = [0.4, frame as f32 / 600.];
        let mesh = base
            .with_grid(MeshGrid::new(3, 3, points).unwrap())
            .unwrap();
        let mut scene = Scene::default();
        for (i, slot) in slots.iter().enumerate() {
            let mut primitive = draw(&mesh, i as f32 * 340., 320. - i as f32, *slot);
            primitive.bounds.size.height = ScaledPixels(240.);
            primitive.content_mask.bounds = primitive.bounds;
            scene.insert_primitive(primitive);
        }
        scene.finish();
        let error = renderer.prepare_content_snapshot(&scene).unwrap_err();
        assert_eq!(
            error.downcast_ref::<ContentError>().unwrap().kind,
            ContentErrorKind::Pending
        );
        assert_eq!(renderer.content_presentation_stats().completions, frame);
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        samples.push(start.elapsed().as_secs_f64() * 1000.);
        last = scene;
    }
    renderer.prepare_content_snapshot(&last).unwrap();
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.completions, 60);
    assert_eq!(stats.execution.operations, 5);
    assert_eq!(stats.execution.readbacks, 0);
    assert_eq!(stats.execution.pipeline_compilations, 0);
    assert!(stats.execution.max_in_flight <= 2);
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "mesh stream: 60/60 completed; three extents share tessellation; median_ms={:.2}; p95_ms={:.2}; producer_readbacks=0",
        samples[30], samples[57]
    );
}

#[test]
fn dense_mesh_updates_finish_in_a_bounded_subprocess() {
    use std::process::Command;
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "dense_mesh_worker", "--nocapture"])
        .env("XGPUI_MESH_CHILD", "1")
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
            panic!("dense mesh updates exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "subprocess helper; invoked by dense_mesh_updates_finish_in_a_bounded_subprocess"]
fn dense_mesh_worker() {
    assert_eq!(std::env::var("XGPUI_MESH_CHILD").as_deref(), Ok("1"));
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots: Vec<_> = (0..512).map(|_| PresentationSlotId::new()).collect();
    for cycle in 0..2 {
        for count in [512, 1, 512] {
            let mut scene = Scene::default();
            let mut colors = Vec::new();
            for (index, slot) in slots.iter().enumerate().take(count) {
                let color = [index as f32 / 511., cycle as f32, 0.25];
                colors.push(color);
                let mesh =
                    MeshGradient::new(MeshGrid::regular(2, 2, vec![color; 4]).unwrap(), 2).unwrap();
                scene.insert_primitive(draw(&mesh, index as f32, 1., *slot));
            }
            scene.finish();
            ready(&mut renderer, &scene, &rx);
            let requests = renderer.content_presentation_stats().requests;
            for _ in 0..2 {
                let image = renderer
                    .render_scene_to_image(
                        &scene,
                        size(DevicePixels(count as i32), DevicePixels(8)),
                    )
                    .unwrap();
                for (x, _, pixel) in image.enumerate_pixels() {
                    let expected = colors[x as usize].map(|v| (v * 255.).round() as u8);
                    assert_eq!(pixel.0[3], 255);
                    assert!(
                        pixel.0[..3]
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| a.abs_diff(b) <= 1)
                    );
                }
                let stats = renderer.content_presentation_stats();
                assert_eq!(stats.requests, requests);
                assert_eq!(stats.execution.readbacks, 0);
                assert!(stats.execution.max_in_flight <= 2);
            }
            eprintln!(
                "mesh cycle={cycle} count={count}: pixels verified, cached_replay=true, producer_readbacks=0"
            );
        }
    }
}
