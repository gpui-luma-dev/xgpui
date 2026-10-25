struct Parameters { color: vec4<f32>, effect: vec4<f32>, }
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let radius = max(p.effect.x, 0.);
    let half_size = max(input.size * 0.5 - vec2(12.), vec2(0.));
    let q = abs(input.position - input.size * 0.5) - half_size + radius;
    let distance = min(max(q.x, q.y), 0.) + length(max(q, vec2(0.))) - radius;
    let glow = exp(-abs(distance) * 0.28) * (0.65 + 0.35 * sin(p.effect.y));
    return vec4(p.color.rgb, p.color.a * glow * p.effect.z);
}
