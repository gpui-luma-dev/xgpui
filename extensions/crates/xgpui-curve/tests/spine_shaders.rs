//! The WGSL and MSL spine helpers agree with the f64 CPU projection.
#![cfg(feature = "spine")]

use xgpui_curve::kurbo::{ParamCurve, Point};
use xgpui_curve::measure::quad_arclen;
use xgpui_curve::spine::{CURVE_SPINE_MSL, CURVE_SPINE_WGSL, CurveSpine};

/// (point, t_min, t_max, spine) cases, packed three `vec4`s per case.
fn cases() -> Vec<(Point, f32, f32, CurveSpine)> {
    let spines = [
        CurveSpine::new(Point::new(0., 0.), Point::new(0.5, 0.8), Point::new(1., 0.)).unwrap(),
        CurveSpine::new(Point::new(0.2, 0.3), Point::new(0.9, 0.1), Point::new(0.4, 0.9)).unwrap(),
        CurveSpine::with_bend(Point::new(0.1, 0.1), Point::new(0.9, 0.6), 0.2),
        // Degenerate: straight (the default "no bend" state), collinear back-tracking, a point.
        CurveSpine::straight(Point::new(0., 0.5), Point::new(1., 0.5)),
        CurveSpine::new(Point::new(0., 0.), Point::new(2., 0.), Point::new(1., 0.)).unwrap(),
        CurveSpine::straight(Point::new(0.5, 0.5), Point::new(0.5, 0.5)),
        CurveSpine::new(Point::new(0., 0.), Point::new(1., 1.), Point::new(1., 1.)).unwrap(),
    ];
    let mut out = Vec::new();
    for spine in spines {
        for (lo, hi) in [(0., 1.), (0.2, 0.8)] {
            for i in 0..9 {
                for j in 0..9 {
                    let p = Point::new(-0.2 + i as f64 * 0.175, -0.3 + j as f64 * 0.175);
                    out.push((p, lo, hi, spine));
                }
            }
        }
    }
    out
}

fn packed(cases: &[(Point, f32, f32, CurveSpine)]) -> Vec<[f32; 4]> {
    cases
        .iter()
        .flat_map(|(p, lo, hi, s)| {
            let k = s.shader_params(*lo, *hi);
            [[p.x as f32, p.y as f32, *lo, *hi], [k[0], k[1], k[2], k[3]], [k[4], k[5], 0., 0.]]
        })
        .collect()
}

fn wgsl() -> String {
    format!(
        "{CURVE_SPINE_WGSL}
@group(0) @binding(0) var<storage, read_write> output: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> input: array<vec4<f32>>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let a = input[id.x * 3u];
    let b = input[id.x * 3u + 1u];
    let c = input[id.x * 3u + 2u];
    output[id.x] = curve_spine_project(a.xy, b.xy, b.zw, c.xy, a.z, a.w);
}}"
    )
}

fn validated(source: &str) -> (naga::Module, naga::valid::ModuleInfo) {
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .unwrap();
    (module, info)
}

#[test]
fn wgsl_translates_to_all_native_shader_languages() {
    let (module, info) = validated(&wgsl());
    naga::back::msl::write_string(&module, &info, &Default::default(), &Default::default()).unwrap();
    let mut hlsl = String::new();
    naga::back::hlsl::Writer::new(&mut hlsl, &Default::default(), &Default::default())
        .write(&module, &info, None)
        .unwrap();
    naga::back::spv::write_vec(&module, &info, &Default::default(), None).unwrap();
}

/// Check one GPU result against the CPU, allowing for f32 precision and for
/// ties between equidistant spine points (t is checked through its distance).
fn check(case: &(Point, f32, f32, CurveSpine), gpu: [f32; 4], label: &str) {
    let (p, lo, hi, spine) = case;
    let cpu = spine.project_range(*p, *lo as f64, *hi as f64);
    let [distance, t, s, total] = gpu.map(f64::from);
    assert!(gpu.iter().all(|v| v.is_finite()), "{label}: non-finite {gpu:?} for {case:?}");
    assert!((distance.abs() - cpu.distance.abs()).abs() <= 2e-4, "{label}: distance {gpu:?} vs {cpu:?} for {case:?}");
    assert!(t >= *lo as f64 - 1e-6 && t <= *hi as f64 + 1e-6, "{label}: t out of range {gpu:?}");
    let at_t = spine.quad().eval(t).distance(*p);
    assert!((at_t - cpu.distance.abs()).abs() <= 3e-4, "{label}: t {gpu:?} is not nearest; cpu {cpu:?} for {case:?}");
    if cpu.distance.abs() > 1e-3 && (t - cpu.t).abs() < 1e-2 {
        assert_eq!(distance.signum(), cpu.distance.signum(), "{label}: side {gpu:?} vs {cpu:?} for {case:?}");
    }
    let length = spine.length();
    assert!((total - length).abs() <= 1e-5 * (1. + length), "{label}: length {total} vs {length}");
    let expected_s = if length > 0. { quad_arclen(spine.quad(), 0., t) / length } else { 0. };
    assert!((s - expected_s).abs() <= 3e-4, "{label}: s {s} vs {expected_s} for {case:?}");
}

