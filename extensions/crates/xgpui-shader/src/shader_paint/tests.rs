use super::*;
const SOURCE: &str = r#"
struct Parameters { gain:f32, color:vec3<f32>, alpha:f32 }
fn paint(input:PaintInput)->vec4<f32>{return vec4(parameters.color+vec3(input.uv*parameters.gain,0.),parameters.alpha);}
"#;
fn parameters() -> UniformValues {
    UniformValues::new()
        .float("gain", 0.25)
        .vec3("color", [0.2, 0.4, 0.6])
        .float("alpha", 0.75)
}
#[test]
fn portable_snapshots_pack_core_reflection_and_keep_identity() {
    let program = ShaderProgram::new(SOURCE).unwrap();
    let paint = program.paint(parameters()).unwrap();
    let bytes = program
        .parameter_layout()
        .pack(&paint.source.parameters)
        .unwrap();
    assert_eq!(bytes.len(), 32);
    assert_eq!(&bytes[0..4], &0.25f32.to_le_bytes());
    assert_eq!(&bytes[4..16], &[0; 12]);
    assert_eq!(&bytes[16..20], &0.2f32.to_le_bytes());
    assert_eq!(&bytes[28..32], &0.75f32.to_le_bytes());
    assert_eq!(paint.content().id(), paint.clone().content().id());
    let next = program.paint(parameters().float("gain", 0.5)).unwrap();
    assert_ne!(paint.content().id(), next.content().id());
    assert!(Arc::ptr_eq(&paint.source.program.0, &next.source.program.0));
    let _: crate::Paint = paint.into();
}
#[test]
fn parameter_sets_use_exact_names_types_and_finite_values() {
    let program = ShaderProgram::new(SOURCE).unwrap();
    for values in [
        UniformValues::new(),
        parameters().float("unknown", 1.),
        parameters().vec2("gain", [1.; 2]),
        parameters().float("gain", f32::NAN),
        parameters().vec3("color", [f32::INFINITY; 3]),
    ] {
        assert_eq!(
            program.paint(values).unwrap_err().kind,
            ContentErrorKind::InvalidDescriptor
        );
    }
}
#[test]
fn unused_parameters_remain_validated_without_becoming_execution_bindings() {
    let program = ShaderProgram::new(
        "struct Parameters { ignored:f32 } fn paint(input:PaintInput)->vec4<f32>{return vec4(1.);}",
    )
    .unwrap();
    assert!(program.0.portable.binding("parameters").is_none());
    assert!(program.paint(UniformValues::new()).is_err());
    assert!(
        program
            .paint(UniformValues::new().float("ignored", f32::NAN))
            .is_err()
    );
    let paint = program
        .paint(UniformValues::new().float("ignored", 1.))
        .unwrap();
    let graph = ContentGraph::describe(
        &[ContentRoot {
            content: paint.content(),
            request: ContentRequest::new(
                gpui::size(gpui::px(8.), gpui::px(8.)),
                ImageExtent::new(8, 8).unwrap(),
                1.,
            )
            .unwrap(),
        }],
        ContentCapabilities::default(),
        GraphLimits::default(),
    )
    .unwrap();
    assert_eq!(graph.nodes().len(), 1);
}
#[test]
fn integer_matrix_parameters_loops_derivatives_and_discard_use_portable_validation() {
    let source = "struct Parameters { index:i32, count:u32, transform:mat2x2<f32> } fn paint(input:PaintInput)->vec4<f32>{ var v=input.uv; for(var i=0u;i<2u;i++){ v=parameters.transform*v; } if parameters.index<0 {discard;} return vec4(v+dpdx(input.uv),f32(parameters.count),1.);}";
    let program = ShaderProgram::new(source).unwrap();
    assert!(
        program
            .paint(
                UniformValues::new()
                    .ints("index", [0])
                    .uints("count", [1])
                    .matrix("transform", 2, 2, [1., 0., 0., 1.])
            )
            .is_ok()
    );
}
#[test]
fn rejects_invalid_interfaces_and_reports_user_source_lines() {
    for source in [format!("{SOURCE}\n@group(2) @binding(0) var extra:texture_2d<f32>;"),format!("{SOURCE}\n@compute @workgroup_size(1) fn other(){{}}"),SOURCE.replace("input.uv*parameters.gain","xgpui_geometry.dimensions.xy"),"struct Parameters { values:array<vec4<f32>,2> } fn paint(input:PaintInput)->vec4<f32>{return parameters.values[0];}".into()] {
 // Geometry's private binding cannot be read by user code; use PaintInput instead.
 assert!(ShaderProgram::new(&source).is_err(),"accepted {source}");
 }
    assert!(ShaderProgram::new(&" ".repeat(65537)).is_err());
    let error = ShaderProgram::new(
        "struct Parameters { gain:f32 }\nfn paint(input:PaintInput)->vec4<f32>{return missing;}",
    )
    .unwrap_err();
    assert!(error.to_string().contains("user WGSL line 2"), "{error}");
    let error = ShaderProgram::new(
        "struct Parameters { gain:f32 }\nfn paint(input:PaintInput)->vec4<f32>{return input.uv;}",
    )
    .unwrap_err();
    assert!(error.to_string().contains("user WGSL line 2"), "{error}");
}
