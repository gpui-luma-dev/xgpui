//! CPU program-interface validation, intentionally independent of native drivers.
use gpui::content::*;
use std::{collections::HashSet, sync::Arc};

fn srgb() -> ImageInterpretation {
    ImageInterpretation {
        color: ColorEncoding::Srgb,
        alpha: AlphaMode::Straight,
    }
}
fn p3() -> ImageInterpretation {
    ImageInterpretation {
        color: ColorEncoding::LinearP3,
        alpha: AlphaMode::Premultiplied,
    }
}
fn fullscreen(source: &str) -> Result<PortableProgram, ContentError> {
    PortableProgram::fullscreen(source, "paint", srgb(), ImageContracts::new())
}
fn uniform<'a>(program: &'a PortableProgram, name: &str) -> &'a UniformLayout {
    let ProgramBindingKind::Uniform(layout) = program.binding(name).unwrap().kind() else {
        panic!("not a uniform")
    };
    layout
}
const SOLID: &str =
    "@fragment fn paint() -> @location(0) vec4<f32> { return vec4(1.0, 0.0, 0.0, 1.0); }";
const UNIFORMS: &str = r#"
struct Parameters { gain: f32, tint: vec3<f32>, mode: u32, transform: mat3x3<f32> }
@group(0) @binding(2) var<uniform> parameters: Parameters;
@fragment fn paint() -> @location(0) vec4<f32> {
    return vec4((parameters.transform * parameters.tint) * parameters.gain + vec3(f32(parameters.mode)), 1.0);
}
"#;
fn values(gain: f32) -> UniformValues {
    UniformValues::new()
        .float("gain", gain)
        .vec3("tint", [0.1, 0.2, 0.3])
        .uints("mode", vec![2])
        .matrix("transform", 3, 3, vec![1., 2., 3., 4., 5., 6., 7., 8., 9.])
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[test]
fn fullscreen_program_has_a_portable_vertex_interface_and_structural_identity() {
    let first = fullscreen(SOLID).unwrap();
    let second = fullscreen(SOLID).unwrap();
    assert_eq!(first, second);
    assert!(first.is_fullscreen());
    assert!(first.vertex_inputs().is_empty());
    assert_eq!(
        first.render_entries(),
        Some(("_xgpui_content_vertex", "paint"))
    );
    assert_eq!(first.fragment_output(), Some(srgb()));
    assert!(first.compute_entry().is_none());
    let wide = PortableProgram::fullscreen(SOLID, "paint", p3(), ImageContracts::new()).unwrap();
    assert_ne!(first, wide); // The interpretation participates in pipeline/result identity.
    let mut set = HashSet::new();
    set.insert(first);
    set.insert(second);
    set.insert(wide);
    assert_eq!(set.len(), 2);
}

#[test]
fn syntax_semantics_stage_names_and_duplicate_generated_names_are_rejected() {
    assert_eq!(
        fullscreen("@fragment fn paint( invalid").unwrap_err().kind,
        ContentErrorKind::ShaderValidation
    );
    assert_eq!(
        fullscreen("@fragment fn paint() -> @location(0) vec4<f32> { return nonexistent; }")
            .unwrap_err()
            .kind,
        ContentErrorKind::ShaderValidation
    );
    assert_eq!(
        PortableProgram::compute(SOLID, "paint", ImageContracts::new())
            .unwrap_err()
            .kind,
        ContentErrorKind::ShaderValidation
    );
    assert!(fullscreen(&format!("{SOLID}\n@vertex fn _xgpui_content_vertex() -> @builtin(position) vec4<f32> {{ return vec4(0.0); }}")).is_err());
    let oversized = " ".repeat(PortableProgram::MAX_SOURCE_BYTES + 1);
    assert_eq!(
        fullscreen(&oversized).unwrap_err().kind,
        ContentErrorKind::ResourceLimit
    );
}

#[test]
fn uniform_packing_uses_wgsl_offsets_and_initializes_matrix_padding() {
    let program = fullscreen(UNIFORMS).unwrap();
    let layout = uniform(&program, "parameters");
    assert_eq!(
        program.binding("parameters").unwrap().address(),
        BindingAddress {
            group: 0,
            binding: 2
        }
    );
    assert_eq!(
        layout
            .fields()
            .iter()
            .map(UniformField::offset)
            .collect::<Vec<_>>(),
        vec![0, 16, 28, 32]
    );
    assert_eq!(layout.byte_len(), 80);
    let bytes = layout.pack(&values(0.5)).unwrap();
    assert_eq!(word(&bytes, 0), 0.5f32.to_bits());
    assert!(bytes[4..16].iter().all(|v| *v == 0));
    assert_eq!(word(&bytes, 28), 2);
    for column in 0..3 {
        for row in 0..3 {
            assert_eq!(
                word(&bytes, 32 + column * 16 + row * 4),
                (1. + (column * 3 + row) as f32).to_bits()
            );
        }
        assert!(
            bytes[44 + column * 16..48 + column * 16]
                .iter()
                .all(|v| *v == 0)
        );
    }
    assert_eq!(
        layout.pack(&values(-0.)).unwrap(),
        layout.pack(&values(0.)).unwrap()
    );
}

#[test]
fn uniforms_reject_missing_unknown_wrong_type_wrong_shape_and_nonfinite_values() {
    let program = fullscreen(UNIFORMS).unwrap();
    let layout = uniform(&program, "parameters");
    for bad in [
        UniformValues::new(),
        values(0.5).float("unknown", 1.),
        values(0.5).float("mode", 2.),
        values(0.5).vec2("tint", [1., 2.]),
        values(f32::NAN),
        values(f32::INFINITY),
        values(0.5).matrix("transform", 2, 2, vec![0.; 4]),
        values(0.5).matrix("transform", 3, 3, vec![0.; 8]),
        values(0.5).matrix("transform", 3, 3, vec![f32::NEG_INFINITY; 9]),
    ] {
        assert_eq!(
            layout.pack(&bad).unwrap_err().kind,
            ContentErrorKind::InvalidDescriptor
        );
    }
}

#[test]
fn signed_and_unsigned_uniform_bits_remain_distinct() {
    let program = fullscreen(r#"
        struct Parameters { signed_value: i32, unsigned_value: u32 }
        @group(0) @binding(0) var<uniform> p: Parameters;
        @fragment fn paint() -> @location(0) vec4<f32> { return vec4(f32(p.signed_value), f32(p.unsigned_value), 0.0, 1.0); }
    "#).unwrap();
    let layout = uniform(&program, "p");
    let bytes = layout
        .pack(
            &UniformValues::new()
                .ints("signed_value", vec![-2])
                .uints("unsigned_value", vec![u32::MAX]),
        )
        .unwrap();
    assert_eq!(word(&bytes, 0), (-2i32) as u32);
    assert_eq!(word(&bytes, 4), u32::MAX);
    assert!(
        layout
            .pack(
                &UniformValues::new()
                    .uints("signed_value", vec![2])
                    .ints("unsigned_value", vec![2])
            )
            .is_err()
    );
}

const SAMPLED: &str = r#"
@group(1) @binding(2) var source: texture_2d<f32>;
@group(1) @binding(3) var source_sampler: sampler;
@group(2) @binding(0) var unused_image: texture_2d<f32>;
fn sample_source(uv: vec2<f32>) -> vec4<f32> { return textureSample(source, source_sampler, uv); }
@fragment fn paint(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { return sample_source(uv); }
"#;
#[test]
fn image_resources_require_exact_interpretations_and_reflect_helper_uses() {
    let contracts = ImageContracts::new().image("source", srgb());
    let program = PortableProgram::fullscreen(SAMPLED, "paint", srgb(), contracts.clone()).unwrap();
    assert_eq!(program.bindings().len(), 2);
    assert_eq!(
        program.binding("source").unwrap().kind(),
        &ProgramBindingKind::SampledImage(srgb())
    );
    assert_eq!(
        program.binding("source").unwrap().stages(),
        ProgramStages::FRAGMENT
    );
    assert!(program.binding("unused_image").is_none());
    assert_eq!(
        PortableProgram::fullscreen(SAMPLED, "paint", srgb(), ImageContracts::new())
            .unwrap_err()
            .kind,
        ContentErrorKind::InvalidDescriptor
    );
    assert_eq!(
        PortableProgram::fullscreen(
            SAMPLED,
            "paint",
            srgb(),
            contracts.image("unused_image", srgb())
        )
        .unwrap_err()
        .kind,
        ContentErrorKind::InvalidDescriptor
    );
    let wide_input = PortableProgram::fullscreen(
        SAMPLED,
        "paint",
        srgb(),
        ImageContracts::new().image("source", p3()),
    )
    .unwrap();
    assert_ne!(program, wide_input);
}

const MESH: &str = r#"
struct Varyings { @builtin(position) position: vec4<f32>, @location(0) color: vec4<f32> }
@vertex fn vertex_main(@location(0) position: vec2<f32>, @location(1) color: vec4<f32>) -> Varyings {
    var out: Varyings; out.position = vec4(position, 0.0, 1.0); out.color = color; return out;
}
@fragment fn fragment_main(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> { return color; }
"#;
#[test]
fn mesh_program_reflects_vertex_inputs_and_links_selected_stages() {
    let program = PortableProgram::render(
        MESH,
        "vertex_main",
        "fragment_main",
        srgb(),
        ImageContracts::new(),
    )
    .unwrap();
    assert!(!program.is_fullscreen());
    assert_eq!(
        program.vertex_inputs(),
        &[
            VertexInput {
                location: 0,
                ty: ShaderValueType::Float(2)
            },
            VertexInput {
                location: 1,
                ty: ShaderValueType::Float(4)
            }
        ]
    );
}

#[test]
fn individually_valid_stages_cannot_link_mismatched_locations_types_or_qualifiers() {
    for source in [
        "@fragment fn paint(@location(0) uv: vec3<f32>) -> @location(0) vec4<f32> { return vec4(uv, 1.0); }",
        "@fragment fn paint(@location(1) uv: vec2<f32>) -> @location(0) vec4<f32> { return vec4(uv, 0.0, 1.0); }",
        "@fragment fn paint(@location(0) @interpolate(flat) uv: vec2<f32>) -> @location(0) vec4<f32> { return vec4(uv, 0.0, 1.0); }",
    ] {
        assert_eq!(
            fullscreen(source).unwrap_err().kind,
            ContentErrorKind::ShaderValidation
        );
    }
}

const COMPUTE: &str = r#"
struct State { header: vec4<u32>, values: array<vec4<f32>> }
@group(0) @binding(0) var<storage, read_write> state: State;
var<workgroup> shared_values: array<u32, 4>;
fn read_shared(i: u32) -> u32 { return shared_values[i]; }
@compute @workgroup_size(4, 1, 1)
fn compute_main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    shared_values[local] = id.x; workgroupBarrier();
    if id.x < arrayLength(&state.values) { state.values[id.x] = vec4(f32(read_shared(local))); }
}
"#;
fn compute_limits() -> ComputeLimits {
    ComputeLimits {
        max_workgroup_size: [8, 8, 8],
        max_workgroup_invocations: 64,
        max_dispatch: [65535; 3],
        max_workgroup_memory_bytes: 32,
    }
}

#[test]
fn compute_reflects_runtime_storage_tail_access_and_shared_memory() {
    let program = PortableProgram::compute(COMPUTE, "compute_main", ImageContracts::new()).unwrap();
    assert_eq!(program.workgroup_size(), Some([4, 1, 1]));
    assert_eq!(program.workgroup_memory_bytes(), 16);
    assert!(program.render_entries().is_none());
    let binding = program.binding("state").unwrap();
    assert_eq!(binding.stages(), ProgramStages::COMPUTE);
    let ProgramBindingKind::StorageBuffer { layout, access } = binding.kind() else {
        panic!()
    };
    assert_eq!(*access, StorageAccess::ReadWrite);
    assert_eq!(layout.minimum_bytes(), 32);
    assert_eq!(layout.alignment(), 16);
    assert_eq!(
        layout.runtime_array(),
        Some(RuntimeArrayLayout {
            offset: 16,
            stride: 16
        })
    );
    layout.validate_byte_len(32).unwrap();
    layout.validate_byte_len(64).unwrap();
    assert!(layout.validate_byte_len(16).is_err());
    assert!(layout.validate_byte_len(33).is_err());
}

#[test]
fn dispatch_checks_availability_axes_total_invocations_and_workgroup_memory() {
    let program = PortableProgram::compute(COMPUTE, "compute_main", ImageContracts::new()).unwrap();
    program
        .validate_dispatch([1; 3], Some(&compute_limits()))
        .unwrap();
    assert_eq!(
        program.validate_dispatch([1; 3], None).unwrap_err().kind,
        ContentErrorKind::UnsupportedCapability
    );
    assert_eq!(
        program
            .validate_dispatch([0, 1, 1], Some(&compute_limits()))
            .unwrap_err()
            .kind,
        ContentErrorKind::InvalidDescriptor
    );
    assert!(
        program
            .validate_dispatch([65536, 1, 1], Some(&compute_limits()))
            .is_err()
    );
    for limits in [
        ComputeLimits {
            max_workgroup_size: [2, 8, 8],
            ..compute_limits()
        },
        ComputeLimits {
            max_workgroup_invocations: 2,
            ..compute_limits()
        },
        ComputeLimits {
            max_workgroup_memory_bytes: 8,
            ..compute_limits()
        },
    ] {
        assert!(program.validate_dispatch([1; 3], Some(&limits)).is_err());
    }
    assert!(
        fullscreen(SOLID)
            .unwrap()
            .validate_dispatch([1; 3], Some(&compute_limits()))
            .is_err()
    );
}

#[test]
fn storage_image_format_and_access_are_reflected_without_inferring_color() {
    let source = r#"
        @group(0) @binding(0) var destination: texture_storage_2d<rgba16float, write>;
        @compute @workgroup_size(1) fn main() { textureStore(destination, vec2<i32>(0), vec4(1.0)); }
    "#;
    let program = PortableProgram::compute(
        source,
        "main",
        ImageContracts::new().image("destination", p3()),
    )
    .unwrap();
    assert_eq!(
        program.binding("destination").unwrap().kind(),
        &ProgramBindingKind::StorageImage {
            format: PixelFormat::Rgba16Float,
            access: StorageAccess::WriteOnly,
            interpretation: p3(),
        }
    );
}

#[test]
fn only_resources_used_by_the_selected_compute_entry_are_required() {
    let source = r#"
        @group(0) @binding(0) var<storage, read_write> a: array<u32>;
        @group(0) @binding(1) var<storage, read_write> b: array<u32>;
        @compute @workgroup_size(1) fn first() { a[0] = 1u; }
        @compute @workgroup_size(1) fn second() { b[0] = 2u; }
    "#;
    let first = PortableProgram::compute(source, "first", ImageContracts::new()).unwrap();
    assert_eq!(first.bindings().len(), 1);
    assert!(first.binding("a").is_some());
    assert!(first.binding("b").is_none());
    let second = PortableProgram::compute(source, "second", ImageContracts::new()).unwrap();
    assert_ne!(first, second);
}

#[test]
fn unsupported_interfaces_are_explicit_errors_instead_of_silent_lowering() {
    let wrong_output = "@fragment fn paint() -> @location(0) vec3<f32> { return vec3(1.0); }";
    assert_eq!(
        fullscreen(wrong_output).unwrap_err().kind,
        ContentErrorKind::UnsupportedCapability
    );
    let nested_uniform = r#"
        struct Inner { value: vec4<f32> } struct Outer { inner: Inner }
        @group(0) @binding(0) var<uniform> params: Outer;
        @fragment fn paint() -> @location(0) vec4<f32> { return params.inner.value; }
    "#;
    assert_eq!(
        fullscreen(nested_uniform).unwrap_err().kind,
        ContentErrorKind::UnsupportedCapability
    );
    let depth_sampler = r#"
        @group(0) @binding(0) var depth: texture_depth_2d;
        @group(0) @binding(1) var compare: sampler_comparison;
        @fragment fn paint(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { return vec4(textureSampleCompare(depth, compare, uv, 0.5)); }
    "#;
    assert!(
        PortableProgram::fullscreen(
            depth_sampler,
            "paint",
            srgb(),
            ImageContracts::new().image("depth", srgb())
        )
        .is_err()
    );
}

#[test]
fn programs_can_be_validated_on_a_worker_without_renderer_or_gpu_handles() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PortableProgram>();
    let source: Arc<str> = Arc::from(UNIFORMS);
    let program = std::thread::spawn(move || {
        PortableProgram::fullscreen(source, "paint", srgb(), ImageContracts::new()).unwrap()
    })
    .join()
    .unwrap();
    uniform(&program, "parameters").pack(&values(0.5)).unwrap();
}

#[test]
fn all_matrix_dimensions_use_reflected_column_padding() {
    for columns in 2..=4u8 {
        for rows in 2..=4u8 {
            let source = format!(
                r#"
                struct Parameters {{ matrix: mat{columns}x{rows}<f32> }}
                @group(0) @binding(0) var<uniform> p: Parameters;
                @fragment fn paint() -> @location(0) vec4<f32> {{ return vec4(p.matrix[0][0]); }}
            "#
            );
            let program = fullscreen(&source).unwrap();
            let layout = uniform(&program, "p");
            let values: Vec<f32> = (0..columns * rows).map(|i| f32::from(i + 1)).collect();
            let bytes = layout
                .pack(&UniformValues::new().matrix("matrix", columns, rows, values.clone()))
                .unwrap();
            let stride = if rows == 2 { 8usize } else { 16 };
            assert_eq!(
                layout.fields()[0].ty(),
                UniformType::Matrix {
                    columns,
                    rows,
                    column_stride: stride as u32
                }
            );
            for column in 0..usize::from(columns) {
                for row in 0..usize::from(rows) {
                    assert_eq!(
                        word(&bytes, column * stride + row * 4),
                        values[column * usize::from(rows) + row].to_bits()
                    );
                }
                assert!(
                    bytes[column * stride + usize::from(rows) * 4..(column + 1) * stride]
                        .iter()
                        .all(|v| *v == 0)
                );
            }
        }
    }
}

#[test]
fn cross_stage_binding_collisions_are_rejected() {
    let source = r#"
        struct Parameters { value: vec4<f32> }
        @group(0) @binding(0) var<uniform> vertex_parameters: Parameters;
        @group(0) @binding(0) var<uniform> fragment_parameters: Parameters;
        @vertex fn vertex_main() -> @builtin(position) vec4<f32> { return vertex_parameters.value; }
        @fragment fn fragment_main() -> @location(0) vec4<f32> { return fragment_parameters.value; }
    "#;
    assert_eq!(
        PortableProgram::render(
            source,
            "vertex_main",
            "fragment_main",
            srgb(),
            ImageContracts::new()
        )
        .unwrap_err()
        .kind,
        ContentErrorKind::ShaderValidation
    );
}

#[test]
fn shared_memory_accounting_includes_per_variable_padding() {
    let source = r#"
        var<workgroup> shared_scalar: u32;
        var<workgroup> shared_vector: vec3<f32>;
        @group(0) @binding(0) var<storage, read_write> result: vec4<f32>;
        @compute @workgroup_size(1) fn main() {
            shared_scalar = 1u; shared_vector = vec3(2.0); workgroupBarrier();
            result = vec4(shared_vector, f32(shared_scalar));
        }
    "#;
    let program = PortableProgram::compute(source, "main", ImageContracts::new()).unwrap();
    assert_eq!(program.workgroup_memory_bytes(), 32);
    assert!(
        program
            .validate_dispatch(
                [1; 3],
                Some(&ComputeLimits {
                    max_workgroup_memory_bytes: 16,
                    ..compute_limits()
                })
            )
            .is_err()
    );
}
