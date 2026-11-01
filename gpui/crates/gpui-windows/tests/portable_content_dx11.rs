#![cfg(all(target_os = "windows", feature = "test-support"))]
//! Native GPU acceptance. The dense case runs in a child with a process timeout.
use gpui::{content::*, px, size};
use gpui_windows::content_executor::*;
use std::{
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, ContentError>;
type Describe = dyn Fn(&mut ContentBuilder, &ContentRequest) -> Result<ImageRef> + Send + Sync;
struct Source(Box<Describe>);
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeDx11Source")
    }
}
impl ContentSource for Source {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest) -> Result<ImageRef> {
        (self.0)(cx, request)
    }
}
fn source(
    f: impl Fn(&mut ContentBuilder, &ContentRequest) -> Result<ImageRef> + Send + Sync + 'static,
) -> Content {
    Content::new(Source(Box::new(f)))
}
fn extent() -> ImageExtent {
    ImageExtent::new(4, 4).unwrap()
}
fn srgb() -> ImageInterpretation {
    ImageInterpretation {
        color: ColorEncoding::Srgb,
        alpha: AlphaMode::Straight,
    }
}
fn p3(alpha: AlphaMode) -> ImageInterpretation {
    ImageInterpretation {
        color: ColorEncoding::LinearP3,
        alpha,
    }
}
fn target() -> ImageDescriptor {
    ImageDescriptor::new(extent(), PixelFormat::Rgba8, srgb(), 1).unwrap()
}
fn executor() -> Dx11ContentExecutor {
    Dx11ContentExecutor::new(Dx11ContentOptions {
        queue_capacity: 1,
        ..Default::default()
    })
    .unwrap()
}
fn graph(executor: &Dx11ContentExecutor, contents: Vec<Content>) -> Arc<ContentGraph> {
    let roots: Vec<_> = contents
        .into_iter()
        .map(|content| ContentRoot {
            content,
            request: ContentRequest::new(size(px(4.), px(4.)), extent(), 1.).unwrap(),
        })
        .collect();
    Arc::new(
        ContentGraph::describe(
            &roots,
            executor.capabilities().clone(),
            GraphLimits::default(),
        )
        .unwrap(),
    )
}
fn frame(ex: &Dx11ContentExecutor, contents: Vec<Content>) -> Dx11ContentFrame {
    ex.submit(graph(ex, contents), Dx11ContentLimits::default())
        .unwrap()
        .wait_for_readback(Duration::from_secs(20))
        .unwrap()
}
fn all_rgba8(image: &Dx11ContentImage, expected: [u8; 4]) {
    assert_eq!(image.descriptor.format(), PixelFormat::Rgba8);
    for pixel in image.levels[0].as_chunks::<4>().0 {
        assert_eq!(*pixel, expected);
    }
}
fn half(value: u16) -> f32 {
    let sign = if value & 0x8000 == 0 { 1. } else { -1. };
    let e = (value >> 10) & 31;
    let m = f32::from(value & 1023);
    if e == 0 {
        sign * m * 2f32.powi(-24)
    } else {
        sign * (1. + m / 1024.) * 2f32.powi(i32::from(e) - 15)
    }
}
fn fp16(image: &Dx11ContentImage, x: usize, y: usize) -> [f32; 4] {
    assert_eq!(image.descriptor.format(), PixelFormat::Rgba16Float);
    let offset = (y * image.descriptor.extent().width() as usize + x) * 8;
    std::array::from_fn(|c| {
        half(u16::from_le_bytes(
            image.levels[0][offset + c * 2..offset + c * 2 + 2]
                .try_into()
                .unwrap(),
        ))
    })
}
fn close(actual: [f32; 4], expected: [f32; 4]) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < 0.001, "{actual:?} vs {expected:?}");
    }
}
fn solid() -> Content {
    Content::solid(ColorValue::srgb([0.25, 0.5, 1., 1.]).unwrap())
}

