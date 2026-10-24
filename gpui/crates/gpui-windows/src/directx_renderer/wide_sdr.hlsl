// Private headless compositor. Inputs and output are associated alpha.
cbuffer Params : register(b0) {
    float4 bounds;   // destination x, y, width, height
    float4 viewport; // target width, height, opacity, source kind (0 UI, 1 P3)
};
Texture2D<float4> source : register(t0);
struct Vertex { float4 position : SV_Position; float2 uv : TEXCOORD0; };
Vertex vertex_main(uint id : SV_VertexID) {
    const float2 corners[6] = {
        float2(0,0), float2(1,0), float2(0,1),
        float2(0,1), float2(1,0), float2(1,1)
    };
    Vertex v;
    v.uv = corners[id];
    float2 p = bounds.xy + v.uv * bounds.zw;
    v.position = float4(p.x / viewport.x * 2 - 1, 1 - p.y / viewport.y * 2, 0, 1);
    return v;
}
float decode_srgb(float c) {
    return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}
float4 fragment_main(Vertex input) : SV_Target {
    uint w, h;
    source.GetDimensions(w, h);
    int2 texel = clamp(int2(input.uv * float2(w,h)), int2(0,0), int2(w-1,h-1));
    float4 c = source.Load(int3(texel,0));
    if (viewport.w < 0.5) {
        // Scratch contains associated encoded sRGB. Decode straight RGB first.
        float3 straight = c.a > 0 ? c.rgb / c.a : float3(0,0,0);
        c.rgb = float3(decode_srgb(straight.r), decode_srgb(straight.g), decode_srgb(straight.b)) * c.a;
    } else {
        // Linear P3/D65 -> linear sRGB/scRGB. Preserve negative / >1 channels.
        c.rgb = float3(
            dot(c.rgb, float3(1.22494018, -0.22494018, 0)),
            dot(c.rgb, float3(-0.04205695, 1.04205695, 0)),
            dot(c.rgb, float3(-0.01963755, -0.07863605, 1.09827360)));
    }
    return c * viewport.z; // opacity multiplies RGB and alpha exactly once
}

float4 present_main(Vertex input) : SV_Target {
    float4 c = source.Load(int3(int2(input.position.xy),0));
    return float4(c.rgb * viewport.z, c.a);
}
float encode_srgb(float c) {
    c = saturate(c);
    return c <= 0.0031308 ? c*12.92 : 1.055*pow(c,1.0/2.4)-0.055;
}
float4 preview_main(Vertex input) : SV_Target {
    float4 c = source.Load(int3(int2(input.position.xy),0));
    float3 v = c.a > 0 ? c.rgb/c.a : float3(0,0,0);
    return float4(float3(encode_srgb(v.r),encode_srgb(v.g),encode_srgb(v.b))*c.a,c.a);
}
