// SPDX-License-Identifier: MIT
// Copyright (c) 2022 Nikita Patskov and others
// Adapted from patskovn/MeshGradient, revision f16b544336b7bdaf7e3eba4ac6cba50e1160220e.
// The complete upstream permission notice is in ../../LICENSE-MeshGradient.
// Changes: normalized top-left coordinates, explicit float4 layouts, patch-local
// tessellation, guarded 1D dispatch, and one batched triangle-list render.
#include <metal_stdlib>
using namespace metal;
struct ControlPoint { float4 positionAndU; float4 tangentV; float4 color; };
struct Patch { float4x4 X, Y, R, G, B; };
struct MeshVertex { float4 position; float4 color; };
struct Params { uint width, height, samples, patches; };
static_assert(sizeof(ControlPoint) == 48, "control stride");
static_assert(sizeof(Patch) == 320, "patch stride");
static_assert(sizeof(MeshVertex) == 32, "vertex stride");

static float4x4 geometryCoefficients(ControlPoint p00, ControlPoint p01,
                                     ControlPoint p10, ControlPoint p11, uint axis) {
    return float4x4 {
        {p00.positionAndU[axis], p10.positionAndU[axis], p00.positionAndU[axis+2], p10.positionAndU[axis+2]},
        {p01.positionAndU[axis], p11.positionAndU[axis], p01.positionAndU[axis+2], p11.positionAndU[axis+2]},
        {p00.tangentV[axis], p10.tangentV[axis], 0, 0},
        {p01.tangentV[axis], p11.tangentV[axis], 0, 0}
    };
}
static float4x4 colorCoefficients(float p00, float p01, float p10, float p11) {
    return float4x4 {{p00,p10,0,0},{p01,p11,0,0},{0,0,0,0},{0,0,0,0}};
}
kernel void mesh_coefficients(device const ControlPoint *points [[buffer(0)]],
                              device Patch *patches [[buffer(1)]],
                              constant Params &p [[buffer(2)]], uint id [[thread_position_in_grid]]) {
    if (id >= p.patches) return;
    uint x = id % (p.width-1), y = id / (p.width-1);
    ControlPoint p00=points[x+y*p.width], p10=points[x+1+y*p.width];
    ControlPoint p01=points[x+(y+1)*p.width], p11=points[x+1+(y+1)*p.width];
    patches[id] = {
        geometryCoefficients(p00,p01,p10,p11,0), geometryCoefficients(p00,p01,p10,p11,1),
        colorCoefficients(p00.color.r,p01.color.r,p10.color.r,p11.color.r),
        colorCoefficients(p00.color.g,p01.color.g,p10.color.g,p11.color.g),
        colorCoefficients(p00.color.b,p01.color.b,p10.color.b,p11.color.b)
    };
}
kernel void mesh_evaluate(device const Patch *patches [[buffer(0)]],
                          device MeshVertex *vertices [[buffer(1)]],
                          constant Params &p [[buffer(2)]], uint id [[thread_position_in_grid]]) {
    uint perPatch=p.samples*p.samples;
    if (id >= perPatch*p.patches) return;
    uint local=id%perPatch;
    float u=float(local%p.samples)/float(p.samples-1), v=float(local/p.samples)/float(p.samples-1);
    float4 U={u*u*u,u*u,u,1}, V={v*v*v,v*v,v,1};
    float4x4 H={{2,-3,0,1},{-2,3,0,0},{1,-2,1,0},{1,-1,0,0}};
    float4x4 HT=transpose(H);
    Patch patch=patches[id/perPatch];
    vertices[id] = {
        float4(dot(V,U*H*patch.X*HT),dot(V,U*H*patch.Y*HT),0,1),
        float4(dot(V,U*H*patch.R*HT),dot(V,U*H*patch.G*HT),dot(V,U*H*patch.B*HT),1)
    };
}
kernel void mesh_triangles(device const MeshVertex *vertices [[buffer(0)]],
                           device MeshVertex *triangles [[buffer(1)]],
                           constant Params &p [[buffer(2)]], uint id [[thread_position_in_grid]]) {
    uint cells=(p.samples-1)*(p.samples-1);
    if (id >= cells*p.patches) return;
    uint patch=id/cells, local=id%cells, x=local%(p.samples-1), y=local/(p.samples-1);
    uint a=patch*p.samples*p.samples+x+y*p.samples;
    triangles[id*6+0]=vertices[a];
    triangles[id*6+1]=vertices[a+1];
    triangles[id*6+2]=vertices[a+1+p.samples];
    triangles[id*6+3]=vertices[a+1+p.samples];
    triangles[id*6+4]=vertices[a+p.samples];
    triangles[id*6+5]=vertices[a];
}
struct Raster { float4 position [[position]]; float4 color; };
vertex Raster mesh_vertex(uint id [[vertex_id]], device const MeshVertex *vertices [[buffer(0)]]) {
    MeshVertex v=vertices[id];
    return {float4(v.position.x*2-1,1-v.position.y*2,0,1),v.color};
}
fragment float4 mesh_fragment(Raster in [[stage_in]]) { return float4(clamp(in.color.rgb,0.0,1.0),1); }
