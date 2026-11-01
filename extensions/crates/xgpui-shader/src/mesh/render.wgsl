// SPDX-License-Identifier: MIT
// Hermite mesh adapted from patskovn/MeshGradient. See ../../LICENSE-MeshGradient.
struct MeshVertex { position: vec4<f32>, color: vec4<f32> }
@group(0) @binding(0) var<storage,read> vertices: array<MeshVertex>;
struct Raster { @builtin(position) position: vec4<f32>, @location(0) color: vec3<f32> }
@vertex fn mesh_vertex(@builtin(vertex_index) id: u32) -> Raster {
    let v = vertices[id];
    return Raster(vec4(v.position.x*2.-1.,1.-v.position.y*2.,0.,1.), v.color.rgb);
}
@fragment fn mesh_fragment(input: Raster) -> @location(0) vec4<f32> {
    return vec4(clamp(input.color,vec3(0.),vec3(1.)),1.);
}
