struct Params { bounds: vec4<f32>, mask: vec4<f32>, radii: vec4<f32>, viewport: vec4<f32>, color_r: vec4<f32>, color_g: vec4<f32>, color_b: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var paint_sampler: sampler;
@group(0) @binding(2) var image: texture_2d<f32>;
@group(0) @binding(3) var shape_mask: texture_2d<f32>;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn custom_vertex(@builtin(vertex_index) id: u32) -> Vertex {
    let vertices = array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
    let uv = vertices[id];
    let xy = p.bounds.xy + uv * p.bounds.zw;
    return Vertex(vec4(xy.x / p.viewport.x * 2. - 1., 1. - xy.y / p.viewport.y * 2., 0., 1.), uv);
}
@fragment fn custom_fragment(in: Vertex) -> @location(0) vec4<f32> {
    if any(in.position.xy < p.mask.xy) || any(in.position.xy >= p.mask.xy + p.mask.zw) { discard; }
    let local = in.uv * p.bounds.zw;
    let right = local.x > p.bounds.z * 0.5;
    let bottom = local.y > p.bounds.w * 0.5;
    let r = select(select(p.radii.x, p.radii.y, right), select(p.radii.w, p.radii.z, right), bottom);
    let q = abs(local - p.bounds.zw * 0.5) - p.bounds.zw * 0.5 + r;
    let distance = length(max(q, vec2(0.))) + min(max(q.x,q.y),0.) - r;
    var coverage = clamp(0.5 - distance,0.,1.);
    if (u32(p.viewport.w) & 1u) != 0u { coverage = textureSample(shape_mask,paint_sampler,in.uv).r; }
    var color = textureSample(image,paint_sampler,in.uv);
    if p.viewport.w > 1.5 && color.a > 0. { color = vec4(color.rgb / color.a, color.a); }
    let rgb1 = vec4(color.rgb, 1.);
    color = vec4(dot(p.color_r,rgb1), dot(p.color_g,rgb1), dot(p.color_b,rgb1), color.a);
    return vec4(color.rgb, color.a * coverage * p.viewport.z);
}
