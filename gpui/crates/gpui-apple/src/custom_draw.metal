#include <metal_stdlib>
using namespace metal;
struct Params { float4 bounds; float4 mask; float4 radii; float4 viewport; float4 color_r; float4 color_g; float4 color_b; float4 yuv_r; float4 yuv_g; float4 yuv_b; };
struct Vertex { float4 position [[position]]; float2 uv; };
vertex Vertex custom_vertex(uint id [[vertex_id]], constant Params &p [[buffer(0)]]) {
    const float2 vertices[] = {{0,0},{1,0},{0,1},{0,1},{1,0},{1,1}};
    float2 uv = vertices[id];
    float2 xy = p.bounds.xy + uv * p.bounds.zw;
    return {float4(xy.x / p.viewport.x * 2 - 1, 1 - xy.y / p.viewport.y * 2, 0, 1), uv};
}
fragment float4 custom_fragment(Vertex in [[stage_in]], constant Params &p [[buffer(0)]], texture2d<float> image [[texture(0)]], texture2d<float> mask [[texture(1)]], texture2d<float> chroma [[texture(2)]]) {
    float2 xy = in.position.xy;
    if (any(xy < p.mask.xy) || any(xy >= p.mask.xy + p.mask.zw)) discard_fragment();
    float2 local = in.uv * p.bounds.zw;
    bool right = local.x > p.bounds.z * 0.5;
    bool bottom = local.y > p.bounds.w * 0.5;
    float r = bottom ? (right ? p.radii.z : p.radii.w) : (right ? p.radii.y : p.radii.x);
    float2 q = abs(local - p.bounds.zw * 0.5) - p.bounds.zw * 0.5 + r;
    float distance = length(max(q, float2(0))) + min(max(q.x, q.y), 0.0) - r;
    float coverage = clamp(0.5 - distance, 0.0, 1.0);
    constexpr sampler s(coord::normalized, address::clamp_to_edge, filter::linear, mip_filter::linear);
    if (fmod(p.viewport.w, 2.0) > 0.5) coverage = mask.sample(s, in.uv).r;
    float4 color;
    if (p.viewport.w >= 4.0) {
        float4 yuv = float4(image.sample(s, in.uv).r, chroma.sample(s, in.uv).rg, 1.0);
        color = float4(clamp(float3(dot(p.yuv_r, yuv), dot(p.yuv_g, yuv), dot(p.yuv_b, yuv)), 0.0, 1.0), 1.0);
    } else {
        color = image.sample(s, in.uv);
    }
    if (fmod(floor(p.viewport.w / 2.0), 2.0) > 0.5 && color.a > 0.0) color.rgb /= color.a;
    float4 rgb1 = float4(color.rgb, 1.0);
    color.rgb = float3(dot(p.color_r, rgb1), dot(p.color_g, rgb1), dot(p.color_b, rgb1));
    return float4(color.rgb, color.a * coverage * p.viewport.z);
}
