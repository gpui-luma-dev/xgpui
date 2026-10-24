// SPDX-License-Identifier: MIT
// Hermite mesh adapted from patskovn/MeshGradient. See ../../LICENSE-MeshGradient.
// WGSL evaluates patch vertices directly, retaining patch-local tessellation.
struct Point { position_u: vec4<f32>, tangent_v: vec4<f32>, color: vec4<f32> }
struct Params { grid: vec4<f32>, points: array<Point,256> }
@group(0) @binding(0) var<uniform> p: Params;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) color: vec3<f32> }
fn hermite(t: f32) -> vec4<f32> {
    return vec4(2.*t*t*t-3.*t*t+1.,-2.*t*t*t+3.*t*t,t*t*t-2.*t*t+t,t*t*t-t*t);
}
@vertex fn paint_vertex(@builtin(vertex_index) id: u32) -> Vertex {
    let width = u32(p.grid.x);
    let samples = u32(p.grid.z);
    let cells = (samples-1u)*(samples-1u);
    let cell_id = id/6u;
    let patch_id = cell_id/cells;
    let cell = cell_id%cells;
    let corners = array<vec2<u32>,6>(vec2(0u,0u),vec2(1u,0u),vec2(1u,1u),vec2(1u,1u),vec2(0u,1u),vec2(0u,0u));
    let xy = vec2(cell%(samples-1u),cell/(samples-1u))+corners[id%6u];
    let u = hermite(f32(xy.x)/f32(samples-1u));
    let v = hermite(f32(xy.y)/f32(samples-1u));
    let a = patch_id%(width-1u)+(patch_id/(width-1u))*width;
    let p00 = p.points[a]; let p10 = p.points[a+1u];
    let p01 = p.points[a+width]; let p11 = p.points[a+width+1u];
    let position = p00.position_u.xy*u.x*v.x+p10.position_u.xy*u.y*v.x+p01.position_u.xy*u.x*v.y+p11.position_u.xy*u.y*v.y
        + p00.position_u.zw*u.z*v.x+p10.position_u.zw*u.w*v.x+p01.position_u.zw*u.z*v.y+p11.position_u.zw*u.w*v.y
        + p00.tangent_v.xy*u.x*v.z+p10.tangent_v.xy*u.y*v.z+p01.tangent_v.xy*u.x*v.w+p11.tangent_v.xy*u.y*v.w;
    let color = p00.color.rgb*u.x*v.x+p10.color.rgb*u.y*v.x+p01.color.rgb*u.x*v.y+p11.color.rgb*u.y*v.y;
    return Vertex(vec4(position.x*2.-1.,1.-position.y*2.,0.,1.),color);
}
@fragment fn paint_fragment(in: Vertex) -> @location(0) vec4<f32> { return vec4(clamp(in.color,vec3(0.),vec3(1.)),1.); }
