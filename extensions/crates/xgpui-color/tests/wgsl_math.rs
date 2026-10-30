use xgpui_color::*;

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "reference/adobe_rgb.rs"]
mod adobe_reference;

fn vectors() -> Vec<(String, [f64; 4])> {
    let mut cases = Vec::new();
    let rgba = |v: [f64; 3]| [v[0], v[1], v[2], 1.];
    let wg = |v: [f64; 3]| format!("vec3<f32>({:?}, {:?}, {:?})", v[0], v[1], v[2]);
    let spaces = [
        RgbSpace::Srgb,
        RgbSpace::DisplayP3,
        RgbSpace::Rec2020,
        RgbSpace::AdobeRgb1998,
    ];
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
fn metal_outputs(cases: &[(String, [f64; 4])], fast_math: bool) -> Vec<[f32; 4]> {
    use objc2_metal::*;
    let (module, info) = validated(&source(cases));
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
    let device =
        MTLCreateSystemDefaultDevice().expect("Metal device required for native validation");
    let compile = MTLCompileOptions::new();
    // Exercise the compiler switch used on the supported older macOS hosts.
    #[allow(deprecated)]
    compile.setFastMathEnabled(fast_math);
    let library = device
        .newLibraryWithSource_options_error(
            &objc2_foundation::NSString::from_str(&msl),
            Some(&compile),
        )
        .unwrap();
    let entry = reflection.entry_point_names[0].as_ref().unwrap();
    let function = library
        .newFunctionWithName(&objc2_foundation::NSString::from_str(entry))
        .expect("Metal function missing");
    let pipeline = device
        .newComputePipelineStateWithFunction_error(&function)
        .unwrap();
    let buffer = device
        .newBufferWithLength_options(cases.len() * 16, MTLResourceOptions::StorageModeShared)
        .expect("Metal buffer allocation failed");
    let queue = device
        .newCommandQueue()
        .expect("Metal queue allocation failed");
    let command = queue
        .commandBuffer()
        .expect("Metal command buffer allocation failed");
    let encoder = command
        .computeCommandEncoder()
        .expect("Metal compute encoder allocation failed");
    encoder.setComputePipelineState(&pipeline);
    unsafe { encoder.setBuffer_offset_atIndex(Some(&buffer), 0, 0) };
    encoder.dispatchThreads_threadsPerThreadgroup(
        MTLSize {
            width: cases.len(),
            height: 1,
            depth: 1,
        },
        MTLSize {
            width: 1,
            height: 1,
            depth: 1,
        },
    );
    encoder.endEncoding();
    command.commit();
    command.waitUntilCompleted();
    assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
    // Shared buffer is CPU-readable after command completion; exactly len*16 bytes.
    let output = unsafe {
        std::slice::from_raw_parts(buffer.contents().as_ptr() as *const [f32; 4], cases.len())
    };
    output.to_vec()
}

#[cfg(target_os = "macos")]
#[test]
fn metal_float32_matches_cpu_reference() {
    let cases = vectors();
    let output = metal_outputs(&cases, false);
    for ((expr, expected), actual) in cases.iter().zip(output) {
        for i in 0..4 {
            assert!(
                actual[i].is_finite() && (actual[i] as f64 - expected[i]).abs() <= 3e-5,
                "{expr}: actual={actual:?}, expected={expected:?}"
            );
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn metal_adobe_transfer_matches_decimal_oracle_with_fast_math_on_and_off() {
    let mut cases = Vec::new();
    for &(input, decode, encode) in adobe_reference::TRANSFER_F32 {
        for sign in [-1., 1.] {
            let value = input as f64 * sign;
            cases.push((
                format!("vec4(gamut_decode(vec3<f32>({value:?}),3u),1.0)"),
                [decode * sign, decode * sign, decode * sign, 1.],
            ));
            cases.push((
                format!("vec4(gamut_encode(vec3<f32>({value:?}),3u),1.0)"),
                [encode * sign, encode * sign, encode * sign, 1.],
            ));
        }
    }
    for fast_math in [false, true] {
        let output = metal_outputs(&cases, fast_math);
        let mut worst_relative = 0_f64;
        for ((expr, expected), actual) in cases.iter().zip(output) {
            for i in 0..4 {
                let error = (actual[i] as f64 - expected[i]).abs();
                // Separate acceptance criteria for f32 pow and the compiler's
                // faster approximation. Both are relative: a fixed SDR-sized
                // absolute floor would let values disappear near black.
                // 8 eps = 0.96 ppm; 64 eps = 7.63 ppm. The latter is still well
                // below FP16's worst-case normal rounding error (488 ppm).
                let epsilons = if fast_math { 64. } else { 8. };
                let budget = expected[i].abs() * epsilons * f32::EPSILON as f64
                    + 8. * f32::from_bits(1) as f64;
                assert!(
                    actual[i].is_finite() && error <= budget,
                    "fast_math={fast_math}, {expr}: {actual:?} != {expected:?}, error={error:e}, budget={budget:e}"
                );
                if expected[i] != 0. {
                    worst_relative = worst_relative.max(error / expected[i].abs());
                }
                if expected[i] == 0. || expected[i].abs() == 1. {
                    assert_eq!(
                        actual[i] as f64, expected[i],
                        "transfer endpoints must be exact"
                    );
                }
            }
        }
        eprintln!(
            "Adobe GPU transfer: fast_math={fast_math}, cases={}, worst relative error={worst_relative:e}",
            cases.len()
        );
    }
}
