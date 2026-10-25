struct Parameters {
    first: vec4<f32>, second: vec4<f32>, axis: vec4<f32>, options: vec4<f32>, shape: vec4<f32>,
}
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let t = dot(input.uv, p.axis.xy) + p.axis.z;
    let interpolated = gamut_interpolate(p.first, p.second, t, 1u, u32(p.options.x), u32(p.options.y));
    let mapped = gamut_map(interpolated.rgb, 1u, u32(p.options.z));
    let radius = p.shape.x;
    let q = abs(input.position - input.size * 0.5) - input.size * 0.5 + radius;
    let distance = min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - radius;
    let coverage = clamp(0.5 - distance * input.scale_factor, 0.0, 1.0);
    return vec4(mapped, interpolated.a * coverage);
}
