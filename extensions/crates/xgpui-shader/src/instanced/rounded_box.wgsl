struct Parameters { color: vec4<f32>, effect: vec4<f32>, }
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let radius = clamp(p.effect.x, 0., min(input.size.x, input.size.y) * 0.5);
    let q = abs(input.position - input.size * 0.5) - input.size * 0.5 + radius;
    let distance = min(max(q.x, q.y), 0.) + length(max(q, vec2(0.))) - radius;
    let aa = 1. / input.scale_factor;
    return vec4(p.color.rgb, p.color.a * (1. - smoothstep(-aa, aa, distance)));
}