#[test]
fn native_solids_preserve_rgba8_and_linear_p3_fp16_meaning() {
    let ex = executor();
    let result = frame(
        &ex,
        vec![
            solid(),
            Content::solid(
                ColorValue::new([0.25, 0., 0., 0.5], p3(AlphaMode::Premultiplied)).unwrap(),
            ),
        ],
    );
    all_rgba8(&result.images[0], [64, 128, 255, 255]);
    for y in 0..4 {
        for x in 0..4 {
            close(fp16(&result.images[1], x, y), [0.25, 0., 0., 0.5]);
        }
    }
    assert_eq!(result.stats.operations, 2);
    assert_eq!(result.stats.readbacks, 2);
    assert_eq!(result.stats.max_in_flight, 1);
    assert_eq!(
        result.stats.worker_thread.as_deref(),
        Some("dx11-portable-content")
    );
}

const COLOR: &str = "struct Parameters { color: vec4<f32> } @group(0) @binding(0) var<uniform> p: Parameters; @fragment fn paint() -> @location(0) vec4<f32> { return p.color; }";
const SAMPLE: &str = "@group(0) @binding(0) var image: texture_2d<f32>; @group(0) @binding(1) var filtering: sampler; @fragment fn paint(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { return textureSample(image, filtering, uv); }";
fn color_program(interpretation: ImageInterpretation) -> PortableProgram {
    PortableProgram::fullscreen(COLOR, "paint", interpretation, ImageContracts::new()).unwrap()
}
fn sample_program(shader: &str) -> PortableProgram {
    PortableProgram::fullscreen(
        shader,
        "paint",
        srgb(),
        ImageContracts::new().image("image", srgb()),
    )
    .unwrap()
}

#[test]
fn native_uniforms_and_fullscreen_coordinates_match_pixel_centers() {
    let ex = executor();
    let uniform = source(|cx, _| {
        cx.render(
            RenderPass::fullscreen(&color_program(srgb()), target())
                .uniform("p", UniformValues::new().vec4("color", [0.25, 0.5, 1., 1.])),
        )
    });
    let uv = PortableProgram::fullscreen("@fragment fn paint(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { return vec4(uv, 0.25, 1.0); }", "paint", p3(AlphaMode::Straight), ImageContracts::new()).unwrap();
    let gradient = source(move |cx, _| {
        cx.render(RenderPass::fullscreen(
            &uv,
            ImageDescriptor::linear_p3_f16(extent(), AlphaMode::Straight)?,
        ))
    });
    let matrix = source(|cx, _| {
        let p = PortableProgram::fullscreen(
            "struct P { gain: f32, tint: vec3<f32>, mode: u32, transform: mat3x3<f32> } @group(0) @binding(0) var<uniform> p: P; @fragment fn paint() -> @location(0) vec4<f32> { return vec4((p.transform * p.tint) * p.gain + vec3(f32(p.mode)), 1.0); }",
            "paint",
            srgb(),
            ImageContracts::new(),
        )?;
        cx.render(
            RenderPass::fullscreen(&p, target()).uniform(
                "p",
                UniformValues::new()
                    .float("gain", 0.5)
                    .vec3("tint", [0.5, 0.25, 1.])
                    .uints("mode", vec![0])
                    .matrix(
                        "transform",
                        3,
                        3,
                        vec![1., 0., 0., 0., 1., 0., 0.25, 0., 0.5],
                    ),
            ),
        )
    });
    let result = frame(&ex, vec![uniform, gradient, matrix]);
    all_rgba8(&result.images[0], [64, 128, 255, 255]);
    for y in 0..4 {
        for x in 0..4 {
            close(
                fp16(&result.images[1], x, y),
                [(x as f32 + 0.5) / 4., (y as f32 + 0.5) / 4., 0.25, 1.],
            );
        }
    }
    all_rgba8(&result.images[2], [96, 32, 64, 255]);
    assert_eq!(result.stats.pipeline_compilations, 3);
}

