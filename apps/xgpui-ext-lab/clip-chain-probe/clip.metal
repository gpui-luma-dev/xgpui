#include <metal_stdlib>
using namespace metal;
struct V { float4 position [[position]]; };
vertex V vs(uint i [[vertex_id]]) {
    const float2 p[3] = {float2(-1,-1),float2(3,-1),float2(-1,3)};
    return {float4(p[i],0,1)};
}
bool inside(float2 p, uint index, constant float4 *records) {
    float4 b=records[index*4]; float2 q=p-b.xy;
    if (any(q<0) || any(q>b.zw)) return false;
    if (records[index*4+3].x==1) { float2 d=(q-b.zw*.5)/(b.zw*.5); return dot(d,d)<=1; }
    float4 rx=records[index*4+1],ry=records[index*4+2];
    float2 centers[4]={float2(rx.x,ry.x),float2(b.z-rx.y,ry.y),float2(b.z-rx.z,b.w-ry.z),float2(rx.w,b.w-ry.w)};
    for (uint c=0;c<4;c++) {
        if (rx[c]==0 || ry[c]==0) continue;
        float2 d=q-centers[c]; float sx=(c==1 || c==2)?1:-1,sy=c>=2?1:-1;
        if (d.x*sx>0 && d.y*sy>0) { float2 e=d/float2(rx[c],ry[c]); if(dot(e,e)>1) return false; }
    }
    return true;
}
fragment float fs(V v [[stage_in]], constant float4 *records [[buffer(0)]], constant uint &count [[buffer(1)]]) {
    float coverage=0;
    for(uint y=0;y<4;y++) for(uint x=0;x<4;x++) {
        float2 s=floor(v.position.xy)+(float2(x,y)+.5)/4;
        bool visible=true;
        for(uint c=0;c<count;c++) visible=visible && inside(s,c,records);
        coverage+=visible?.0625:0;
    }
    return coverage;
}
