struct Parameters {
    phase: vec4<f32>,
    metrics: vec4<f32>,
    color: vec4<f32>,
}

fn coverage(pixel: vec2<f32>, phase: vec2<f32>, spacing: f32, dots: bool, aa: f32) -> f32 {
    let distance = abs(fract((pixel + phase) / spacing + vec2<f32>(0.5)) - vec2<f32>(0.5)) * spacing;
    let d = select(min(distance.x, distance.y), length(distance), dots);
    let radius = select(0.5, 1.25, dots);
    return 1.0 - smoothstep(max(0.0, radius - aa), radius + aa, d);
}

fn shade(input: ShaderInput, parameters: Parameters) -> vec4<f32> {
    let pixel = input.uv * input.size;
    let aa = 0.75 / input.scale_factor;
    let dots = parameters.metrics.w > 0.5;
    let minor = coverage(pixel, parameters.phase.xy, parameters.metrics.x, dots, aa);
    let major = coverage(pixel, parameters.phase.zw, parameters.metrics.y, dots, aa);
    let alpha = max(minor * parameters.metrics.z, major * 0.65);
    return vec4<f32>(parameters.color.rgb, parameters.color.a * alpha);
}
