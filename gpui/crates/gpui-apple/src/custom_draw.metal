#include <metal_stdlib>
using namespace metal;
struct Params { float4 bounds; float4 mask; float4 radii; float4 viewport; float4 color_r; float4 color_g; float4 color_b; float4 yuv_r; float4 yuv_g; float4 yuv_b; float4 vignette; float4 linear; };
struct Vertex { float4 position [[position]]; float2 uv; };
vertex Vertex custom_vertex(uint id [[vertex_id]], constant Params &p [[buffer(0)]]) {
    const float2 vertices[] = {{0,0},{1,0},{0,1},{0,1},{1,0},{1,1}};
    float2 uv = vertices[id];
    float2 xy = p.bounds.xy + uv * p.bounds.zw;
    return {float4(xy.x / p.viewport.x * 2 - 1, 1 - xy.y / p.viewport.y * 2, 0, 1), uv};
}
float decode_srgb(float c) { return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4); }
float encode_srgb(float c) { return c <= 0.0031308 ? 12.92 * c : 1.055 * pow(c, 1.0 / 2.4) - 0.055; }
float3 p3_to_srgb(float3 c) {
    return float3(dot(c,float3(1.22494018,-0.22494018,0.)),dot(c,float3(-0.04205695,1.04205695,0.)),dot(c,float3(-0.01963755,-0.07863605,1.09827360)));
}
float3 srgb_to_p3(float3 c) {
    return float3(dot(c,float3(0.82246197,0.17753803,0.)),dot(c,float3(0.03319420,0.96680580,0.)),dot(c,float3(0.01708263,0.07239744,0.91051993)));
}
// Linear SDR Rec.2020 -> P3; retain extended channels until preview mapping.
float3 rec2020_to_p3(float3 c) {
    return float3(dot(c,float3(1.343578253,-0.282179671,-0.061398582)),dot(c,float3(-0.065297453,1.075787916,-0.010490463)),dot(c,float3(0.002821787,-0.019598495,1.016776707)));
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
    if (p.linear.y > 2.5) color.rgb = rec2020_to_p3(color.rgb);
    bool processing = p.linear.x > 0.5 || p.linear.y > 0.5;
    if (p.linear.y > 0.5) {
        color.rgb = color.a > 0.0 ? color.rgb / color.a : float3(0.0, 0.0, 0.0);
        if (p.linear.z < 0.5) color.rgb = p3_to_srgb(color.rgb);
    } else if (processing) {
        float3 c = clamp(color.rgb, 0.0, 1.0);
        color.rgb = float3(decode_srgb(c.r), decode_srgb(c.g), decode_srgb(c.b));
        if (p.linear.z > 0.5) color.rgb = srgb_to_p3(color.rgb);
    }
    float4 rgb1 = float4(color.rgb, 1.0);
    color.rgb = float3(dot(p.color_r, rgb1), dot(p.color_g, rgb1), dot(p.color_b, rgb1));
    if (processing) {
        float2 dims = float2(image.get_width(), image.get_height());
        float2 q = in.uv * 2.0 - 1.0;
        float ellipse = length(q);
        float circle = length(q * dims / min(dims.x, dims.y));
        float d = ellipse + p.vignette.w * (circle - ellipse);
        float t = clamp((d - p.vignette.y) / p.vignette.z, 0.0, 1.0);
        float factor = 1.0 - p.vignette.x * t*t*(3.0 - 2.0*t);
        float3 rgb = color.rgb * factor;
        if (p.linear.z > 0.5) rgb = p3_to_srgb(rgb);
        float3 c = clamp(rgb, 0.0, 1.0);
        color.rgb = float3(encode_srgb(c.r), encode_srgb(c.g), encode_srgb(c.b));
        if (color.a == 0.0) color.rgb = float3(0.0, 0.0, 0.0);
    }
    return float4(color.rgb, color.a * coverage * p.viewport.z);
}
