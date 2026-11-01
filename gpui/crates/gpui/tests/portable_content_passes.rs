//! Pass graph validation without native GPU execution.
use gpui::{content::*, px, size};
use std::sync::{Arc, Mutex};
type Result<T> = std::result::Result<T, ContentError>;
type Describe = dyn Fn(&mut ContentBuilder, &ContentRequest) -> Result<ImageRef> + Send + Sync;
struct Source(Box<Describe>);
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PassSource")
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
fn target() -> ImageDescriptor {
    ImageDescriptor::new(extent(), PixelFormat::Rgba8, srgb(), 1).unwrap()
}
fn request() -> ContentRequest {
    ContentRequest::new(size(px(4.), px(4.)), extent(), 1.).unwrap()
}
fn red() -> ColorValue {
    ColorValue::srgb([1., 0., 0., 1.]).unwrap()
}
fn caps() -> ContentCapabilities {
    ContentCapabilities {
        compute: Some(ComputeLimits {
            max_workgroup_size: [64; 3],
            max_workgroup_invocations: 256,
            max_dispatch: [65535; 3],
            max_workgroup_memory_bytes: 16384,
        }),
        storage_image_formats: vec![PixelFormat::Rgba8, PixelFormat::Rgba16Float],
        ..ContentCapabilities::default()
    }
}
fn describe(content: Content, capabilities: ContentCapabilities) -> Result<ContentGraph> {
    ContentGraph::describe(
        &[ContentRoot {
            content,
            request: request(),
        }],
        capabilities,
        GraphLimits::default(),
    )
}
fn roots(contents: Vec<Content>) -> Vec<ContentRoot> {
    contents
        .into_iter()
        .map(|content| ContentRoot {
            content,
            request: request(),
        })
        .collect()
}
fn plan(graph: Arc<ContentGraph>, bytes: u64) -> Result<ExecutionPlan> {
    let charges = graph.nodes().iter().map(ContentNode::byte_len).collect();
    ExecutionPlan::new(
        graph,
        charges,
        ExecutionLimits {
            max_live_bytes: bytes,
            max_recording_nodes: 1,
        },
    )
}
const COLOR: &str = r#"
struct Parameters { color: vec4<f32> }
@group(0) @binding(0) var<uniform> parameters: Parameters;
@fragment fn paint() -> @location(0) vec4<f32> { return parameters.color; }
"#;
const SAMPLE: &str = r#"
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@fragment fn paint(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { return textureSample(image, filtering, uv); }
"#;
fn color_program() -> PortableProgram {
    PortableProgram::fullscreen(COLOR, "paint", srgb(), ImageContracts::new()).unwrap()
}
fn sample_program() -> PortableProgram {
    PortableProgram::fullscreen(
        SAMPLE,
        "paint",
        srgb(),
        ImageContracts::new().image("image", srgb()),
    )
    .unwrap()
}
fn buffer(byte_len: u64, stride: u32, usage: BufferUsage) -> BufferDescriptor {
    BufferDescriptor::new(
        byte_len,
        BufferLayout::new(stride, stride.min(16)).unwrap(),
        usage,
    )
    .unwrap()
}
const IMAGE_OUTPUTS: &str = r#"
@group(0) @binding(0) var first: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var second: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    textureStore(first, vec2<i32>(id.xy), vec4(1.0, 0.0, 0.0, 1.0));
    textureStore(second, vec2<i32>(id.xy), vec4(0.0, 1.0, 0.0, 1.0));
}
"#;
fn image_program() -> PortableProgram {
    PortableProgram::compute(
        IMAGE_OUTPUTS,
        "main",
        ImageContracts::new()
            .image("first", srgb())
            .image("second", srgb()),
    )
    .unwrap()
}
fn compute_images(cx: &mut ContentBuilder) -> Result<ComputeOutputs> {
    cx.compute(
        ComputePass::new(&image_program(), [4, 4, 1])
            .write_image("first", target())
            .write_image("second", target()),
    )
}
const MESH_COMPUTE: &str = r#"
@group(0) @binding(0) var<storage, read_write> positions: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> colors: array<vec4<f32>>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < 3u { let p = array<vec2<f32>, 3>(vec2(-1.0, 1.0), vec2(1.0, 1.0), vec2(0.0, -1.0));
        positions[id.x] = p[id.x]; colors[id.x] = vec4(1.0, 0.0, 0.0, 1.0); }
}
"#;
const MESH_RENDER: &str = r#"
struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) color: vec4<f32> }
@vertex fn vertex(@location(0) p: vec2<f32>, @location(1) c: vec4<f32>) -> VertexOut {
    var out: VertexOut; out.position = vec4(p, 0.0, 1.0); out.color = c; return out;
}
@fragment fn fragment(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> { return color; }
"#;
fn mesh_program() -> PortableProgram {
    PortableProgram::render(
        MESH_RENDER,
        "vertex",
        "fragment",
        srgb(),
        ImageContracts::new(),
    )
    .unwrap()
}
fn compute_mesh(cx: &mut ContentBuilder) -> Result<ComputeOutputs> {
    let program = PortableProgram::compute(MESH_COMPUTE, "main", ImageContracts::new())?;
    cx.compute(
        ComputePass::new(&program, [3, 1, 1])
            .write_buffer(
                "positions",
                buffer(24, 8, BufferUsage::STORAGE | BufferUsage::VERTEX),
            )
            .write_buffer(
                "colors",
                buffer(48, 16, BufferUsage::STORAGE | BufferUsage::VERTEX),
            ),
    )
}
fn mesh_pass(positions: BufferRef, colors: BufferRef) -> RenderPass {
    RenderPass::mesh(&mesh_program(), target(), 3)
        .vertex_buffer(
            0,
            BufferSlice {
                buffer: positions,
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
                buffer: colors,
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
        )
}

#[test]
fn uniform_render_snapshots_deduplicate_and_count_payload_once() {
    let content = |value| {
        source(move |cx, _| {
            cx.render(RenderPass::fullscreen(&color_program(), target()).uniform(
                "parameters",
                UniformValues::new().vec4("color", [value, 0., 0., 1.]),
            ))
        })
    };
    let g = ContentGraph::describe(
        &roots(vec![content(0.), content(-0.), content(1.)]),
        caps(),
        GraphLimits::default(),
    )
    .unwrap();
    assert_eq!(g.nodes().len(), 2);
    assert_eq!(g.payload_bytes(), 32);
    assert_eq!(g.roots()[0], g.roots()[1]);
    assert_ne!(g.roots()[0], g.roots()[2]);
    let ContentOperation::Render(pass) = g.nodes()[0].operation() else {
        panic!()
    };
    assert!(
        matches!(pass.bindings()[0].resource(), BoundResource::Uniform(bytes) if bytes.len() == 16)
    );
    let limits = GraphLimits {
        max_payload_bytes: 15,
        ..GraphLimits::default()
    };
    assert_eq!(
        ContentGraph::describe(&roots(vec![content(1.)]), caps(), limits)
            .unwrap_err()
            .kind,
        ContentErrorKind::ResourceLimit
    );
}

#[test]
fn multipass_sampling_and_load_share_a_dependency_without_writing_the_input() {
    let content = source(|cx, _| {
        let image = cx.solid(extent(), red())?;
        let first = cx.render(
            RenderPass::fullscreen(&sample_program(), target())
                .image("image", image)
                .sampler("filtering", Sampler::linear_clamp())
                .load(image),
        )?;
        cx.render(
            RenderPass::fullscreen(&sample_program(), target())
                .image("image", first)
                .sampler("filtering", Sampler::linear_clamp()),
        )
    });
    let g = describe(content, caps()).unwrap();
    assert_eq!(g.nodes().len(), 3);
    assert_eq!(g.nodes()[1].dependencies(), &[0]);
    assert_eq!(g.nodes()[2].dependencies(), &[1]);
    let ContentOperation::Render(pass) = g.nodes()[1].operation() else {
        panic!()
    };
    assert!(matches!(pass.load(), ResolvedLoad::Load(id) if id.node() == 0));
    let p = plan(Arc::new(g), 128).unwrap();
    assert_eq!(p.peak_bytes(), 128);
}

#[test]
fn source_sized_image_binding_does_not_require_destination_extent() {
    let content = source(|cx, _| {
        let d = ImageDescriptor::new(ImageExtent::new(2, 1)?, PixelFormat::Rgba8, srgb(), 1)?;
        let image = cx.upload_image(d, Arc::from([255; 8]))?;
        cx.render(
            RenderPass::fullscreen(&sample_program(), target())
                .image("image", image)
                .sampler("filtering", Sampler::linear_clamp()),
        )
    });
    let g = describe(content, caps()).unwrap();
    assert_eq!(g.nodes()[0].byte_len(), 8);
    assert_eq!(g.nodes()[1].byte_len(), 64);
}

#[test]
fn one_compute_dispatch_produces_distinct_named_outputs_and_root_selectors() {
    let first = source(|cx, _| compute_images(cx)?.image("first"));
    let second = source(|cx, _| compute_images(cx)?.image("second"));
    let g = ContentGraph::describe(&roots(vec![first, second]), caps(), GraphLimits::default())
        .unwrap();
    assert_eq!(g.nodes().len(), 1);
    assert_eq!(g.nodes()[0].outputs().len(), 2);
    assert_eq!(g.nodes()[0].byte_len(), 128);
    assert_eq!((g.roots()[0].node(), g.roots()[0].output()), (0, 0));
    assert_eq!((g.roots()[1].node(), g.roots()[1].output()), (0, 1));
    assert_ne!(g.nodes()[0].output_key(0), g.nodes()[0].output_key(1));
    assert!(g.nodes()[0].output_key(2).is_none());
    let ContentOperation::Compute(pass) = g.nodes()[0].operation() else {
        panic!()
    };
    assert_eq!(pass.dispatch(), [4, 4, 1]);
    assert!(matches!(
        pass.bindings()[1].resource(),
        BoundResource::Output {
            output: 1,
            initial: None
        }
    ));
}

#[test]
fn same_descriptor_outputs_cannot_alias_when_used_as_copy_inputs() {
    let first = source(|cx, _| {
        let image = compute_images(cx)?.image("first")?;
        cx.copy_image(image)
    });
    let second = source(|cx, _| {
        let image = compute_images(cx)?.image("second")?;
        cx.copy_image(image)
    });
    let g = ContentGraph::describe(&roots(vec![first, second]), caps(), GraphLimits::default())
        .unwrap();
    assert_eq!(g.nodes().len(), 3);
    assert_ne!(g.nodes()[1].operation_key(), g.nodes()[2].operation_key());
    let ContentOperation::CopyImage { source } = g.nodes()[2].operation() else {
        panic!()
    };
    assert_eq!((source.node(), source.output()), (0, 1));
}

#[test]
fn compute_to_mesh_has_one_producer_and_two_typed_vertex_streams() {
    let content = source(|cx, _| {
        let out = compute_mesh(cx)?;
        cx.render(mesh_pass(out.buffer("positions")?, out.buffer("colors")?))
    });
    let g = describe(content, caps()).unwrap();
    assert_eq!(g.nodes().len(), 2);
    assert_eq!(g.nodes()[0].byte_len(), 72);
    assert_eq!(g.nodes()[1].dependencies(), &[0]);
    let ContentOperation::Render(pass) = g.nodes()[1].operation() else {
        panic!()
    };
    let ResolvedGeometry::Mesh { vertices, .. } = pass.geometry() else {
        panic!()
    };
    assert_eq!(vertices.len(), 2);
    assert_eq!(vertices[0].buffer.source.output(), 0);
    assert_eq!(vertices[1].buffer.source.output(), 1);
    assert_eq!(plan(Arc::new(g), 136).unwrap().peak_bytes(), 136);
}

#[test]
fn graph_pruning_remaps_pass_bindings_geometry_and_selected_outputs() {
    let make = |dead| {
        source(move |cx, _| {
            if dead {
                cx.solid(extent(), ColorValue::srgb([0., 1., 0., 1.])?)?;
            }
            let out = compute_mesh(cx)?;
            cx.render(mesh_pass(out.buffer("positions")?, out.buffer("colors")?))
        })
    };
    let a = describe(make(false), caps()).unwrap();
    let b = describe(make(true), caps()).unwrap();
    assert_eq!(a.nodes().len(), 2);
    assert_eq!(b.nodes().len(), 2);
    assert_eq!(a.nodes()[1].operation_key(), b.nodes()[1].operation_key());
    let ContentOperation::Render(pass) = b.nodes()[1].operation() else {
        panic!()
    };
    let ResolvedGeometry::Mesh { vertices, .. } = pass.geometry() else {
        panic!()
    };
    assert_eq!(vertices[1].buffer.source.node(), 0);
}

#[test]
fn multi_output_working_set_is_charged_and_admitted_atomically() {
    let g =
        Arc::new(describe(source(|cx, _| compute_images(cx)?.image("second")), caps()).unwrap());
    assert_eq!(
        ExecutionPlan::new(
            g.clone(),
            vec![64],
            ExecutionLimits {
                max_live_bytes: 128,
                max_recording_nodes: 1
            }
        )
        .unwrap_err()
        .kind,
        ContentErrorKind::InvalidDescriptor
    );
    assert_eq!(
        plan(g.clone(), 127).unwrap_err().kind,
        ContentErrorKind::ResourceLimit
    );
    let mut tracker = plan(g, 128).unwrap().start();
    assert_eq!(tracker.try_admit(0).unwrap(), Admission::Admitted);
    assert_eq!(tracker.live_bytes(), 128);
    let completion = tracker.submitted(0).unwrap();
    drop(completion);
    tracker.completed(0, true).unwrap();
    assert_eq!(tracker.live_bytes(), 128);
    tracker.release_roots();
    assert_eq!(tracker.live_bytes(), 0);
}

#[test]
fn compute_lookup_checks_names_and_output_kinds() {
    describe(
        source(|cx, _| {
            let out = compute_images(cx)?;
            assert!(out.buffer("first").is_err());
            assert!(out.image("missing").is_err());
            out.image("second")
        }),
        caps(),
    )
    .unwrap();
}

#[test]
fn compute_requires_capability_and_fresh_writable_bindings() {
    let c = source(|cx, _| compute_images(cx)?.image("first"));
    assert_eq!(
        describe(c, ContentCapabilities::default())
            .unwrap_err()
            .kind,
        ContentErrorKind::UnsupportedCapability
    );
    let program = PortableProgram::compute("@group(0) @binding(0) var<storage, read_write> data: array<u32>; @compute @workgroup_size(1) fn main() { data[0] = 1u; }", "main", ImageContracts::new()).unwrap();
    let c = source(move |cx, _| {
        let b = cx.zeroed_buffer(buffer(16, 4, BufferUsage::STORAGE))?;
        cx.compute(ComputePass::new(&program, [1; 3]).storage_buffer(
            "data",
            BufferSlice {
                buffer: b,
                offset: 0,
                byte_len: 16,
            },
        ))?;
        cx.solid(extent(), red())
    });
    assert_eq!(
        describe(c, caps()).unwrap_err().kind,
        ContentErrorKind::InvalidDescriptor
    );
}

#[test]
fn initial_buffer_copy_can_use_copy_source_without_storage_binding_alignment() {
    let content = source(|cx, _| {
        let initial =
            cx.upload_buffer(buffer(40, 8, BufferUsage::COPY_SOURCE), Arc::from([0; 40]))?;
        let program = PortableProgram::compute(MESH_COMPUTE, "main", ImageContracts::new())?;
        let out = cx.compute(
            ComputePass::new(&program, [3, 1, 1])
                .read_modify_write_buffer(
                    "positions",
                    buffer(24, 8, BufferUsage::STORAGE | BufferUsage::VERTEX),
                    BufferSlice {
                        buffer: initial,
                        offset: 8,
                        byte_len: 24,
                    },
                )
                .write_buffer(
                    "colors",
                    buffer(48, 16, BufferUsage::STORAGE | BufferUsage::VERTEX),
                ),
        )?;
        cx.render(mesh_pass(out.buffer("positions")?, out.buffer("colors")?))
    });
    let g = describe(content, caps()).unwrap();
    assert_eq!(g.nodes().len(), 3);
    assert_eq!(g.nodes()[1].dependencies(), &[0]);
    let ContentOperation::Compute(pass) = g.nodes()[1].operation() else {
        panic!()
    };
    assert!(
        matches!(pass.bindings()[0].resource(), BoundResource::Output { initial: Some(InitialResource::Buffer(b)), .. } if b.offset == 8)
    );
}

#[test]
fn storage_input_binding_checks_reflected_size_usage_and_executor_alignment() {
    let program = PortableProgram::fullscreen("@group(0) @binding(0) var<storage, read> colors: array<vec4<f32>>; @fragment fn paint() -> @location(0) vec4<f32> { return colors[0]; }", "paint", srgb(), ImageContracts::new()).unwrap();
    let c = source(move |cx, _| {
        let b = cx.zeroed_buffer(buffer(64, 16, BufferUsage::STORAGE))?;
        cx.render(RenderPass::fullscreen(&program, target()).storage_buffer(
            "colors",
            BufferSlice {
                buffer: b,
                offset: 16,
                byte_len: 16,
            },
        ))
    });
    assert_eq!(
        describe(c.clone(), caps()).unwrap_err().kind,
        ContentErrorKind::UnsupportedCapability
    );
    let mut supported = caps();
    supported.bindings.storage_offset_alignment = 16;
    let g = describe(c, supported).unwrap();
    assert_eq!(g.nodes()[1].dependencies(), &[0]);
}

#[test]
fn invalid_binding_errors_poison_even_if_a_producer_catches_them() {
    let c = source(|cx, _| {
        let pass = RenderPass::fullscreen(&color_program(), target())
            .uniform("parameters", UniformValues::new().vec3("color", [1.; 3]));
        assert!(cx.render(pass).is_err());
        cx.solid(extent(), red())
    });
    assert_eq!(
        describe(c, caps()).unwrap_err().kind,
        ContentErrorKind::InvalidDescriptor
    );
}

#[test]
fn missing_unknown_duplicate_and_wrong_kind_bindings_are_rejected() {
    for case in 0..4 {
        let c = source(move |cx, _| {
            let mut pass = RenderPass::fullscreen(&color_program(), target());
            match case {
                0 => {}
                1 => pass = pass.uniform("unknown", UniformValues::new().vec4("color", [1.; 4])),
                2 => {
                    pass = pass
                        .uniform("parameters", UniformValues::new())
                        .uniform("parameters", UniformValues::new())
                }
                _ => pass = pass.sampler("parameters", Sampler::nearest_clamp()),
            }
            cx.render(pass)
        });
        assert_eq!(
            describe(c, caps()).unwrap_err().kind,
            ContentErrorKind::InvalidDescriptor
        );
    }
}

#[test]
fn sampled_image_contract_must_match_without_implicit_color_conversion() {
    let c = source(|cx, _| {
        let image = cx.solid(extent(), ColorValue::linear_p3([1., 0., 0., 1.])?)?;
        cx.render(
            RenderPass::fullscreen(&sample_program(), target())
                .image("image", image)
                .sampler("filtering", Sampler::nearest_clamp()),
        )
    });
    assert_eq!(
        describe(c, caps()).unwrap_err().kind,
        ContentErrorKind::InvalidDescriptor
    );
}

#[test]
fn reflected_sampler_pairs_avoid_unrelated_filterability_rejections() {
    let shader = r#"
        @group(0) @binding(0) var a: texture_2d<f32>; @group(0) @binding(1) var sa: sampler;
        @group(0) @binding(2) var b: texture_2d<f32>; @group(0) @binding(3) var sb: sampler;
        @fragment fn paint(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { return textureSample(a, sa, uv) + textureSample(b, sb, uv); }
    "#;
    let program = PortableProgram::fullscreen(
        shader,
        "paint",
        srgb(),
        ImageContracts::new().image("a", srgb()).image("b", srgb()),
    )
    .unwrap();
    let make = |linear| {
        let program = program.clone();
        source(move |cx, _| {
            let a = cx.solid(extent(), red())?;
            let b = cx.upload_image(
                ImageDescriptor::new(extent(), PixelFormat::Bgra8, srgb(), 1)?,
                Arc::from([0; 64]),
            )?;
            cx.render(
                RenderPass::fullscreen(&program, target())
                    .image("a", a)
                    .sampler("sa", Sampler::linear_clamp())
                    .image("b", b)
                    .sampler(
                        "sb",
                        if linear {
                            Sampler::linear_clamp()
                        } else {
                            Sampler::nearest_clamp()
                        },
                    ),
            )
        })
    };
    let mut limited = caps();
    limited.filterable_image_formats = vec![PixelFormat::Rgba8];
    describe(make(false), limited.clone()).unwrap();
    assert_eq!(
        describe(make(true), limited).unwrap_err().kind,
        ContentErrorKind::UnsupportedCapability
    );
}

#[test]
fn mip_filtering_requires_defined_levels_but_accepts_a_complete_one_pixel_chain() {
    let make = |mips| {
        source(move |cx, _| {
            let d = ImageDescriptor::new(extent(), PixelFormat::Rgba8, srgb(), mips)?;
            let bytes: Arc<[u8]> = vec![0; d.byte_len() as usize].into();
            let image = cx.upload_image(d, bytes)?;
            let sampler = Sampler {
                mip_filter: Some(FilterMode::Linear),
                ..Sampler::linear_clamp()
            };
            cx.render(
                RenderPass::fullscreen(&sample_program(), target())
                    .image("image", image)
                    .sampler("filtering", sampler),
            )
        })
    };
    assert_eq!(
        describe(make(1), caps()).unwrap_err().kind,
        ContentErrorKind::InvalidDescriptor
    );
    describe(make(3), caps()).unwrap();
    describe(
        source(|cx, _| {
            let image = cx.solid(ImageExtent::new(1, 1)?, red())?;
            let sampler = Sampler {
                mip_filter: Some(FilterMode::Nearest),
                ..Sampler::nearest_clamp()
            };
            cx.render(
                RenderPass::fullscreen(&sample_program(), target())
                    .image("image", image)
                    .sampler("filtering", sampler),
            )
        }),
        caps(),
    )
    .unwrap();
}

#[test]
fn render_target_load_blend_mip_and_capability_contracts_are_checked() {
    for case in 0..4 {
        let c = source(move |cx, _| {
            let p = color_program();
            let values = UniformValues::new().vec4("color", [1.; 4]);
            let mut pass = RenderPass::fullscreen(
                &p,
                if case == 0 {
                    ImageDescriptor::new(extent(), PixelFormat::Rgba8, srgb(), 2)?
                } else {
                    target()
                },
            )
            .uniform("parameters", values);
            if case == 1 {
                pass = pass.blend(PassBlend::SourceOver);
            }
            if case == 2 {
                let small = cx.solid(ImageExtent::new(1, 1)?, red())?;
                pass = pass.load(small);
            }
            if case == 3 {
                pass = pass.clear(ColorValue::linear_p3([1.; 4])?);
            }
            cx.render(pass)
        });
        assert_eq!(
            describe(c, caps()).unwrap_err().kind,
            ContentErrorKind::InvalidDescriptor
        );
    }
    let mut limited = caps();
    limited.renderable_image_formats.clear();
    let c = source(|cx, _| {
        cx.render(
            RenderPass::fullscreen(&color_program(), target())
                .uniform("parameters", UniformValues::new().vec4("color", [1.; 4])),
        )
    });
    assert_eq!(
        describe(c, limited).unwrap_err().kind,
        ContentErrorKind::UnsupportedCapability
    );
}

#[test]
fn foreign_image_and_buffer_refs_cannot_enter_passes() {
    let saved = Arc::new(Mutex::new(None));
    let slot = saved.clone();
    describe(
        source(move |cx, _| {
            let image = cx.solid(extent(), red())?;
            *slot.lock().unwrap() = Some(image);
            Ok(image)
        }),
        caps(),
    )
    .unwrap();
    let image = saved.lock().unwrap().unwrap();
    let c = source(move |cx, _| {
        cx.render(
            RenderPass::fullscreen(&sample_program(), target())
                .image("image", image)
                .sampler("filtering", Sampler::linear_clamp()),
        )
    });
    assert_eq!(
        describe(c, caps()).unwrap_err().kind,
        ContentErrorKind::InvalidReference
    );
    let saved = Arc::new(Mutex::new(None));
    let slot = saved.clone();
    describe(
        source(move |cx, _| {
            let b = cx.zeroed_buffer(buffer(24, 8, BufferUsage::VERTEX))?;
            *slot.lock().unwrap() = Some(b);
            cx.solid(extent(), red())
        }),
        caps(),
    )
    .unwrap();
    let foreign = saved.lock().unwrap().unwrap();
    let c = source(move |cx, _| {
        let out = compute_mesh(cx)?;
        cx.render(mesh_pass(foreign, out.buffer("colors")?))
    });
    assert_eq!(
        describe(c, caps()).unwrap_err().kind,
        ContentErrorKind::InvalidReference
    );
}

#[test]
fn mesh_attribute_type_range_and_slot_errors_are_rejected_before_execution() {
    for case in 0..5 {
        let c = source(move |cx, _| {
            let out = compute_mesh(cx)?;
            let p = out.buffer("positions")?;
            let colors = out.buffer("colors")?;
            let pass = match case {
                0 => RenderPass::mesh(&mesh_program(), target(), 3), // Missing attributes.
                1 => mesh_pass(p, colors).draw(0..6, 0..1),
                2 => mesh_pass(p, colors).vertex_buffer(
                    0,
                    BufferSlice {
                        buffer: p,
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
                ),
                3 => RenderPass::mesh(&mesh_program(), target(), 3)
                    .vertex_buffer(
                        0,
                        BufferSlice {
                            buffer: p,
                            offset: 0,
                            byte_len: 24,
                        },
                        VertexLayout {
                            stride: 8,
                            step: VertexStep::Vertex,
                            attributes: vec![VertexAttribute {
                                location: 0,
                                offset: 0,
                                ty: ShaderValueType::Uint(2),
                            }],
                        },
                    )
                    .vertex_buffer(
                        1,
                        BufferSlice {
                            buffer: colors,
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
                    ),
                _ => mesh_pass(p, colors).draw(0..3, 1..1),
            };
            cx.render(pass)
        });
        assert_eq!(
            describe(c, caps()).unwrap_err().kind,
            ContentErrorKind::InvalidDescriptor
        );
    }
}

#[test]
fn indexed_mesh_validates_index_width_and_bound_range() {
    for case in 0..3 {
        let c = source(move |cx, _| {
            let out = compute_mesh(cx)?;
            let index = cx.upload_buffer(
                buffer(6, 2, BufferUsage::INDEX),
                Arc::from([0, 0, 1, 0, 2, 0]),
            )?;
            let pass = mesh_pass(out.buffer("positions")?, out.buffer("colors")?)
                .index_buffer(
                    BufferSlice {
                        buffer: index,
                        offset: 0,
                        byte_len: 6,
                    },
                    if case == 2 {
                        IndexFormat::Uint32
                    } else {
                        IndexFormat::Uint16
                    },
                )
                .draw(if case == 1 { 0..6 } else { 0..3 }, 0..1);
            cx.render(pass)
        });
        if case == 0 {
            describe(c, caps()).unwrap();
        } else {
            assert_eq!(
                describe(c, caps()).unwrap_err().kind,
                ContentErrorKind::InvalidDescriptor
            );
        }
    }
}

#[test]
fn executor_binding_limits_are_checked_independently_of_program_validation() {
    let c = source(|cx, _| {
        cx.render(
            RenderPass::fullscreen(&color_program(), target())
                .uniform("parameters", UniformValues::new().vec4("color", [1.; 4])),
        )
    });
    for case in 0..3 {
        let mut limited = caps();
        match case {
            0 => limited.bindings.max_bindings = 0,
            1 => limited.bindings.max_uniform_bytes = 15,
            _ => limited.bindings.max_bind_groups = 0,
        }
        assert_eq!(
            describe(c.clone(), limited).unwrap_err().kind,
            ContentErrorKind::UnsupportedCapability
        );
    }
}

#[test]
fn interleaved_attribute_order_does_not_change_structural_identity() {
    let make = |reverse| {
        source(move |cx, _| {
            let b = cx.upload_buffer(
                BufferDescriptor::new(72, BufferLayout::new(24, 8)?, BufferUsage::VERTEX)?,
                Arc::from([0; 72]),
            )?;
            let mut attributes = vec![
                VertexAttribute {
                    location: 0,
                    offset: 0,
                    ty: ShaderValueType::Float(2),
                },
                VertexAttribute {
                    location: 1,
                    offset: 8,
                    ty: ShaderValueType::Float(4),
                },
            ];
            if reverse {
                attributes.reverse();
            }
            cx.render(
                RenderPass::mesh(&mesh_program(), target(), 3).vertex_buffer(
                    0,
                    BufferSlice {
                        buffer: b,
                        offset: 0,
                        byte_len: 72,
                    },
                    VertexLayout {
                        stride: 24,
                        step: VertexStep::Vertex,
                        attributes,
                    },
                ),
            )
        })
    };
    let g = ContentGraph::describe(
        &roots(vec![make(false), make(true)]),
        caps(),
        GraphLimits::default(),
    )
    .unwrap();
    assert_eq!(g.nodes().len(), 2);
    assert_eq!(g.roots()[0], g.roots()[1]);
}

#[test]
fn explicit_linear_premultiplied_source_over_is_supported() {
    let interpretation = ImageInterpretation {
        color: ColorEncoding::LinearP3,
        alpha: AlphaMode::Premultiplied,
    };
    let p = PortableProgram::fullscreen(
        "@fragment fn paint() -> @location(0) vec4<f32> { return vec4(0.5, 0.0, 0.0, 0.5); }",
        "paint",
        interpretation,
        ImageContracts::new(),
    )
    .unwrap();
    let g = describe(
        source(move |cx, _| {
            cx.render(
                RenderPass::fullscreen(
                    &p,
                    ImageDescriptor::linear_p3_f16(extent(), AlphaMode::Premultiplied)?,
                )
                .blend(PassBlend::SourceOver)
                .clear(ColorValue::new([0., 0., 0., 0.], interpretation)?),
            )
        }),
        caps(),
    )
    .unwrap();
    let ContentOperation::Render(pass) = g.nodes()[0].operation() else {
        panic!()
    };
    assert_eq!(pass.blend(), PassBlend::SourceOver);
}

#[test]
fn compute_initial_image_copy_survives_graph_pruning_and_output_selection() {
    let g = describe(
        source(|cx, _| {
            cx.solid(extent(), ColorValue::srgb([0., 0., 1., 1.])?)?; // Intentionally unused.
            let initial = cx.solid(extent(), red())?;
            let outputs = cx.compute(
                ComputePass::new(&image_program(), [4, 4, 1])
                    .write_image("first", target())
                    .read_modify_write_image("second", target(), initial),
            )?;
            outputs.image("second")
        }),
        caps(),
    )
    .unwrap();
    assert_eq!(g.nodes().len(), 2);
    assert_eq!((g.roots()[0].node(), g.roots()[0].output()), (1, 1));
    let ContentOperation::Compute(pass) = g.nodes()[1].operation() else {
        panic!()
    };
    assert!(
        matches!(pass.bindings()[1].resource(), BoundResource::Output { output: 1, initial: Some(InitialResource::Image(id)) } if id.node() == 0 && id.output() == 0)
    );
}

#[test]
fn multi_output_owner_leases_survive_until_all_submitted_readers_finish() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Owner(Arc<AtomicUsize>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let first = source(|cx, _| {
        let image = compute_images(cx)?.image("first")?;
        cx.copy_image(image)
    });
    let second = source(|cx, _| {
        let image = compute_images(cx)?.image("second")?;
        cx.copy_image(image)
    });
    let g = Arc::new(
        ContentGraph::describe(&roots(vec![first, second]), caps(), GraphLimits::default())
            .unwrap(),
    );
    let mut tracker = plan(g, 256).unwrap().start();
    let drops = Arc::new(AtomicUsize::new(0));
    tracker.try_admit(0).unwrap();
    tracker
        .retain_owner(0, Arc::new(Owner(drops.clone())))
        .unwrap();
    tracker
        .retain_owner(0, Arc::new(Owner(drops.clone())))
        .unwrap();
    let producer = tracker.submitted(0).unwrap();
    tracker.try_admit(1).unwrap();
    let first = tracker.submitted(1).unwrap();
    tracker.try_admit(2).unwrap();
    let second = tracker.submitted(2).unwrap();
    drop(producer);
    tracker.completed(0, false).unwrap();
    assert!(!tracker.is_valid(1).unwrap());
    assert!(!tracker.is_valid(2).unwrap());
    drop(first);
    tracker.completed(1, true).unwrap();
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(second);
    tracker.completed(2, true).unwrap();
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(tracker.live_bytes(), 0);
}

#[test]
fn instance_stream_capacity_includes_the_first_instance_offset() {
    for end in [3, 4] {
        let content = source(move |cx, _| {
            let out = compute_mesh(cx)?;
            let pass = RenderPass::mesh(&mesh_program(), target(), 3)
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
                        step: VertexStep::Instance,
                        attributes: vec![VertexAttribute {
                            location: 1,
                            offset: 0,
                            ty: ShaderValueType::Float(4),
                        }],
                    },
                )
                .draw(0..3, 1..end);
            cx.render(pass)
        });
        if end == 3 {
            describe(content, caps()).unwrap();
        } else {
            assert_eq!(
                describe(content, caps()).unwrap_err().kind,
                ContentErrorKind::InvalidDescriptor
            );
        }
    }
}
