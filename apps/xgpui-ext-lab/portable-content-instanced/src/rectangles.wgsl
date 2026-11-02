struct Parameters { color: vec4<f32>, effect: vec4<f32>, }
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let radius = clamp(p.effect.x, 0., min(input.size.x, input.size.y) * 0.5);
    let q = abs(input.position - input.size * 0.5) - input.size * 0.5 + radius;
    let distance = min(max(q.x, q.y), 0.) + length(max(q, vec2(0.))) - radius;
    // Match portable-content's native rounded-rectangle coverage in device pixels.
    let coverage = clamp(0.5 - distance * input.scale_factor, 0., 1.);
    return vec4(p.color.rgb, p.color.a * coverage);
}
