use super::*;

const SOURCE: &str = r#"
struct Parameters { gain: f32, color: vec3<f32>, alpha: f32 }
fn paint(input: PaintInput) -> vec4<f32> {
    return vec4(parameters.color + vec3(input.uv * parameters.gain, 0.), parameters.alpha);
}
"#;
fn parameters() -> ShaderParameters {
    ShaderParameters::new()
        .float("gain", 0.25)
        .vec3("color", [0.2, 0.4, 0.6])
        .float("alpha", 0.75)
}
#[test]
fn portable_program_translates_and_packs_reflected_alignment() -> Result<()> {
    let program = ShaderProgram::new(SOURCE)?;
    let paint = program.paint(parameters())?;
    assert_eq!(paint.0.bytes.len(), 32);
    assert_eq!(&paint.0.bytes[0..4], &0.25_f32.to_le_bytes());
    assert_eq!(&paint.0.bytes[4..16], &[0; 12]);
    assert_eq!(&paint.0.bytes[16..20], &0.2_f32.to_le_bytes());
    assert_eq!(&paint.0.bytes[28..32], &0.75_f32.to_le_bytes());
    assert!(
        paint
            .prepare(&(), size(DevicePixels(8), DevicePixels(8)))
            .is_err()
    );
    let _: Paint = paint.into();
    Ok(())
}
#[test]
fn portable_program_rejects_bad_parameter_sets_and_keeps_snapshots() -> Result<()> {
    let program = ShaderProgram::new(SOURCE)?;
    assert!(program.paint(ShaderParameters::new()).is_err());
    assert!(program.paint(parameters().float("unknown", 1.)).is_err());
    assert!(program.paint(parameters().vec2("gain", [1.; 2])).is_err());
    assert!(program.paint(parameters().float("gain", f32::NAN)).is_err());
    assert!(
        program
            .paint(parameters().vec3("color", [f32::INFINITY; 3]))
            .is_err()
    );
    let first = program.paint(parameters())?;
    let clone = first.clone();
    let next = program.paint(parameters().float("gain", 0.5))?;
    assert!(Arc::ptr_eq(&first.0, &clone.0));
    assert!(Arc::ptr_eq(&first.0.program.0, &next.0.program.0));
    assert_ne!(first.0.bytes, next.0.bytes);
    Ok(())
}
#[test]
fn portable_program_rejects_out_of_subset_source() {
    let cases=[
        SOURCE.replace("return vec4", "discard; return vec4"),
        SOURCE.replace("return vec4", "for(var i=0u;i<2u;i++) {} return vec4"),
        SOURCE.replace("input.uv * parameters.gain", "dpdx(input.uv) * parameters.gain"),
        format!("{SOURCE}\n@group(2) @binding(0) var extra: texture_2d<f32>;"),
        format!("{SOURCE}\n@compute @workgroup_size(1) fn other() {{}}"),
        SOURCE.replace("input.uv * parameters.gain", "xgpui_geometry.dimensions.xy"),
        "struct Parameters { index: i32 } fn paint(input: PaintInput) -> vec4<f32> { return vec4(f32(parameters.index)); }".into(),
        "struct Parameters { values: array<vec4<f32>,2> } fn paint(input: PaintInput) -> vec4<f32> { return parameters.values[0]; }".into(),
        "struct Parameters { color: vec4<f32> } fn paint(input: PaintInput) -> vec3<f32> { return parameters.color.rgb; }".into(),
    ];
    for source in cases {
        assert!(
            ShaderProgram::new(&source).is_err(),
            "accepted unsupported source: {source}"
        );
    }
    assert!(ShaderProgram::new(&" ".repeat(65537)).is_err());
    let error=ShaderProgram::new("struct Parameters { gain: f32 }\nfn paint(input: PaintInput) -> vec4<f32> { return missing; }").unwrap_err();
    assert!(error.to_string().contains("user WGSL line 2"), "{error:#}");
    let error = ShaderProgram::new("struct Parameters { gain: f32 }\nfn paint(input: PaintInput) -> vec4<f32> { return input.uv; }").unwrap_err();
    assert!(error.to_string().contains("user WGSL line 2"), "{error:#}");
}
