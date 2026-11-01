#![cfg(all(target_os = "macos", feature = "portable-shaders"))]
//! Procedural paints through the shared portable worker and window presenter.
use gpui::{
    Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, ScaledPixels, Scene,
    content::*, point, px, size,
};
use gpui_apple::{content_executor::*, metal_renderer::MetalHeadlessRenderer};
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use xgpui_shader::shader_paint::{ShaderPaint, ShaderProgram};
const SOURCE: &str = r#"
struct Parameters { gain:f32, color:vec3<f32>, alpha:f32 }
fn paint(input:PaintInput)->vec4<f32>{return vec4(parameters.color+vec3(input.uv.x*parameters.gain,input.position.y/200.,input.size.x/400.),parameters.alpha);}
"#;
fn params(gain: f32, color: [f32; 3], alpha: f32) -> UniformValues {
    UniformValues::new()
        .float("gain", gain)
        .vec3("color", color)
        .float("alpha", alpha)
}
fn request(w: u32, h: u32, scale: f32) -> ContentRequest {
    ContentRequest::new(
        size(px(w as f32 / scale), px(h as f32 / scale)),
        ImageExtent::new(w, h).unwrap(),
        scale,
    )
    .unwrap()
}
fn bytes(values: [f32; 4]) -> [u8; 4] {
    values.map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
}
#[test]
fn source_pixels_preserve_logical_coordinates_straight_alpha_and_clamping() {
    let executor = MetalContentExecutor::new(MetalContentOptions {
        queue_capacity: 1,
        ..Default::default()
    })
    .unwrap();
    let program = ShaderProgram::new(SOURCE).unwrap();
    let mut first = true;
    for alpha in [-0.5, 0.5, 1.5] {
        let paint = program.paint(params(0.5, [-0.2, 0.3, 0.4], alpha)).unwrap();
        let roots = [
            request(33, 17, 1.),
            request(33, 17, 0.5),
            request(65, 31, 2.),
        ]
        .map(|request| ContentRoot {
            content: paint.content(),
            request,
        });
        let graph = ContentGraph::describe(
            &roots,
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
        assert_eq!(frame.stats.pipeline_compilations, usize::from(first));
        first = false;
        for (root, image) in roots.iter().zip(frame.images) {
            let (w, h) = (
                root.request.extent().width(),
                root.request.extent().height(),
            );
            let logical = root.request.logical_size();
            let data = &image.levels[0];
            for y in 0..h {
                for x in 0..w {
                    let expected = bytes([
                        -0.2 + (x as f32 + 0.5) / w as f32 * 0.5,
                        0.3 + (y as f32 + 0.5) / h as f32 * f32::from(logical.height) / 200.,
                        0.4 + f32::from(logical.width) / 400.,
                        alpha,
                    ]);
                    let offset = ((y * w + x) * 4) as usize;
                    assert!(
                        data[offset..offset + 4]
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| a.abs_diff(b) <= 1),
                        "source pixels {:?} != {expected:?}",
                        &data[offset..offset + 4]
                    );
                }
            }
        }
    }
}
#[test]
fn typed_matrix_integer_loop_derivative_and_discard_pixels_are_native() {
    let source = "struct Parameters { index:i32,count:u32,transform:mat2x2<f32> } fn paint(input:PaintInput)->vec4<f32>{let dx=dpdx(input.uv.x); if input.uv.x<0.25 {discard;} var v=input.uv; for(var i=0u;i<2u;i++){v=parameters.transform*v;} return vec4(v.x+dx,v.y,f32(parameters.index)*0.2+f32(parameters.count)*0.1,1.);}";
    let paint = ShaderProgram::new(source)
        .unwrap()
        .paint(
            UniformValues::new()
                .ints("index", [2])
                .uints("count", [3])
                .matrix("transform", 2, 2, [1., 0., 0.25, 0.5]),
        )
        .unwrap();
    let ex = MetalContentExecutor::new(Default::default()).unwrap();
    let graph = ContentGraph::describe(
        &[ContentRoot {
            content: paint.content(),
            request: request(32, 16, 1.),
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
    let data = &frame.images[0].levels[0];
    for y in 0..16 {
        for x in 0..32 {
            let expected = if x < 8 {
                [0; 4]
            } else {
                bytes([
                    (x as f32 + 1.5) / 32. + 0.375 * (y as f32 + 0.5) / 16.,
                    0.25 * (y as f32 + 0.5) / 16.,
                    0.7,
                    1.,
                ])
            };
            let offset = ((y * 32 + x) * 4) as usize;
            assert!(
                data[offset..offset + 4]
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 1)
            );
        }
    }
}
fn scene(paint: &ShaderPaint, slots: &[PresentationSlotId; 3], large: bool) -> Scene {
    let mut scene = Scene::default();
    for (i, slot) in slots.iter().enumerate() {
        let bounds = Bounds::new(
            point(ScaledPixels(i as f32 * 340.), ScaledPixels(0.)),
            size(
                ScaledPixels(if large { 320. - i as f32 } else { 16. }),
                ScaledPixels(if large { 240. } else { 8. }),
            ),
        );
        scene.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Default::default(),
            opacity: 1.,
            coverage: None,
            content: ContentDraw::new(paint.content())
                .slot(*slot)
                .into_scene_content(),
        });
    }
    scene.finish();
    scene
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
fn pending_parameters_keep_completed_pixels_and_promote_on_completion() {
    struct Release(Arc<MetalContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    let program = ShaderProgram::new(SOURCE).unwrap();
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots = std::array::from_fn(|_| PresentationSlotId::new());
    let paint = program.paint(params(0., [1., 0., 0.], 1.)).unwrap();
    let old = scene(&paint, &slots, false);
    ready(&mut renderer, &old, &rx);
    let extent = size(DevicePixels(696), DevicePixels(8));
    let initial = renderer.render_scene_to_image(&old, extent).unwrap();
    let gate = Release(Arc::new(MetalContentTestGate::default()));
    renderer.set_content_validation_limits(MetalContentLimits {
        pause_before_node: Some((0, gate.0.clone())),
        ..Default::default()
    });
    let paint = program.paint(params(0., [0., 1., 0.], 1.)).unwrap();
    let new = scene(&paint, &slots, false);
    assert_eq!(
        renderer.render_scene_to_image(&new, extent).unwrap(),
        initial
    );
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    assert_eq!(
        renderer.render_scene_to_image(&new, extent).unwrap(),
        initial
    );
    gate.0.release();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    renderer.prepare_content_snapshot(&new).unwrap();
    let result = renderer.render_scene_to_image(&new, extent).unwrap();
    assert_eq!(result.get_pixel(8, 4).0[1], 255);
    assert_eq!(result.get_pixel(8, 4).0[0], 0);
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.execution.pipeline_compilations, 0);
    assert_eq!(stats.execution.readbacks, 0);
    let requests = stats.requests;
    assert_eq!(
        renderer.render_scene_to_image(&new, extent).unwrap(),
        result
    );
    assert_eq!(renderer.content_presentation_stats().requests, requests);
}
#[test]
fn continuous_parameter_edits_advance_all_completed_generations_without_readback() {
    let program = ShaderProgram::new(SOURCE).unwrap();
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots = std::array::from_fn(|_| PresentationSlotId::new());
    let mut times = Vec::new();
    let mut last = Scene::default();
    for frame in 0..60 {
        let start = Instant::now();
        let paint = program
            .paint(params(frame as f32 / 60., [0.1, 0.2, 0.3], 0.75))
            .unwrap();
        let next = scene(&paint, &slots, true);
        let e = renderer.prepare_content_snapshot(&next).unwrap_err();
        assert_eq!(
            e.downcast_ref::<ContentError>().unwrap().kind,
            ContentErrorKind::Pending
        );
        assert_eq!(renderer.content_presentation_stats().completions, frame);
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.);
        last = next;
    }
    renderer.prepare_content_snapshot(&last).unwrap();
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.requests, 60);
    assert_eq!(stats.completions, 60);
    assert_eq!(stats.execution.operations, 3);
    assert_eq!(stats.execution.pipeline_compilations, 0);
    assert_eq!(stats.execution.readbacks, 0);
    assert!(stats.execution.max_in_flight <= 2);
    times.sort_by(f64::total_cmp);
    eprintln!(
        "shader paint stream: 60/60 completed, 3 extents; median_ms={:.2}, p95_ms={:.2}, producer_readbacks=0",
        times[30], times[57]
    );
}

