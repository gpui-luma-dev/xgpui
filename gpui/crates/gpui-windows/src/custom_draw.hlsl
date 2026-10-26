cbuffer Params : register(b0) {
    float4 bounds;
    float4 mask;
    float4 radii;
    float4 viewport;
    float4 color_r;
    float4 color_g;
    float4 color_b;
}
Texture2D<float4> image : register(t0);
Texture2D<float4> shape_mask : register(t1);
SamplerState paint_sampler : register(s0);

struct Vertex {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};
Vertex custom_vertex(uint id : SV_VertexID) {
    float2 vertices[6] = {
        float2(0, 0), float2(1, 0), float2(0, 1),
        float2(0, 1), float2(1, 0), float2(1, 1)
    };
    Vertex result;
    result.uv = vertices[id];
    float2 xy = bounds.xy + result.uv * bounds.zw;
    result.position = float4(xy.x / viewport.x * 2 - 1, 1 - xy.y / viewport.y * 2, 0, 1);
    return result;
}
float4 custom_fragment(Vertex input) : SV_Target {
    float2 xy = input.position.xy;
    if (any(xy < mask.xy) || any(xy >= mask.xy + mask.zw)) discard;

    float2 local = input.uv * bounds.zw;
    bool right = local.x > bounds.z * .5;
    bool bottom = local.y > bounds.w * .5;
    float radius = bottom ? (right ? radii.z : radii.w) : (right ? radii.y : radii.x);
    float2 q = abs(local - bounds.zw * .5) - bounds.zw * .5 + radius;
    float distance = length(max(q, float2(0, 0))) + min(max(q.x, q.y), 0) - radius;
    float coverage = saturate(.5 - distance);
    // Native geometry supplies exact border/path coverage instead of rounded fill coverage.
    if ((uint(viewport.w) & 1) != 0) coverage = shape_mask.Sample(paint_sampler, input.uv).r;

    float4 color = image.Sample(paint_sampler, input.uv);
    if ((uint(viewport.w) & 4) != 0) {
        // Associated linear P3 -> scRGB. Preserve extended channels.
        color.rgb = float3(
            dot(color.rgb, float3(1.22494018,-0.22494018,0)),
            dot(color.rgb, float3(-0.04205695,1.04205695,0)),
            dot(color.rgb, float3(-0.01963755,-0.07863605,1.09827360)));
        return color * (coverage * viewport.z);
    }
    // The legacy native background adapter produces premultiplied pixels.
    if ((uint(viewport.w) & 2) != 0 && color.a > 0) color.rgb /= color.a;
    float4 rgb1 = float4(color.rgb, 1);
    color.rgb = float3(dot(color_r,rgb1), dot(color_g,rgb1), dot(color_b,rgb1));
    return float4(color.rgb, color.a * coverage * viewport.z);
}