#[cfg(target_os = "macos")]
fn run_metal(msl: &str, entry: &str, input: &[[f32; 4]], count: usize, fast_math: bool) -> Vec<[f32; 4]> {
    use objc2_foundation::NSString;
    use objc2_metal::*;
    use std::ptr::NonNull;
    let device = MTLCreateSystemDefaultDevice().expect("Metal device required for native validation");
    let options = MTLCompileOptions::new();
    #[allow(deprecated)]
    options.setFastMathEnabled(fast_math);
    let library = device
        .newLibraryWithSource_options_error(&NSString::from_str(msl), Some(&options))
        .unwrap_or_else(|e| panic!("{e:?}\n{msl}"));
    let function = library.newFunctionWithName(&NSString::from_str(entry)).expect("Metal function missing");
    let pipeline = device.newComputePipelineStateWithFunction_error(&function).unwrap();
    let output = device
        .newBufferWithLength_options(count * 16, MTLResourceOptions::StorageModeShared)
        .expect("Metal buffer allocation failed");
    // SAFETY: `input` outlives the copy made by newBufferWithBytes.
    let input_buffer = unsafe {
        device.newBufferWithBytes_length_options(
            NonNull::new(input.as_ptr() as *mut std::ffi::c_void).unwrap(),
            input.len() * 16,
            MTLResourceOptions::StorageModeShared,
        )
    }
    .expect("Metal buffer allocation failed");
    let queue = device.newCommandQueue().expect("Metal queue allocation failed");
    let command = queue.commandBuffer().expect("Metal command buffer allocation failed");
    let encoder = command.computeCommandEncoder().expect("Metal compute encoder allocation failed");
    encoder.setComputePipelineState(&pipeline);
    unsafe {
        encoder.setBuffer_offset_atIndex(Some(&output), 0, 0);
        encoder.setBuffer_offset_atIndex(Some(&input_buffer), 0, 1);
    }
    encoder.dispatchThreads_threadsPerThreadgroup(
        MTLSize { width: count, height: 1, depth: 1 },
        MTLSize { width: 1, height: 1, depth: 1 },
    );
    encoder.endEncoding();
    command.commit();
    command.waitUntilCompleted();
    assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
    // SAFETY: the shared buffer holds exactly `count` float4 values after completion.
    unsafe { std::slice::from_raw_parts(output.contents().as_ptr() as *const [f32; 4], count) }.to_vec()
}

#[cfg(target_os = "macos")]
#[test]
fn metal_matches_cpu_from_wgsl_and_hand_written_msl() {
    let cases = cases();
    let input = packed(&cases);

    let (module, info) = validated(&wgsl());
    let mut options = naga::back::msl::Options::default();
    let mut resources = naga::back::msl::EntryPointResources::default();
    for (binding, buffer) in [(0, 0), (1, 1)] {
        resources.resources.insert(
            naga::ResourceBinding { group: 0, binding },
            naga::back::msl::BindTarget { buffer: Some(buffer), ..Default::default() },
        );
    }
    options.per_entry_point_map.insert("main".into(), resources);
    let (from_wgsl, reflection) = naga::back::msl::write_string(&module, &info, &options, &Default::default()).unwrap();
    let entry = reflection.entry_point_names[0].as_ref().unwrap().clone();

    let hand = format!(
        "#include <metal_stdlib>
{CURVE_SPINE_MSL}
kernel void spine_main(device float4* output [[buffer(0)]], device const float4* input [[buffer(1)]], uint id [[thread_position_in_grid]]) {{
    float4 a = input[id * 3];
    float4 b = input[id * 3 + 1];
    float4 c = input[id * 3 + 2];
    output[id] = curve_spine_project(a.xy, b.xy, b.zw, c.xy, a.z, a.w);
}}"
    );

    for fast_math in [false, true] {
        for (label, source, name) in [("wgsl", from_wgsl.as_str(), entry.as_str()), ("msl", hand.as_str(), "spine_main")] {
            let results = run_metal(source, name, &input, cases.len(), fast_math);
            for (case, gpu) in cases.iter().zip(results) {
                check(case, gpu, &format!("{label} fast_math={fast_math}"));
            }
        }
    }
}
