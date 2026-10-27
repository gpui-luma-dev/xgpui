#include <metal_stdlib>
using namespace metal;
struct Params { float4 bounds; float4 mask; float4 radii; float4 viewport; float4 color_r; float4 color_g; float4 color_b; float4 vignette; float4 linear; };
struct Vertex { float4 position [[position]]; float2 uv; };
vertex Vertex gamut_vertex(uint id [[vertex_id]], constant Params &p [[buffer(0)]]) {
    const float2 v[] = {{0,0},{1,0},{0,1},{0,1},{1,0},{1,1}};
    float2 uv = v[id], xy = p.bounds.xy + uv*p.bounds.zw;
    return {float4(xy.x/p.viewport.x*2-1, 1-xy.y/p.viewport.y*2,0,1),uv};
}
float3 decode_srgb(float3 c) {
    return select(c/12.92, pow(max((c+0.055)/1.055,0.0),float3(2.4)), c>0.04045);
}
float3 encode_srgb(float3 c) {
    c = clamp(c,0.0,1.0);
    return select(c*12.92, 1.055*pow(c,float3(1.0/2.4))-0.055, c>0.0031308);
}
float3 srgb_to_p3(float3 c) {
    return float3(dot(c,float3(.822461969,.177538031,0)),
                  dot(c,float3(.033194199,.966805801,0)),
                  dot(c,float3(.017082631,.072397441,.910519929)));
}
float3 p3_to_srgb(float3 c) {
    return float3(dot(c,float3(1.224940176,-.224940176,0)),
                  dot(c,float3(-.042056955,1.042056955,0)),
                  dot(c,float3(-.019637555,-.078636046,1.098273600)));
}
fragment float4 gamut_fragment(Vertex in [[stage_in]], constant Params &p [[buffer(0)]],
 texture2d<float> image [[texture(0)]], texture2d<float> mask [[texture(1)]]) {
    float2 xy=in.position.xy;
    if(any(xy<p.mask.xy)||any(xy>=p.mask.xy+p.mask.zw)) discard_fragment();
    constexpr sampler s(coord::normalized,address::clamp_to_edge,filter::linear,mip_filter::linear);
    float4 c;
    if(p.viewport.w > .5) { // Legacy scratch: exact texel read, associated encoded sRGB.
        c=image.read(uint2(xy));
        c.rgb=c.a>0 ? srgb_to_p3(decode_srgb(c.rgb/c.a))*c.a : float3(0);
    } else { // Explicit private wide contract: associated linear P3.
        c=image.sample(s,in.uv);
        if (p.linear.x > .5) {
            float3 rgb=c.a>0 ? c.rgb/c.a : float3(0);
            if (p.linear.y < .5) rgb=p3_to_srgb(rgb);
            float4 v=float4(rgb,1);
            rgb=float3(dot(p.color_r,v),dot(p.color_g,v),dot(p.color_b,v));
            float2 dims=float2(image.get_width(),image.get_height());
            float2 q=in.uv*2.-1.;
            float d=mix(length(q),length(q*dims/min(dims.x,dims.y)),p.vignette.w);
            float t=clamp((d-p.vignette.y)/p.vignette.z,0.,1.);
            rgb*=1.-p.vignette.x*t*t*(3.-2.*t);
            if (p.linear.y < .5) rgb=srgb_to_p3(rgb);
            c.rgb=clamp(rgb,0.,1.)*c.a;
        }
    }
    float2 local=in.uv*p.bounds.zw;
    bool right=local.x>p.bounds.z*.5, bottom=local.y>p.bounds.w*.5;
    float r=bottom?(right?p.radii.z:p.radii.w):(right?p.radii.y:p.radii.x);
    float2 q=abs(local-p.bounds.zw*.5)-p.bounds.zw*.5+r;
    float d=length(max(q,float2(0)))+min(max(q.x,q.y),0.0)-r;
    float coverage=clamp(.5-d,0.0,1.0);
    if(p.viewport.w < -.5) coverage=mask.sample(s,in.uv).r;
    return c*(coverage*p.viewport.z);
}
// Explicit clipped sRGB preview/readback, never a wide-gamut export.
fragment float4 gamut_preview(Vertex in [[stage_in]], texture2d<float> image [[texture(0)]]) {
    float4 c=image.read(uint2(in.position.xy));
    float3 rgb=c.a>0 ? encode_srgb(p3_to_srgb(c.rgb/c.a))*c.a : float3(0);
    return float4(rgb,c.a);
}
