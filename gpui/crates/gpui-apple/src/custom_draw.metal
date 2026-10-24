#include <metal_stdlib>
using namespace metal;
struct Params { float4 bounds; float4 mask; float4 radii; float4 viewport; };
struct Vertex { float4 position [[position]]; float2 uv; };
vertex Vertex custom_vertex(uint id [[vertex_id]], constant Params &p [[buffer(0)]]) {
    const float2 vertices[] = {{0,0},{1,0},{0,1},{0,1},{1,0},{1,1}};
    float2 uv = vertices[id];
    float2 xy = p.bounds.xy + uv * p.bounds.zw;
    return {float4(xy.x / p.viewport.x * 2 - 1, 1 - xy.y / p.viewport.y * 2, 0, 1), uv};
}
fragment float4 custom_fragment(Vertex in [[stage_in]], constant Params &p [[buffer(0)]], texture2d<float> image [[texture(0)]], texture2d<float> mask [[texture(1)]]) {
    float2 xy = in.position.xy;
    if (any(xy < p.mask.xy) || any(xy >= p.mask.xy + p.mask.zw)) discard_fragment();
    float2 local = in.uv * p.bounds.zw;
    bool right = local.x > p.bounds.z * 0.5;
    bool bottom = local.y > p.bounds.w * 0.5;
    float r = bottom ? (right ? p.radii.z : p.radii.w) : (right ? p.radii.y : p.radii.x);
    float2 q = abs(local - p.bounds.zw * 0.5) - p.bounds.zw * 0.5 + r;
    float distance = length(max(q, float2(0))) + min(max(q.x, q.y), 0.0) - r;
    float coverage = clamp(0.5 - distance, 0.0, 1.0);
    constexpr sampler s(coord::normalized, address::clamp_to_edge, filter::linear);
    if (fmod(p.viewport.w, 2.0) > 0.5) coverage = mask.sample(s, in.uv).r;
    float4 color = image.sample(s, in.uv);
    if (p.viewport.w > 1.5 && color.a > 0.0) color.rgb /= color.a;
    return float4(color.rgb, color.a * coverage * p.viewport.z);
}
