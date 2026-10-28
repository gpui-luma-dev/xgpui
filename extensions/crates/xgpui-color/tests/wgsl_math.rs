use xgpui_color::*;

fn vectors() -> Vec<(String, [f64; 4])> {
    let mut cases = Vec::new();
    let rgba = |v: [f64; 3]| [v[0], v[1], v[2], 1.];
    let wg = |v: [f64; 3]| format!("vec3<f32>({:?}, {:?}, {:?})", v[0], v[1], v[2]);
    let spaces = [RgbSpace::Srgb, RgbSpace::DisplayP3, RgbSpace::Rec2020];
    for (i, space) in spaces.into_iter().enumerate() {
        for v in [
            [0.; 3],
            [1.; 3],
            [-0.2, 0.5, 1.2],
            [1., 0., 0.],
            [0., 1., 0.],
            [0.04045, 0.0031308, 0.0812428583],
        ] {
            let rgb = LinearRgb::new(space, v).unwrap();
            cases.push((
                format!("vec4(gamut_decode({}, {i}u),1.0)", wg(v)),
                rgba(
                    EncodedRgb::new(space, v)
                        .unwrap()
                        .decode()
                        .unwrap()
                        .components(),
                ),
            ));
            cases.push((
                format!("vec4(gamut_encode({}, {i}u),1.0)", wg(v)),
                rgba(rgb.encode().unwrap().components()),
            ));
            cases.push((
                format!("vec4(gamut_to_oklab({}, {i}u),1.0)", wg(v)),
                rgba(rgb.to_oklab().unwrap().components()),
            ));
            for (j, dest) in spaces.into_iter().enumerate() {
                cases.push((
                    format!("vec4(gamut_convert({}, {i}u, {j}u),1.0)", wg(v)),
                    rgba(rgb.convert(dest).unwrap().components()),
                ));
            }
            for (policy, id) in [(GamutMapping::Clip, 0), (GamutMapping::ReduceChroma, 1)] {
                cases.push((
                    format!("vec4(gamut_map({}, {i}u, {id}u),1.0)", wg(v)),
                    rgba(rgb.map_gamut(space, policy).unwrap().components()),
                ));
            }
            let lab = rgb.to_oklab().unwrap();
            cases.push((
                format!("vec4(gamut_from_oklab({}, {i}u),1.0)", wg(lab.components())),
                rgba(lab.to_linear(space).unwrap().components()),
            ));
        }
        for (model, interpolation) in [
            (0, ColorInterpolation::LinearRgb),
            (1, ColorInterpolation::Oklab),
            (2, ColorInterpolation::Oklch(HueInterpolation::Shorter)),
            (2, ColorInterpolation::Oklch(HueInterpolation::Longer)),
            (2, ColorInterpolation::Oklch(HueInterpolation::Increasing)),
            (2, ColorInterpolation::Oklch(HueInterpolation::Decreasing)),
        ] {
            let hue = match interpolation {
                ColorInterpolation::Oklch(HueInterpolation::Longer) => 1,
                ColorInterpolation::Oklch(HueInterpolation::Increasing) => 2,
                ColorInterpolation::Oklch(HueInterpolation::Decreasing) => 3,
                _ => 0,
            };
            for (first, second, alpha_a, alpha_b) in [
                ([1.; 3], [0., 0., 1.], 1., 0.75),
                ([0.125; 3], [0., 0., 1.], 1., 0.75),
                ([0.125; 3], [1.; 3], 1., 1.),
                ([1., 0., 0.], [0., 0., 1.], 0., 0.),
            ] {
                let a = LinearRgba::new(LinearRgb::new(space, first).unwrap(), alpha_a).unwrap();
                let b = LinearRgba::new(LinearRgb::new(space, second).unwrap(), alpha_b).unwrap();
                let gradient =
                    LinearGradient::new([0.; 2], [1., 0.], [a, b], space, interpolation).unwrap();
                for t in [0., 0.25, 0.5, 1.] {
                    let result = gradient.sample_at(t).unwrap();
                    let [r, g, b] = result.rgb().components();
                    cases.push((format!("gamut_interpolate(vec4({}, {alpha_a:?}),vec4({}, {alpha_b:?}),{t:?},{i}u,{model}u,{hue}u)",wg(first),wg(second)),[r,g,b,result.alpha()]));
                }
            }
            for alpha in [0., 0.5, 1.] {
                let a =
                    LinearRgba::new(LinearRgb::new(space, [1., 0., 0.]).unwrap(), alpha).unwrap();
                let b =
                    LinearRgba::new(LinearRgb::new(space, [0., 0., 1.]).unwrap(), 0.75).unwrap();
                let gradient =
                    LinearGradient::new([0.; 2], [1., 0.], [a, b], space, interpolation).unwrap();
                for t in [0., 0.25, 0.5, 1.] {
                    let result = gradient.sample_at(t).unwrap();
                    let [r, g, b] = result.rgb().components();
                    cases.push((format!("gamut_interpolate(vec4(1.0,0.0,0.0,{alpha:?}),vec4(0.0,0.0,1.0,0.75),{t:?},{i}u,{model}u,{hue}u)"),[r,g,b,result.alpha()]));
                }
            }
        }
    }
    cases
}
fn source(cases: &[(String, [f64; 4])]) -> String {
    let assignments = cases
        .iter()
        .enumerate()
        .map(|(i, (expr, _))| format!("case {i}u: {{ output[{i}] = {expr}; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{WGSL_COLOR_MATH}\n@group(0) @binding(0) var<storage,read_write> output: array<vec4<f32>>;\n@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{ switch id.x {{ {assignments} default: {{}} }} }}"
    )
}
fn validated(source: &str) -> (naga::Module, naga::valid::ModuleInfo) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
    (module, info)
}
#[test]
fn helpers_translate_to_all_native_shader_languages() {
    let (module, info) = validated(&source(&vectors()));
    naga::back::msl::write_string(&module, &info, &Default::default(), &Default::default())
        .unwrap();
    let mut hlsl = String::new();
    naga::back::hlsl::Writer::new(&mut hlsl, &Default::default(), &Default::default())
        .write(&module, &info, None)
        .unwrap();
    naga::back::spv::write_vec(&module, &info, &Default::default(), None).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn metal_float32_matches_cpu_reference() {
    use metal::*;
    let cases = vectors();
    let (module, info) = validated(&source(&cases));
    let mut options = naga::back::msl::Options::default();
    let mut resources = naga::back::msl::EntryPointResources::default();
    resources.resources.insert(
        naga::ResourceBinding {
            group: 0,
            binding: 0,
        },
        naga::back::msl::BindTarget {
            buffer: Some(0),
            ..Default::default()
        },
    );
    options.per_entry_point_map.insert("main".into(), resources);
    let (msl, reflection) =
        naga::back::msl::write_string(&module, &info, &options, &Default::default()).unwrap();
    let device = Device::system_default().expect("Metal device required for native validation");
    let compile = CompileOptions::new();
    compile.set_fast_math_enabled(false);
    let library = device.new_library_with_source(&msl, &compile).unwrap();
    let entry = reflection.entry_point_names[0].as_ref().unwrap();
    let function = library.get_function(entry, None).unwrap();
    let pipeline = device
        .new_compute_pipeline_state_with_function(&function)
        .unwrap();
    let buffer = device.new_buffer(
        (cases.len() * 16) as u64,
        MTLResourceOptions::StorageModeShared,
    );
    let queue = device.new_command_queue();
    let command = queue.new_command_buffer();
    let encoder = command.new_compute_command_encoder();
    encoder.set_compute_pipeline_state(&pipeline);
    encoder.set_buffer(0, Some(&buffer), 0);
    encoder.dispatch_threads(
        MTLSize::new(cases.len() as u64, 1, 1),
        MTLSize::new(1, 1, 1),
    );
    encoder.end_encoding();
    command.commit();
    command.wait_until_completed();
    assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
    // Shared buffer is CPU-readable after command completion; exactly len*16 bytes.
    let output =
        unsafe { std::slice::from_raw_parts(buffer.contents() as *const [f32; 4], cases.len()) };
    for ((expr, expected), actual) in cases.iter().zip(output) {
        for i in 0..4 {
            assert!(
                actual[i].is_finite() && (actual[i] as f64 - expected[i]).abs() <= 3e-5,
                "{expr}: actual={actual:?}, expected={expected:?}"
            );
        }
    }
}
