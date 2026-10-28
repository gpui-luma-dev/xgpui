struct Parameters { color: vec4<f32>, effect: vec4<f32>, }
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let distance = length((input.uv - vec2(0.5)) * 2.);
    let alpha = pow(max(0., 1. - distance), 2.) * p.effect.z;
    return vec4(p.color.rgb, p.color.a * alpha);
}
