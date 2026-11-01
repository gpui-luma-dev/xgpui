#include <metal_stdlib>
using namespace metal;
struct Parameters { float4 coordinates; uint limit; uint padding[3]; };
float3 decode_p3(float3 v) {
    return select(pow((v + 0.055f) / 1.055f, float3(2.4f)), v / 12.92f, v <= 0.04045f);
}
kernel void mandelbrot_viewport(texture2d<float, access::write> image [[texture(0)]], constant Parameters &p [[buffer(0)]], uint2 pixel [[thread_position_in_grid]]) {
    if (pixel.x >= image.get_width() || pixel.y >= image.get_height()) return;
    float2 uv = (float2(pixel) + 0.5f) / float2(image.get_width(), image.get_height());
    float2 c = p.coordinates.xy + (uv - 0.5f) * p.coordinates.zw;
    float q = (c.x - 0.25f) * (c.x - 0.25f) + c.y * c.y;
    uint n = p.limit;
    if (!(q * (q + c.x - 0.25f) <= 0.25f * c.y * c.y || (c.x + 1.f) * (c.x + 1.f) + c.y * c.y <= 0.0625f)) {
        float2 z = 0.f;
        for (uint i = 0; i < p.limit; ++i) {
            if (dot(z, z) > 4.f) { n = i; break; }
            z = float2(z.x * z.x - z.y * z.y, 2.f * z.x * z.y) + c;
        }
    }
    float3 encoded = n == p.limit ? float3(5.f, 8.f, 16.f) / 255.f : sin(float(n) * 0.11f + float3(0.f, 2.f, 4.f)) * 0.5f + 0.5f;
    image.write(float4(decode_p3(encoded), 1.f), pixel);
}
