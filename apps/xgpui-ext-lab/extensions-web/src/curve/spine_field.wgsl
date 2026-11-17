// Curve-lab spine field. Prepended at build time with xgpui-curve's
// CURVE_SPINE_WGSL; mirrored on the CPU by `spine::shade` for comparison.
struct Parameters {
    // p0.xy, control.xy in pixels within the rectangle.
    a: vec4<f32>,
    // p1.xy in pixels, t_min, t_max.
    b: vec4<f32>,
    // mode (0 bands, 1 texture, 2 bent field), kernel radius, view zoom, unused.
    style: vec4<f32>,
}

fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let r = curve_spine_project(input.position, p.a.xy, p.a.zw, p.b.xy, p.b.z, p.b.w);
    // Back to unzoomed pixels so the pattern zooms with the view.
    let d = r.x / max(p.style.z, 1e-6);
    let along = r.z;
    let mode = p.style.x;
    let radius = max(p.style.y, 1.0);
    var color = vec3<f32>(0.0);
    if (mode < 0.5) {
        let side = select(vec3<f32>(0.25, 0.55, 1.0), vec3<f32>(1.0, 0.55, 0.25), d > 0.0);
        color = side * (1.0 - exp(-abs(d) / 90.0));
        color = color * (0.78 + 0.22 * cos(abs(d) * 0.35));
    } else if (mode < 1.5) {
        let cells = floor(along * 12.0) + floor(d / 22.0);
        let parity = cells - 2.0 * floor(cells * 0.5);
        let a = vec3<f32>(0.93, 0.80, 0.45);
        let b = vec3<f32>(0.18, 0.30, 0.52);
        color = mix(a, b, parity) * exp(-abs(d) / (3.0 * radius));
    } else {
        let k = (along - 0.5) / 0.6;
        let w = exp(-(d * d) / (radius * radius) - k * k);
        let hue = mix(vec3<f32>(1.0, 0.35, 0.45), vec3<f32>(0.3, 0.8, 1.0), along);
        color = mix(vec3<f32>(0.03, 0.04, 0.07), hue, w);
    }
    return vec4<f32>(color, 1.0);
}
