struct Parameters {
    first: vec4<f32>, second: vec4<f32>, third: vec4<f32>,
    stops: vec4<f32>, gradient: vec4<f32>, radii: vec4<f32>,
    widths: vec4<f32>, options: vec4<f32>,
}
fn associated(c: vec4<f32>) -> vec4<f32> { return vec4(c.rgb * c.a, c.a); }
fn interpolate(a: vec4<f32>, b: vec4<f32>, start: f32, end: f32, t: f32) -> vec4<f32> {
    let fraction = clamp((t-start) / (end-start), 0., 1.);
    return mix(associated(a), associated(b), fraction);
}
fn straight(color: vec4<f32>) -> vec4<f32> {
    if (color.a <= 0.) { return vec4(0.); }
    return vec4(color.rgb / color.a, color.a);
}
fn freeform_color(uv: vec2<f32>, p: Parameters) -> vec4<f32> {
    if (p.options.x == -1.) { return straight(associated(p.first)); }
    let a = uv-p.stops.xy;
    let b = uv-p.stops.zw;
    let c = uv-p.gradient.xy;
    let distances = vec3(dot(a,a),dot(b,b),dot(c,c));
    if (distances.x == 0.) { return straight(associated(p.first)); }
    if (distances.y == 0.) { return straight(associated(p.second)); }
    if (p.options.x == -3. && distances.z == 0.) { return straight(associated(p.third)); }
    var nearest = min(distances.x,distances.y);
    if (p.options.x == -3.) { nearest = min(nearest,distances.z); }
    // Relative weights avoid overflowing 1/distance at a control point.
    let wa = nearest/distances.x;
    let wb = nearest/distances.y;
    var wc = 0.;
    if (p.options.x == -3.) { wc = nearest/distances.z; }
    return straight((associated(p.first)*wa + associated(p.second)*wb + associated(p.third)*wc)/(wa+wb+wc));
}
fn mesh_color(uv: vec2<f32>, p: Parameters) -> vec4<f32> {
    let t = clamp(uv,vec2(0.),vec2(1.));
    // Hermite endpoint weights (the color derivatives are zero), shared with
    // the existing mesh evaluator. A regular position patch maps u,v to x,y.
    let h = t*t*(3.-2.*t);
    let top = mix(associated(p.first),associated(p.second),h.x);
    let bottom = mix(associated(p.third),associated(p.stops),h.x);
    return straight(mix(top,bottom,h.y));
}
fn base_color(input: ShaderInput, p: Parameters) -> vec4<f32> {
    if (p.options.x == -4.) { return mesh_color(input.uv,p); }
    if (p.options.x < 0.) { return freeform_color(input.uv,p); }
    if (p.stops.w == 0.) { return p.first; }
    var t = 0.;
    if (p.stops.w == 1.) {
        let direction = p.gradient.xy;
        let extent = dot(abs(direction), input.size);
        t = 0.5 + dot(input.position-input.size*0.5, direction) / max(extent, 0.000001);
    } else {
        t = length((input.uv-p.gradient.xy) / p.gradient.zw);
    }
    var color = associated(p.first);
    if (t >= p.stops.x) {
        color = associated(p.second);
        if (t < p.stops.y) { color = interpolate(p.first, p.second, p.stops.x, p.stops.y, t); }
        if (p.options.x == 3. && t >= p.stops.y) {
            color = associated(p.third);
            if (t < p.stops.z) { color = interpolate(p.second, p.third, p.stops.y, p.stops.z, t); }
        }
    }
    // A transparent result has canonical zero RGB; no arbitrary alpha cutoff.
    if (color.a <= 0.) { return vec4(0.); }
    return vec4(color.rgb / color.a, color.a);
}
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let half_size = input.size * 0.5;
    let local = input.position - half_size;
    let radius = select(select(p.radii.z, p.radii.w, local.x < 0.), select(p.radii.y, p.radii.x, local.x < 0.), local.y < 0.);
    let corner = abs(local) - half_size;
    let rounded = corner + vec2(radius);
    let outer = length(max(rounded, vec2(0.))) + min(max(rounded.x, rounded.y), 0.) - radius;
    let aa = 0.5 / input.scale_factor;
    var coverage = clamp(0.5-outer*input.scale_factor, 0., 1.);
    if (p.options.y == 1.) {
        if (all(p.widths == vec4(0.))) { return vec4(0.); }
        let border = vec2(select(p.widths.y, p.widths.w, local.x < 0.), select(p.widths.z, p.widths.x, local.y < 0.));
        let reduced = select(border, vec2(-aa), border == vec2(0.));
        let inner_corner = corner + reduced;
        var inner = 0.;
        // Same solid-border geometry as GPUI's native quad, including its
        // nearest-ellipse approximation for unequal adjacent widths.
        if (rounded.x <= 0. || rounded.y <= 0.) {
            inner = -max(inner_corner.x, inner_corner.y);
        } else if (inner_corner.x > 0. || inner_corner.y > 0.) {
            inner = -1. / input.scale_factor;
        } else if (reduced.x == reduced.y) {
            inner = -(outer + reduced.x);
        } else {
            let ellipse = max(vec2(0.000001), vec2(radius)-reduced);
            inner = (length(rounded/ellipse)-1.) * (ellipse.x+ellipse.y) * -0.5;
        }
        // Native quad interpolation fades both RGB and alpha against a
        // transparent background at the inner edge. Preserve that effective
        // coverage without changing the existing GPUI border contract.
        let inner_coverage = clamp(0.5-inner*input.scale_factor, 0., 1.);
        coverage *= inner_coverage * inner_coverage;
    }
    let color = brush_color(input, p);
    return vec4(color.rgb, color.a * coverage);
}
