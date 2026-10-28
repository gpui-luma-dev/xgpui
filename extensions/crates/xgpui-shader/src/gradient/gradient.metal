#include <metal_stdlib>
using namespace metal;
struct Params { float4 size_kind_count; float4 center_radius; float4 direction; };
struct Stop { float4 color; float4 offset; };
struct Vertex { float4 position [[position]]; };
vertex Vertex gradient_vertex(uint id [[vertex_id]]) {
    const float2 points[]={{-1,-1},{1,-1},{-1,1},{-1,1},{1,-1},{1,1}};
    return {float4(points[id],0,1)};
}
fragment float4 gradient_fragment(Vertex in [[stage_in]], constant Params &p [[buffer(0)]], constant Stop *stops [[buffer(1)]]) {
    float2 size=p.size_kind_count.xy;
    float2 uv=in.position.xy/size;
    float2 delta=in.position.xy-p.center_radius.xy*size;
    uint kind=uint(p.size_kind_count.z);
    float ratio=0;
    if (kind==0) {
        float2 direction=p.direction.xy;
        ratio=0.5+dot(in.position.xy-size*0.5,direction)/dot(abs(direction),size);
    } else if (kind==1) {
        ratio=length((uv-p.center_radius.xy)/p.center_radius.zw);
    } else if (kind==2) {
        if (dot(delta,delta)>0) ratio=fract((atan2(delta.y,delta.x)+M_PI_F*0.5-p.direction.z)/(2*M_PI_F)+1);
    } else {
        float2 v=(uv-p.center_radius.xy)/p.center_radius.zw;
        float2 rotated=float2(v.x*p.direction.x+v.y*p.direction.y,-v.x*p.direction.y+v.y*p.direction.x);
        ratio=abs(rotated.x)+abs(rotated.y);
    }
    uint count=uint(p.size_kind_count.w);
    if (ratio<stops[0].offset.x) return stops[0].color;
    for (uint i=1;i<count;i++) {
        if (ratio<stops[i].offset.x) {
            float t=(ratio-stops[i-1].offset.x)/(stops[i].offset.x-stops[i-1].offset.x);
            float4 a=stops[i-1].color, b=stops[i].color;
            float alpha=mix(a.a,b.a,t);
            float3 premultiplied=mix(a.rgb*a.a,b.rgb*b.a,t);
            return float4(alpha>0 ? premultiplied/alpha : float3(0),alpha);
        }
    }
    return stops[count-1].color;
}
