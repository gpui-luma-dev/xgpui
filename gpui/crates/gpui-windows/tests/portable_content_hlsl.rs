//! Translation checks run on every host; GPU acceptance is in portable_content_dx11.
#[path = "../src/content_executor/translation.rs"]
mod translation;
use gpui::content::*;
fn srgb() -> ImageInterpretation {
    ImageInterpretation {
        color: ColorEncoding::Srgb,
        alpha: AlphaMode::Straight,
    }
}
#[test]
fn portable_hlsl_covers_resources_entry_names_and_shader_model_five() {
    let shader = "struct P{tint:vec4<f32>} @group(2) @binding(7) var<uniform> params:P; @group(1) @binding(3) var image:texture_2d<f32>; @group(3) @binding(6) var sample_state:sampler; @fragment fn paint(@location(0) uv:vec2<f32>)->@location(0) vec4<f32>{return textureSample(image,sample_state,uv)*params.tint;}";
    let p = PortableProgram::fullscreen(
        shader,
        "paint",
        srgb(),
        ImageContracts::new().image("image", srgb()),
    )
    .unwrap();
    let t = translation::translate(&p, naga::ShaderStage::Fragment, "paint").unwrap();
    assert!(t.source.contains("register(b0)"));
    assert!(t.source.contains("register(t0)"));
    assert!(t.source.contains("register(s0)"));
    assert!(!t.source.contains("space"));
    assert!(!t.entry.is_empty());
    assert_eq!(t.slots.len(), 3);
    assert!(matches!(t.slots["params"], translation::Slot::Uniform(0)));
    assert!(matches!(t.slots["image"], translation::Slot::Read(0)));
    assert!(matches!(
        t.slots["sample_state"],
        translation::Slot::Sampler(0)
    ));
}
#[test]
fn real_gradient_mesh_and_runtime_lengths_translate() {
    let gradient = PortableProgram::fullscreen(
        include_str!("../../../../extensions/crates/xgpui-shader/src/gradient/gradient.wgsl"),
        "paint_fragment",
        srgb(),
        ImageContracts::new(),
    )
    .unwrap();
    translation::translate(&gradient, naga::ShaderStage::Fragment, "paint_fragment").unwrap();
    let compute=PortableProgram::compute("@group(0) @binding(0) var<storage,read_write> values:array<vec4<f32>>;var<workgroup> shared_data:array<f32,4>;@compute @workgroup_size(4) fn main(@builtin(local_invocation_index) id:u32){shared_data[id]=f32(id);workgroupBarrier();if id<arrayLength(&values){values[id]=vec4(shared_data[(id+1u)%4u]);}}","main",ImageContracts::new()).unwrap();
    let t = translation::translate(&compute, naga::ShaderStage::Compute, "main").unwrap();
    assert!(matches!(t.slots["values"], translation::Slot::Write(0)));
    assert!(t.source.contains("RWByteAddressBuffer"));
    assert!(t.source.contains("GetDimensions"));
    assert!(t.source.contains("GroupMemoryBarrierWithGroupSync"));
    let mesh = PortableProgram::render(
        include_str!("../../../../extensions/crates/xgpui-shader/src/mesh/render.wgsl"),
        "mesh_vertex",
        "mesh_fragment",
        srgb(),
        ImageContracts::new(),
    )
    .unwrap();
    for (stage, entry) in [
        (naga::ShaderStage::Vertex, "mesh_vertex"),
        (naga::ShaderStage::Fragment, "mesh_fragment"),
    ] {
        translation::translate(&mesh, stage, entry).unwrap();
    }
}

#[test]
fn tessellation_and_unused_uniforms_translate_without_sampler_heaps() {
    let compute = PortableProgram::compute(
        include_str!("../../../../extensions/crates/xgpui-shader/src/mesh/tessellate.wgsl"),
        "tessellate",
        ImageContracts::new(),
    )
    .unwrap();
    let t = translation::translate(&compute, naga::ShaderStage::Compute, "tessellate").unwrap();
    assert!(t.source.contains("numthreads(64, 1, 1)"));
    let unused=PortableProgram::fullscreen("struct P{value:mat4x4<f32>}@group(0)@binding(0)var<uniform> unused:P;@fragment fn color()->@location(0) vec4<f32>{return vec4(1.);}","color",srgb(),ImageContracts::new()).unwrap();
    translation::translate(&unused, naga::ShaderStage::Fragment, "color").unwrap();
}

#[test]
fn builtin_draw_offsets_and_dispatch_counts_use_sm5_constants() {
    let p=PortableProgram::compute("@group(0)@binding(0)var<storage,read_write> values:array<vec4<f32>>;@compute @workgroup_size(1)fn main(@builtin(num_workgroups)count:vec3<u32>){values[0]=vec4(vec3<f32>(count),1.);}","main",ImageContracts::new()).unwrap();
    let t = translation::translate(&p, naga::ShaderStage::Compute, "main").unwrap();
    assert!(t.source.contains("cbuffer NagaDrawOrDispatch"));
    assert!(t.source.contains("_NagaConstants.first_vertex"));
    assert!(!t.source.contains("ConstantBuffer<"));
    let p = PortableProgram::fullscreen(
        "@fragment fn paint()->@location(0)vec4<f32>{return vec4(1.);}",
        "paint",
        srgb(),
        ImageContracts::new(),
    )
    .unwrap();
    let t = translation::translate(&p, naga::ShaderStage::Vertex, p.render_entries().unwrap().0)
        .unwrap();
    assert!(t.source.contains("_NagaConstants.first_vertex"));
}

#[test]
fn stage_specific_samplers_keep_global_registers_and_renamed_identifiers() {
    let p = PortableProgram::render(
        r#"
        @group(0) @binding(0) var image: texture_2d<f32>;
        @group(0) @binding(1) var linear: sampler;
        @group(0) @binding(2) var point: sampler;
        struct V { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
        @vertex fn vertex_main(@builtin(vertex_index) index: u32) -> V {
            let c = textureSampleLevel(image, linear, vec2(0.5), 0.0);
            return V(vec4(f32(index) + c.x, 0.0, 0.0, 1.0), vec2(0.5));
        }
        @fragment fn fragment_main(v: V) -> @location(0) vec4<f32> {
            return textureSample(image, point, v.uv);
        }
        "#,
        "vertex_main",
        "fragment_main",
        srgb(),
        ImageContracts::new().image("image", srgb()),
    )
    .unwrap();
    for (stage, entry, original, register, method) in [
        (
            naga::ShaderStage::Vertex,
            "vertex_main",
            "linear",
            0,
            "SampleLevel",
        ),
        (
            naga::ShaderStage::Fragment,
            "fragment_main",
            "point",
            1,
            "Sample",
        ),
    ] {
        let t = translation::translate(&p, stage, entry).unwrap();
        assert!(matches!(t.slots[original], translation::Slot::Sampler(r) if r == register));
        let declaration = t
            .source
            .lines()
            .find(|line| {
                line.starts_with("SamplerState ")
                    && line.contains(&format!("register(s{register})"))
            })
            .expect("stage sampler must use its module-wide assigned register");
        let generated = declaration.split_whitespace().nth(1).unwrap();
        assert_ne!(
            generated, original,
            "fixture must exercise HLSL identifier renaming"
        );
        assert!(
            t.source.contains(&format!(".{method}({generated},")),
            "{}",
            t.source
        );
        assert!(!t.source.contains("nagaSamplerHeap["));
    }
}
