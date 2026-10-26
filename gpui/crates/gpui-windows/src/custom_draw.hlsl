cbuffer Params : register(b0) {
    float4 bounds;
    float4 mask;
    float4 radii;
    float4 viewport;
    float4 color_r;
    float4 color_g;
    float4 color_b;
    float4 vignette;
    float4 linear;
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
float decode_srgb(float c) { return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4); }
float encode_srgb(float c) { return c <= 0.0031308 ? 12.92 * c : 1.055 * pow(c, 1.0 / 2.4) - 0.055; }
float3 p3_to_srgb(float3 c) {
    return float3(dot(c,float3(1.22494018,-0.22494018,0.)),dot(c,float3(-0.04205695,1.04205695,0.)),dot(c,float3(-0.01963755,-0.07863605,1.09827360)));
}
float3 srgb_to_p3(float3 c) {
    return float3(dot(c,float3(0.82246197,0.17753803,0.)),dot(c,float3(0.03319420,0.96680580,0.)),dot(c,float3(0.01708263,0.07239744,0.91051993)));
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
        if (linear.y > 1.5 && linear.x > .5) {
            float3 rgb = color.a > 0 ? color.rgb / color.a : float3(0,0,0);
            if (linear.z < .5) rgb = p3_to_srgb(rgb);
            float4 v = float4(rgb,1);
            rgb = float3(dot(color_r,v),dot(color_g,v),dot(color_b,v));
            uint width,height; image.GetDimensions(width,height);
            float2 dims=float2(width,height), q=input.uv*2.-1.;
            float d=lerp(length(q),length(q*dims/min(dims.x,dims.y)),vignette.w);
            float t=clamp((d-vignette.y)/vignette.z,0.,1.);
            rgb*=1.-vignette.x*t*t*(3.-2.*t);
            if (linear.z < .5) rgb=srgb_to_p3(rgb);
            color.rgb=clamp(rgb,0.,1.)*color.a;
        }
        // Associated linear P3 -> scRGB. Preserve extended channels.
        color.rgb = float3(
            dot(color.rgb, float3(1.22494018,-0.22494018,0)),
            dot(color.rgb, float3(-0.04205695,1.04205695,0)),
            dot(color.rgb, float3(-0.01963755,-0.07863605,1.09827360)));
        return color * (coverage * viewport.z);
    }
    // The legacy native background adapter produces premultiplied pixels.
    if ((uint(viewport.w) & 2) != 0 && color.a > 0) color.rgb /= color.a;
    bool processing = linear.x > 0.5 || linear.y > 0.5;
    if (linear.y > 0.5) {
        color.rgb = color.a > 0.0 ? color.rgb / color.a : float3(0.0, 0.0, 0.0);
        if (linear.z < 0.5) color.rgb = p3_to_srgb(color.rgb);
    } else if (processing) {
        float3 c = clamp(color.rgb, 0.0, 1.0);
        color.rgb = float3(decode_srgb(c.r), decode_srgb(c.g), decode_srgb(c.b));
        if (linear.z > 0.5) color.rgb = srgb_to_p3(color.rgb);
    }
    float4 rgb1 = float4(color.rgb, 1.0);
    color.rgb = float3(dot(color_r,rgb1), dot(color_g,rgb1), dot(color_b,rgb1));
    if (processing) {
        uint width, height; image.GetDimensions(width, height);
        float2 dims = float2(width, height);
        float2 q = input.uv * 2.0 - 1.0;
        float ellipse = length(q);
        float circle = length(q * dims / min(dims.x, dims.y));
        float d = ellipse + vignette.w * (circle - ellipse);
        float t = clamp((d - vignette.y) / vignette.z, 0.0, 1.0);
        float factor = 1.0 - vignette.x * t*t*(3.0 - 2.0*t);
        float3 rgb = color.rgb * factor;
        if (linear.z > 0.5) rgb = p3_to_srgb(rgb);
        float3 c = clamp(rgb, 0.0, 1.0);
        color.rgb = float3(encode_srgb(c.r), encode_srgb(c.g), encode_srgb(c.b));
        if (color.a == 0.0) color.rgb = float3(0.0, 0.0, 0.0);
    }
    return float4(color.rgb, color.a * coverage * viewport.z);
}