#[test]
fn native_source_sized_sampling_and_pipeline_replay_are_correct() {
    let ex = executor();
    let p = sample_program(SAMPLE);
    let source = source(move |cx, _| {
        let image = cx.upload_image(
            ImageDescriptor::new(ImageExtent::new(2, 1)?, PixelFormat::Rgba8, srgb(), 1)?,
            Arc::from([255, 0, 0, 255, 0, 255, 0, 255]),
        )?;
        cx.render(
            RenderPass::fullscreen(&p, target())
                .image("image", image)
                .sampler("filtering", Sampler::nearest_clamp()),
        )
    });
    let g = graph(&ex, vec![source]);
    for iteration in 0..3 {
        let f = ex
            .submit(g.clone(), Dx11ContentLimits::default())
            .unwrap()
            .wait_for_readback(Duration::from_secs(20))
            .unwrap();
        assert_eq!(f.stats.pipeline_compilations, usize::from(iteration == 0));
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    &f.images[0].levels[0][(y * 4 + x) * 4..(y * 4 + x + 1) * 4],
                    if x < 2 {
                        &[255, 0, 0, 255]
                    } else {
                        &[0, 255, 0, 255]
                    }
                );
            }
        }
    }
}

#[test]
fn native_multipass_load_blend_and_copy_preserve_premultiplied_linear_values() {
    let ex = executor();
    let content = source(|cx, _| {
        let d = ImageDescriptor::linear_p3_f16(extent(), AlphaMode::Premultiplied)?;
        let p = color_program(p3(AlphaMode::Premultiplied));
        let first = cx.render(
            RenderPass::fullscreen(&p, d.clone())
                .uniform("p", UniformValues::new().vec4("color", [0.25, 0., 0., 0.5])),
        )?;
        let second = cx.render(
            RenderPass::fullscreen(&p, d)
                .uniform("p", UniformValues::new().vec4("color", [0., 0.25, 0., 0.5]))
                .load(first)
                .blend(PassBlend::SourceOver),
        )?;
        cx.copy_image(second)
    });
    let result = frame(&ex, vec![content]);
    for y in 0..4 {
        for x in 0..4 {
            close(fp16(&result.images[0], x, y), [0.125, 0.25, 0., 0.75]);
        }
    }
    assert_eq!(result.stats.submissions, 3);
}

