struct Parameters {
    shape: vec4<f32>, metrics: vec4<f32>, direction: vec4<f32>, stops: vec4<f32>,
    color0: vec4<f32>, color1: vec4<f32>, color2: vec4<f32>, color3: vec4<f32>,
}
fn rounded_distance(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = clamp(radius, 0., max(0., min(half_size.x, half_size.y)));
    let q = abs(point) - half_size + vec2(r);
    return min(max(q.x,q.y),0.) + length(max(q,vec2(0.))) - r;
}
fn transition(a: vec4<f32>, b: vec4<f32>, low: f32, high: f32, t: f32) -> vec4<f32> {
    // Duplicate positions are hard stops; never divide by zero.
    if (high <= low) { return b; }
    return mix(a,b,clamp((t-low)/(high-low),0.,1.));
}
fn ramp(t: f32, p: Parameters) -> vec4<f32> {
    if (t < p.stops.x) { return p.color0; }
    if (t < p.stops.y) { return transition(p.color0,p.color1,p.stops.x,p.stops.y,t); }
    if (t < p.stops.z) { return transition(p.color1,p.color2,p.stops.y,p.stops.z,t); }
    if (t < p.stops.w) { return transition(p.color2,p.color3,p.stops.z,p.stops.w,t); }
    return p.color3;
}
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let half_size = p.shape.zw * 0.5;
    let center = p.shape.xy + half_size;
    let local = input.position - center;
    let aa = 1. / input.scale_factor;
    let receiver_distance = rounded_distance(local, half_size, p.metrics.x);
    let receiver = 1. - smoothstep(-aa, aa, receiver_distance);
    let inset = p.direction.w > 0.5;
    let spread = select(p.metrics.z, -p.metrics.z, inset);
    let expanded_half = half_size + vec2(spread);
    let band = max(p.metrics.y, aa);
    var shape_coverage = 0.;
    if (all(expanded_half > vec2(0.))) {
        let distance = rounded_distance(local - p.direction.xy, expanded_half, p.metrics.x + spread);
        shape_coverage = 1. - smoothstep(-band, band, distance);
    }
    let coverage = pow(clamp(select(shape_coverage,1.-shape_coverage,inset),0.,1.),p.metrics.w)
        * select(1.-receiver,receiver,inset);
    let direction = vec2(sin(p.direction.z), -cos(p.direction.z));
    let denominator = max(dot(abs(direction),p.shape.zw),0.0001);
    let t = clamp(dot(local,direction)/denominator + 0.5,0.,1.);
    let color = ramp(t,p);
    let rgb = color.rgb / max(color.a,0.000001);
    return vec4(rgb,color.a * coverage);
}
