struct Parameters { shape: vec4<f32>, metrics: vec4<f32>, track: vec4<f32>, first: vec4<f32>, second: vec4<f32> }
fn distance_to_box(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = clamp(radius, 0., min(half_size.x, half_size.y));
    let q = abs(p) - half_size + vec2(r);
    return min(max(q.x,q.y),0.) + length(max(q,vec2(0.))) - r;
}
fn premultiply(c: vec4<f32>) -> vec4<f32> { return vec4(c.rgb * c.a,c.a); }
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    if (p.metrics.y <= 0.) { return vec4(0.); }
    let half_size = p.shape.xy * 0.5;
    let local = input.uv * input.size - vec2(p.shape.z) - half_size;
    let d = distance_to_box(local,half_size,p.metrics.x);
    let aa = 0.5 / input.scale_factor;
    let coverage = (1.-smoothstep(-aa,aa,d)) * smoothstep(-p.metrics.y-aa,-p.metrics.y+aa,d);
    let angle = fract(atan2(local.y,local.x) / 6.28318530718 - p.metrics.z + 1.);
    // Wrapped Gaussian lobes remove the hard angular edge of a spinning wedge.
    let da = min(angle,1.-angle);
    let opposite = fract(angle+0.5);
    let db = min(opposite,1.-opposite);
    let sigma = max(p.metrics.w * 0.25,0.001);
    let a = exp(-0.5 * da * da / (sigma*sigma));
    let b = exp(-0.5 * db * db / (sigma*sigma));
    let radial = (d + p.metrics.y * 0.5) / max(p.shape.w,aa);
    let halo = exp(-0.5 * radial * radial);
    let soft_coverage = select(coverage,halo,p.shape.w > 0.);
    let base = premultiply(p.track) * coverage;
    let highlights = premultiply(p.first) * a + premultiply(p.second) * b;
    let color = base * (1.-max(a,b)) + highlights * soft_coverage;
    return vec4(color.rgb / max(color.a,0.000001),clamp(color.a,0.,1.));
}