fn mip_bytes() -> Arc<[u8]> {
    let mut bytes = [255, 0, 0, 255].repeat(16);
    bytes.extend([0, 255, 0, 255].repeat(4));
    bytes.extend([0, 0, 255, 255]);
    bytes.into()
}
fn upload_mips(cx: &mut ContentBuilder) -> Result<ImageRef> {
    cx.upload_image(
        ImageDescriptor::new(extent(), PixelFormat::Rgba8, srgb(), 3)?,
        mip_bytes(),
    )
}
#[test]
fn native_mip_upload_sampling_and_each_readback_level_are_defined() {
    let ex = executor();
    let p = sample_program(
        "@group(0) @binding(0) var image: texture_2d<f32>; @group(0) @binding(1) var filtering: sampler; @fragment fn paint(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { return textureSampleLevel(image, filtering, uv, 1.0); }",
    );
    let sample = source(move |cx, _| {
        let image = upload_mips(cx)?;
        cx.render(
            RenderPass::fullscreen(&p, target())
                .image("image", image)
                .sampler(
                    "filtering",
                    Sampler {
                        mip_filter: Some(FilterMode::Nearest),
                        ..Sampler::nearest_clamp()
                    },
                ),
        )
    });
    let raw = source(|cx, _| upload_mips(cx));
    let f = frame(&ex, vec![sample, raw]);
    all_rgba8(&f.images[0], [0, 255, 0, 255]);
    assert_eq!(f.images[1].levels.len(), 3);
    for (level, color) in [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        .iter()
        .enumerate()
    {
        for pixel in f.images[1].levels[level].as_chunks::<4>().0 {
            assert_eq!(pixel, color);
        }
    }
}

const OUTPUT_IMAGES: &str = "@group(0) @binding(0) var first: texture_storage_2d<rgba8unorm, write>; @group(0) @binding(1) var second: texture_storage_2d<rgba8unorm, write>; @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) { textureStore(first, vec2<i32>(id.xy), vec4(1.0, 0.0, 0.0, 1.0)); textureStore(second, vec2<i32>(id.xy), vec4(0.0, 1.0, 0.0, 1.0)); }";
fn image_outputs(cx: &mut ContentBuilder) -> Result<ComputeOutputs> {
    let program = PortableProgram::compute(
        OUTPUT_IMAGES,
        "main",
        ImageContracts::new()
            .image("first", srgb())
            .image("second", srgb()),
    )?;
    cx.compute(
        ComputePass::new(&program, [4, 4, 1])
            .write_image("first", target())
            .write_image("second", target()),
    )
}
#[test]
fn native_multi_output_compute_dispatches_once_and_reads_distinct_results() {
    let ex = executor();
    let f = frame(
        &ex,
        vec![
            source(|cx, _| image_outputs(cx)?.image("first")),
            source(|cx, _| image_outputs(cx)?.image("second")),
        ],
    );
    assert_eq!(f.stats.operations, 1);
    assert_eq!(f.stats.submissions, 1);
    all_rgba8(&f.images[0], [255, 0, 0, 255]);
    all_rgba8(&f.images[1], [0, 255, 0, 255]);
}

const MESH_VERTEX: &str = "struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) color: vec4<f32> } @vertex fn vertex(@location(0) p: vec2<f32>, @location(1) c: vec4<f32>) -> VertexOut { var out: VertexOut; out.position = vec4(p, 0.0, 1.0); out.color = c; return out; } @fragment fn fragment(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> { return color; }";
fn buffer(len: u64, stride: u32, usage: BufferUsage) -> BufferDescriptor {
    BufferDescriptor::new(
        len,
        BufferLayout::new(stride, stride.min(16)).unwrap(),
        usage,
    )
    .unwrap()
}
fn mesh_outputs(
    cx: &mut ContentBuilder,
    indices: bool,
    invalid_indices: bool,
) -> Result<ComputeOutputs> {
    let extra = if indices {
        "@group(0) @binding(2) var<storage, read_write> indices: array<u32>;"
    } else {
        ""
    };
    let write = if indices {
        if invalid_indices {
            "indices[id.x] = 4294967295u;"
        } else {
            "indices[id.x] = id.x;"
        }
    } else {
        ""
    };
    let shader = format!(
        "@group(0) @binding(0) var<storage, read_write> positions: array<vec2<f32>>; @group(0) @binding(1) var<storage, read_write> colors: array<vec4<f32>>; {extra} @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{ if id.x < 3u {{ let p = array<vec2<f32>, 3>(vec2(-1.0, 1.0), vec2(1.0, 1.0), vec2(0.0, -1.0)); positions[id.x] = p[id.x]; colors[id.x] = vec4(1.0, 0.0, 0.0, 1.0); {write} }} }}"
    );
    let program = PortableProgram::compute(shader, "main", ImageContracts::new())?;
    let mut pass = ComputePass::new(&program, [3, 1, 1])
        .write_buffer(
            "positions",
            buffer(24, 8, BufferUsage::STORAGE | BufferUsage::VERTEX),
        )
        .write_buffer(
            "colors",
            buffer(48, 16, BufferUsage::STORAGE | BufferUsage::VERTEX),
        );
    if indices {
        pass = pass.write_buffer(
            "indices",
            buffer(12, 4, BufferUsage::STORAGE | BufferUsage::INDEX),
        );
    }
    cx.compute(pass)
}
fn mesh_pass(out: &ComputeOutputs) -> Result<RenderPass> {
    let p = PortableProgram::render(
        MESH_VERTEX,
        "vertex",
        "fragment",
        srgb(),
        ImageContracts::new(),
    )?;
    Ok(RenderPass::mesh(&p, target(), 3)
        .vertex_buffer(
            0,
            BufferSlice {
                buffer: out.buffer("positions")?,
                offset: 0,
                byte_len: 24,
            },
            VertexLayout {
                stride: 8,
                step: VertexStep::Vertex,
                attributes: vec![VertexAttribute {
                    location: 0,
                    offset: 0,
                    ty: ShaderValueType::Float(2),
                }],
            },
        )
        .vertex_buffer(
            1,
            BufferSlice {
                buffer: out.buffer("colors")?,
                offset: 0,
                byte_len: 48,
            },
            VertexLayout {
                stride: 16,
                step: VertexStep::Vertex,
                attributes: vec![VertexAttribute {
                    location: 1,
                    offset: 0,
                    ty: ShaderValueType::Float(4),
                }],
            },
        ))
}
fn red_triangle(image: &Dx11ContentImage) {
    assert_eq!(&image.levels[0][8..12], &[255, 0, 0, 255]);
    assert_eq!(&image.levels[0][48..52], &[0, 0, 0, 0]);
}
#[test]
fn native_compute_generated_mesh_and_indices_use_vertex_buffers() {
    let ex = executor();
    for (indices, invalid_indices) in [(false, false), (true, false), (true, true)] {
        let f = frame(
            &ex,
            vec![source(move |cx, _| {
                let out = mesh_outputs(cx, indices, invalid_indices)?;
                let mut pass = mesh_pass(&out)?;
                if indices {
                    pass = pass.index_buffer(
                        BufferSlice {
                            buffer: out.buffer("indices")?,
                            offset: 0,
                            byte_len: 12,
                        },
                        IndexFormat::Uint32,
                    );
                }
                cx.render(pass)
            })],
        );
        if invalid_indices {
            all_rgba8(&f.images[0], [0; 4]);
        } else {
            red_triangle(&f.images[0]);
        }
        assert_eq!(f.stats.operations, 2);
    }
}

#[test]
fn native_unaligned_buffer_copy_is_gpu_ordered_and_preserves_index_bytes() {
    let ex = executor();
    let f = frame(
        &ex,
        vec![source(|cx, _| {
            let out = mesh_outputs(cx, false, false)?;
            let initial = cx.upload_buffer(
                buffer(10, 2, BufferUsage::COPY_SOURCE),
                Arc::from([99, 99, 0, 0, 1, 0, 2, 0, 77, 77]),
            )?;
            let indices = cx.copy_buffer(
                BufferSlice {
                    buffer: initial,
                    offset: 2,
                    byte_len: 6,
                },
                buffer(6, 2, BufferUsage::COPY_DESTINATION | BufferUsage::INDEX),
            )?;
            cx.render(mesh_pass(&out)?.index_buffer(
                BufferSlice {
                    buffer: indices,
                    offset: 0,
                    byte_len: 6,
                },
                IndexFormat::Uint16,
            ))
        })],
    );
    red_triangle(&f.images[0]);
}

#[test]
fn native_budget_rejection_and_submitted_prefix_failure_leave_worker_reusable() {
    let ex = executor();
    let g = graph(
        &ex,
        vec![source(|cx, r| {
            let first = cx.solid(r.extent(), ColorValue::srgb([1., 0., 0., 1.])?)?;
            cx.copy_image(first)
        })],
    );
    let tiny = Dx11ContentLimits {
        max_gpu_bytes: 1,
        ..Default::default()
    };
    assert_eq!(
        ex.submit(g.clone(), tiny)
            .unwrap()
            .wait_for_readback(Duration::from_secs(20))
            .unwrap_err()
            .kind,
        ContentErrorKind::ResourceLimit
    );
    #[cfg(feature = "test-support")]
    assert_eq!(
        ex.submit(
            g.clone(),
            Dx11ContentLimits {
                fail_before_node: Some(1),
                ..Default::default()
            }
        )
        .unwrap()
        .wait_for_readback(Duration::from_secs(20))
        .unwrap_err()
        .kind,
        ContentErrorKind::ExecutionFailure
    );
    assert_eq!(
        ex.submit(
            g.clone(),
            Dx11ContentLimits {
                fail_after_record: Some(1),
                ..Default::default()
            }
        )
        .unwrap()
        .wait_for_readback(Duration::from_secs(20))
        .unwrap_err()
        .kind,
        ContentErrorKind::ExecutionFailure
    );
    let f = ex
        .submit(g, Dx11ContentLimits::default())
        .unwrap()
        .wait_for_readback(Duration::from_secs(20))
        .unwrap();
    all_rgba8(&f.images[0], [255, 0, 0, 255]);
}

#[test]
fn native_job_future_survives_dropping_the_executor_handle() {
    let ex = executor();
    let job = ex
        .submit(graph(&ex, vec![solid()]), Dx11ContentLimits::default())
        .unwrap();
    drop(ex);
    let result = job.wait_for_readback(Duration::from_secs(20)).unwrap();
    all_rgba8(&result.images[0], [64, 128, 255, 255]);
}

#[test]
fn native_dense_bounded_queue_finishes_in_a_subprocess() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "dense_worker", "--nocapture"])
        .env("XGPUI_CONTENT_DX11_CHILD", "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "dense native worker failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("dense native worker exceeded process deadline");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn native_r8_coverage_and_bgra_channel_order_are_preserved() {
    fn bgra(cx: &mut ContentBuilder) -> Result<ImageRef> {
        cx.upload_image(
            ImageDescriptor::new(ImageExtent::new(2, 1)?, PixelFormat::Bgra8, srgb(), 1)?,
            Arc::from([0, 0, 255, 255, 0, 255, 0, 255]),
        )
    }
    let ex = executor();
    let mask = source(|cx, _| {
        let interpretation = ImageInterpretation {
            color: ColorEncoding::Coverage,
            alpha: AlphaMode::Opaque,
        };
        let program = PortableProgram::fullscreen(
            "@fragment fn paint() -> @location(0) vec4<f32> { return vec4(0.5, 0.0, 0.0, 1.0); }",
            "paint",
            interpretation,
            ImageContracts::new(),
        )?;
        cx.render(RenderPass::fullscreen(
            &program,
            ImageDescriptor::new(extent(), PixelFormat::R8, interpretation, 1)?,
        ))
    });
    let sampled = source(|cx, _| {
        let image = bgra(cx)?;
        cx.render(
            RenderPass::fullscreen(&sample_program(SAMPLE), target())
                .image("image", image)
                .sampler("filtering", Sampler::nearest_clamp()),
        )
    });
    let f = frame(&ex, vec![mask, source(|cx, _| bgra(cx)), sampled]);
    assert!(f.images[0].levels[0].iter().all(|v| *v == 128));
    assert_eq!(f.images[1].descriptor.format(), PixelFormat::Bgra8);
    assert_eq!(&*f.images[1].levels[0], &[0, 0, 255, 255, 0, 255, 0, 255]);
    for row in f.images[2].levels[0].as_chunks::<16>().0 {
        assert_eq!(
            row,
            &[
                255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255
            ]
        );
    }
}

#[test]
fn native_vertex_bindings_clamp_to_declared_vertex_count_not_allocation_length() {
    let ex = executor();
    let content = source(|cx, _| {
        let shader = "@group(0) @binding(0) var<storage, read_write> positions: array<vec2<f32>>; @group(0) @binding(1) var<storage, read_write> colors: array<vec4<f32>>; @group(0) @binding(2) var<storage, read_write> indices: array<u32>; @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) { if id.x < 6u { let p = array<vec2<f32>, 3>(vec2(-1.0, 1.0), vec2(1.0, 1.0), vec2(0.0, -1.0)); positions[id.x] = p[id.x % 3u]; colors[id.x] = vec4(0.0, 1.0, 0.0, 1.0); } if id.x < 3u { indices[id.x] = id.x + 3u; } }";
        let p = PortableProgram::compute(shader, "main", ImageContracts::new())?;
        let out = cx.compute(
            ComputePass::new(&p, [6, 1, 1])
                .write_buffer(
                    "positions",
                    buffer(48, 8, BufferUsage::STORAGE | BufferUsage::VERTEX),
                )
                .write_buffer(
                    "colors",
                    buffer(96, 16, BufferUsage::STORAGE | BufferUsage::VERTEX),
                )
                .write_buffer(
                    "indices",
                    buffer(12, 4, BufferUsage::STORAGE | BufferUsage::INDEX),
                ),
        )?;
        let program = PortableProgram::render(
            MESH_VERTEX,
            "vertex",
            "fragment",
            srgb(),
            ImageContracts::new(),
        )?;
        cx.render(
            RenderPass::mesh(&program, target(), 3)
                .vertex_buffer(
                    0,
                    BufferSlice {
                        buffer: out.buffer("positions")?,
                        offset: 0,
                        byte_len: 48,
                    },
                    VertexLayout {
                        stride: 8,
                        step: VertexStep::Vertex,
                        attributes: vec![VertexAttribute {
                            location: 0,
                            offset: 0,
                            ty: ShaderValueType::Float(2),
                        }],
                    },
                )
                .vertex_buffer(
                    1,
                    BufferSlice {
                        buffer: out.buffer("colors")?,
                        offset: 0,
                        byte_len: 96,
                    },
                    VertexLayout {
                        stride: 16,
                        step: VertexStep::Vertex,
                        attributes: vec![VertexAttribute {
                            location: 1,
                            offset: 0,
                            ty: ShaderValueType::Float(4),
                        }],
                    },
                )
                .index_buffer(
                    BufferSlice {
                        buffer: out.buffer("indices")?,
                        offset: 0,
                        byte_len: 12,
                    },
                    IndexFormat::Uint32,
                ),
        )
    });
    // The allocation contains six valid vertices, but this draw only authorizes
    // three. Indices 3,4,5 must not reveal the extra green triangle.
    let f = frame(&ex, vec![content]);
    all_rgba8(&f.images[0], [0; 4]);
}

#[test]
fn native_workgroup_barriers_and_runtime_storage_lengths_reach_fragment_reads() {
    let ex = executor();
    let content = source(|cx, _| {
        let compute = PortableProgram::compute(
            "@group(0) @binding(0) var<storage, read_write> values: array<vec4<f32>>; var<workgroup> shared_values: array<f32, 4>; @compute @workgroup_size(4) fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32) { shared_values[local] = f32(local + 1u) / 4.0; workgroupBarrier(); if id.x < arrayLength(&values) { values[id.x] = vec4(shared_values[(local + 1u) % 4u], 0.0, 0.0, 1.0); } }",
            "main",
            ImageContracts::new(),
        )?;
        let output = cx.compute(
            ComputePass::new(&compute, [1; 3])
                .write_buffer("values", buffer(64, 16, BufferUsage::STORAGE)),
        )?;
        let render = PortableProgram::fullscreen(
            "@group(0) @binding(0) var<storage, read> values: array<vec4<f32>>; @fragment fn paint(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> { return values[u32(position.x)]; }",
            "paint",
            srgb(),
            ImageContracts::new(),
        )?;
        cx.render(RenderPass::fullscreen(&render, target()).storage_buffer(
            "values",
            BufferSlice {
                buffer: output.buffer("values")?,
                offset: 0,
                byte_len: 64,
            },
        ))
    });
    let f = frame(&ex, vec![content]);
    for row in f.images[0].levels[0].as_chunks::<16>().0 {
        assert_eq!(
            row,
            &[
                128, 0, 0, 255, 191, 0, 0, 255, 255, 0, 0, 255, 64, 0, 0, 255
            ]
        );
    }
}
#[test]
#[ignore = "subprocess entry; run native_dense_bounded_queue_finishes_in_a_subprocess"]
fn dense_worker() {
    assert_eq!(
        std::env::var("XGPUI_CONTENT_DX11_CHILD").as_deref(),
        Ok("1")
    );
    let ex = executor();
    for cycle in 0..3 {
        for count in [512usize, 1, 512] {
            let contents: Vec<_> = (0..count)
                .map(|i| {
                    Content::solid(ColorValue::linear_p3([i as f32 / 512., 0.5, 0.25, 1.]).unwrap())
                })
                .collect();
            let g = graph(&ex, contents);
            for replay in 0..2 {
                let f = ex
                    .submit(g.clone(), Dx11ContentLimits::default())
                    .unwrap()
                    .wait_for_readback(Duration::from_secs(20))
                    .unwrap();
                assert_eq!(f.stats.operations, count);
                assert_eq!(f.stats.submissions, count);
                assert_eq!(f.stats.max_in_flight, 1);
                for (i, image) in f.images.iter().enumerate() {
                    close(fp16(image, 0, 0), [i as f32 / 512., 0.5, 0.25, 1.]);
                }
                eprintln!(
                    "native dense cycle={cycle} count={count} replay={replay}: pixels verified, max_in_flight=1"
                );
            }
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
fn cancellation_and_bounded_request_queue_retire_submitted_prefix() {
    let ex = Dx11ContentExecutor::new(Dx11ContentOptions {
        queue_capacity: 1,
        pending_jobs: 1,
        ..Default::default()
    })
    .unwrap();
    let g = graph(
        &ex,
        vec![source(|cx, r| {
            let a = cx.solid(r.extent(), ColorValue::srgb([1., 0., 0., 1.])?)?;
            cx.copy_image(a)
        })],
    );
    let gate = Arc::new(Dx11ContentTestGate::default());
    struct Release(Arc<Dx11ContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    let _release = Release(gate.clone());
    let job = ex
        .submit(
            g.clone(),
            Dx11ContentLimits {
                pause_before_node: Some((1, gate.clone())),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(gate.wait_until_entered(Duration::from_secs(20)));
    let queued = ex.submit(g.clone(), Default::default()).unwrap();
    assert_eq!(
        ex.submit(g.clone(), Default::default()).err().unwrap().kind,
        ContentErrorKind::ResourceLimit
    );
    drop(job);
    gate.release();
    all_rgba8(
        &queued
            .wait_for_readback(Duration::from_secs(20))
            .unwrap()
            .images[0],
        [255, 0, 0, 255],
    );
    assert_eq!(ex.retained_gpu_bytes(), 0);
    all_rgba8(&frame(&ex, vec![solid()]).images[0], [64, 128, 255, 255]);
}

#[test]
fn native_nonzero_vertex_and_instance_builtins_match_portable_ranges() {
    let ex = executor();
    let f = frame(
        &ex,
        vec![source(|cx, _| {
            let p = PortableProgram::render(
                "@vertex fn vertex(@builtin(vertex_index)id:u32,@builtin(instance_index)instance:u32)->@builtin(position)vec4<f32>{let positions=array<vec2<f32>,3>(vec2(-1.,1.),vec2(3.,1.),vec2(-1.,-3.));if id<3u||instance!=2u{return vec4(0.,0.,0.,1.);}return vec4(positions[id-3u],0.,1.);}@fragment fn fragment()->@location(0)vec4<f32>{return vec4(1.,0.,0.,1.);}",
                "vertex",
                "fragment",
                srgb(),
                ImageContracts::new(),
            )?;
            cx.render(RenderPass::mesh(&p, target(), 6).draw(3..6, 2..3))
        })],
    );
    all_rgba8(&f.images[0], [255, 0, 0, 255]);
}
#[test]
fn native_num_workgroups_reports_dispatch_dimensions() {
    let ex = executor();
    let f = frame(
        &ex,
        vec![source(|cx, _| {
            let p = PortableProgram::compute(
                "@group(0)@binding(0)var<storage,read_write> values:array<vec4<f32>>;@compute @workgroup_size(1)fn main(@builtin(global_invocation_id)id:vec3<u32>,@builtin(num_workgroups)count:vec3<u32>){if all(id==vec3(0u)){values[0]=vec4(vec3<f32>(count)/4.,1.);}}",
                "main",
                ImageContracts::new(),
            )?;
            let out = cx.compute(
                ComputePass::new(&p, [3, 2, 1])
                    .write_buffer("values", buffer(16, 16, BufferUsage::STORAGE)),
            )?;
            let p = PortableProgram::fullscreen(
                "@group(0)@binding(0)var<storage,read> values:array<vec4<f32>>;@fragment fn paint()->@location(0)vec4<f32>{return values[0];}",
                "paint",
                srgb(),
                ImageContracts::new(),
            )?;
            cx.render(RenderPass::fullscreen(&p, target()).storage_buffer(
                "values",
                BufferSlice {
                    buffer: out.buffer("values")?,
                    offset: 0,
                    byte_len: 16,
                },
            ))
        })],
    );
    all_rgba8(&f.images[0], [191, 128, 64, 255]);
}
