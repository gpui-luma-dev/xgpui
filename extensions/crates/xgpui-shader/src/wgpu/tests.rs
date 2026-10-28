#[test]
fn portable_effects_validate_and_generate_vulkan_and_windows_shaders() {
    for source in [include_str!("gradient.wgsl"), include_str!("mesh.wgsl")] {
        let module = naga::front::wgsl::parse_str(source).expect("WGSL parses");
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        for entry in &module.entry_points {
            let options = naga::back::spv::PipelineOptions {
                shader_stage: entry.stage,
                entry_point: entry.name.clone(),
            };
            let words =
                naga::back::spv::write_vec(&module, &info, &Default::default(), Some(&options))
                    .expect("SPIR-V generation");
            assert_eq!(words[0], 0x07230203);
            let options = naga::back::hlsl::Options {
                shader_model: naga::back::hlsl::ShaderModel::V5_0,
                ..Default::default()
            };
            let pipeline = naga::back::hlsl::PipelineOptions {
                entry_point: Some((entry.stage, entry.name.clone())),
            };
            let mut hlsl = String::new();
            let reflection = naga::back::hlsl::Writer::new(&mut hlsl, &options, &pipeline)
                .write(&module, &info, None)
                .expect("HLSL generation");
            assert!(reflection.entry_point_names.iter().all(Result::is_ok));
            assert!(!hlsl.is_empty());
        }
    }
}