#[test]
fn dense_procedural_draws_finish_in_a_bounded_subprocess() {
    use std::process::Command;
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "dense_shader_worker", "--nocapture"])
        .env("XGPUI_SHADER_CHILD", "1")
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
            panic!("dense procedural draws timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "subprocess helper; invoked by dense_procedural_draws_finish_in_a_bounded_subprocess"]
fn dense_shader_worker() {
    assert_eq!(std::env::var("XGPUI_SHADER_CHILD").as_deref(), Ok("1"));
    let program=ShaderProgram::new("struct Parameters { color:vec4<f32> } fn paint(input:PaintInput)->vec4<f32>{return parameters.color;}").unwrap();
    let mut renderer = MetalHeadlessRenderer::new();
    let rx = wake(&mut renderer);
    let slots: Vec<_> = (0..512).map(|_| PresentationSlotId::new()).collect();
    for cycle in 0..2 {
        for count in [512, 1, 512] {
            let mut scene = Scene::default();
            for (index, slot) in slots.iter().enumerate().take(count) {
                let paint = program
                    .paint(
                        UniformValues::new()
                            .vec4("color", [index as f32 / 511., cycle as f32, 0.25, 1.]),
                    )
                    .unwrap();
                let bounds = Bounds::new(
                    point(ScaledPixels(index as f32), ScaledPixels(0.)),
                    size(ScaledPixels(1.), ScaledPixels(8.)),
                );
                scene.insert_primitive(CustomDraw {
                    order: 0,
                    bounds,
                    content_mask: ContentMask { bounds },
                    corner_radii: Default::default(),
                    opacity: 1.,
                    coverage: None,
                    content: ContentDraw::new(paint.content())
                        .slot(*slot)
                        .into_scene_content(),
                });
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
                    let expected = bytes([x as f32 / 511., cycle as f32, 0.25, 1.]);
                    assert!(
                        pixel
                            .0
                            .into_iter()
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
                "shader cycle={cycle} count={count}: pixels verified, cached_replay=true, producer_readbacks=0"
            );
        }
    }
}
