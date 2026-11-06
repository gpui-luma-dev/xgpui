// Native Metal pastel waves. Constants (RIB_EDGES, RIB_STYLE, FOLD_CRESTS,
// FIELD_GEOM, FIELD_TINT, BG_POS, BG_COLOR) are generated from
// pastel_waves.rs and prepended to this source.
//
// pastel_columns evaluates every ribbon curve once per pixel column (the
// curves depend only on x). pastel_fragment then shades each pixel from those
// columns: no spline, trig or fold-strength math per pixel.

struct Uniforms {
    float4 frame;     // x: eased phase, y: grain (0/1), z: scale factor
    float4 extent;    // x: width, y: height in device pixels
    float4 bounds[5]; // per-ribbon fill y bounds (min, max), normalized
};

constant float THREADS[5] = {0.15f, 0.32f, 0.52f, 0.73f, 0.88f};

// Catmull-Rom edge (x: value, y: d/du), matching edge_y in pastel_waves.rs.
static float2 ribbon_edge(uint r, uint side, float u) {
    float x = clamp(u, 0.0f, 1.0f) * 6.0f;
    int i = min(int(floor(x)), 5);
    float t = x - float(i);
    uint base = r * 14u + side * 7u;
    float a = RIB_EDGES[base + uint(max(i - 1, 0))];
    float b = RIB_EDGES[base + uint(i)];
    float c = RIB_EDGES[base + uint(i + 1)];
    float d = RIB_EDGES[base + uint(min(i + 2, 6))];
    float c1 = -a + c;
    float c2 = 2.0f * a - 5.0f * b + 4.0f * c - d;
    float c3 = -a + 3.0f * b - 3.0f * c + d;
    float value = 0.5f * (2.0f * b + c1 * t + c2 * t * t + c3 * t * t * t);
    float slope = 3.0f * (c1 + 2.0f * c2 * t + 3.0f * c3 * t * t);
    return float2(value, (u >= 0.0f && u <= 1.0f) ? slope : 0.0f);
}

static float2 ribbon_motion(uint r, float u, float phase) {
    float offset = u * 5.0f + float(r) * 0.7f;
    return float2(0.012f * (sin(phase + offset) - sin(offset)),
                  0.06f * (cos(phase + offset) - cos(offset)));
}

static float fold_strength(uint r, float u) {
    float crest = FOLD_CRESTS[r - 5u];
    float e = 0.008f;
    float bend = abs(ribbon_edge(r, 0u, u + e).x - 2.0f * ribbon_edge(r, 0u, u).x
                     + ribbon_edge(r, 0u, u - e).x) / (e * e);
    float curvature = bend / (bend + 22.0f);
    float z = (u - crest) / 0.105f;
    float fold = exp(-0.5f * z * z);
    return clamp(0.68f * fold + 0.32f * curvature, 0.0f, 1.0f);
}

// Per column x and ribbon r, two float4s at (x * 9 + r) * 2:
//   [0] top value, top d/du, bottom value, bottom d/du (drift included)
//   [1] fold strength (foreground ribbons), then the device-pixel y range
//       outside which every layer of this ribbon has exactly zero coverage
kernel void pastel_columns(device float4 *columns [[buffer(0)]],
                           constant Uniforms &uniforms [[buffer(1)]],
                           uint x [[thread_position_in_grid]]) {
    uint width = uint(uniforms.extent.x);
    if (x >= width) {
        return;
    }
    float2 size = max(uniforms.extent.xy, float2(1.0f));
    float u = (float(x) + 0.5f) / float(width);
    for (uint r = 0u; r < 9u; r++) {
        float2 drift = ribbon_motion(r, u, uniforms.frame.x);
        float2 top = ribbon_edge(r, 0u, u) + drift;
        float2 bottom = ribbon_edge(r, 1u, u) + drift;
        float strength = r >= 5u ? fold_strength(r, u) : 0.0f;

        // Coverage uses perpendicular distance, so a half-pixel edge spans
        // 0.5 * sqrt(1 + slope^2) vertically. Bands between top and bottom
        // interpolate value and slope, so the steeper edge bounds them all.
        float top_slope = top.y * size.y / size.x;
        float bottom_slope = bottom.y * size.y / size.x;
        float top_stretch = sqrt(1.0f + top_slope * top_slope);
        float max_stretch = sqrt(1.0f + max(top_slope * top_slope, bottom_slope * bottom_slope));
        float top_y = top.x * size.y;
        float bottom_y = bottom.x * size.y;
        float lo = min(top_y, bottom_y) - 0.5f * max_stretch;
        float hi = max(top_y, bottom_y) + 0.5f * max_stretch;
        if (r >= 5u) {
            // Rim spans -0.2..0.8 of its width around the top edge; the veil
            // reaches `reach` of the thickness below it.
            float rim = (0.35f + 2.25f * strength) * uniforms.frame.z;
            float reach = 0.12f + 0.43f * strength;
            lo = min(lo, top_y - (0.2f * rim + 0.5f) * top_stretch);
            hi = max(hi, top_y + (0.8f * rim + 0.5f) * top_stretch);
            hi = max(hi, (top.x + reach * max(bottom.x - top.x, 1e-5f)) * size.y);
        }
        columns[(x * 9u + r) * 2u] = float4(top, bottom);
        // One extra pixel absorbs rounding differences with the fragment pass.
        columns[(x * 9u + r) * 2u + 1u] = float4(strength, lo - 1.0f, hi + 1.0f, 0.0f);
    }
}

struct VertexOut {
    float4 position [[position]];
};

// One oversized triangle covering the target.
vertex VertexOut pastel_vertex(uint id [[vertex_id]]) {
    float2 uv = float2(float((id << 1u) & 2u), float(id & 2u));
    VertexOut out;
    out.position = float4(uv * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);
    return out;
}

// Coverage below a curve (value, d/du) from perpendicular distance in device px.
static float below(float2 curve, float y, float2 size) {
    float slope = curve.y * size.y / size.x;
    float distance = (y - curve.x * size.y) * rsqrt(1.0f + slope * slope);
    return clamp(0.5f + distance, 0.0f, 1.0f);
}

static float band(float2 top, float2 bottom, float fa, float fb, float y, float2 size) {
    return abs(below(top + (bottom - top) * fa, y, size) - below(top + (bottom - top) * fb, y, size));
}

static float veil_profile(float v) {
    const float floor_value = exp(-3.5f);
    return (exp(-3.5f * v) - floor_value) / (1.0f - floor_value);
}

static float4 assoc(float4 c) {
    return float4(c.rgb * c.a, c.a);
}

static float3 over(float3 dst, float3 color, float alpha) {
    float a = clamp(alpha, 0.0f, 1.0f);
    return saturate(color) * a + dst * (1.0f - a);
}

static float3 soft_light(float3 base, float s) {
    float3 d = select(sqrt(base), ((16.0f * base - 12.0f) * base + 4.0f) * base, base <= 0.25f);
    return s <= 0.5f ? base - (1.0f - 2.0f * s) * base * (1.0f - base)
                     : base + (2.0f * s - 1.0f) * (d - base);
}

static float grain_signal(float2 pixel, uint r) {
    uint2 cell = as_type<uint2>(int2(floor(pixel)));
    uint h = ((cell.x + r * 7919u) * 0x9e3779b9u) ^ ((cell.y + r * 104729u) * 0x85ebca6bu);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return float(h & 0x00ffffffu) / 16777216.0f;
}

fragment float4 pastel_fragment(VertexOut in [[stage_in]],
                                constant Uniforms &uniforms [[buffer(0)]],
                                device const float4 *columns [[buffer(1)]]) {
    float2 size = max(uniforms.extent.xy, float2(1.0f));
    float2 pixel = in.position.xy;
    float2 uv = pixel / size;
    float u = clamp(uv.x, 0.0f, 1.0f);
    float y = pixel.y;
    float scale = uniforms.frame.z;
    bool grain = uniforms.frame.y > 0.5f;
    uint column = min(uint(pixel.x), uint(size.x) - 1u);

    // Background: inverse-square freeform field of five opaque colors.
    float distances[5];
    float nearest = 1e30f;
    for (uint i = 0u; i < 5u; i++) {
        float2 delta = uv - BG_POS[i];
        float dd = max(dot(delta, delta), 1e-12f);
        distances[i] = dd;
        nearest = min(nearest, dd);
    }
    float3 acc = float3(0.0f);
    float total = 0.0f;
    for (uint i = 0u; i < 5u; i++) {
        float w = nearest / distances[i];
        acc += BG_COLOR[i] * w;
        total += w;
    }
    acc /= total;

    // Fixed radial color fields: stops 0, 0.48, 1 at alpha 1, 0.45, 0.
    for (uint i = 0u; i < 9u; i++) {
        float4 geom = FIELD_GEOM[i];
        float4 tint = FIELD_TINT[i];
        float t = length((uv - geom.xy) / geom.zw);
        float fade = t < 0.48f ? mix(1.0f, 0.45f, t / 0.48f)
                               : mix(0.45f, 0.0f, clamp((t - 0.48f) / 0.52f, 0.0f, 1.0f));
        acc = over(acc, tint.rgb, fade * tint.a);
    }

    for (uint r = 0u; r < 9u; r++) {
        // Skip ribbons that cannot touch this row: every layer below would
        // composite with zero alpha.
        float4 extra = columns[(column * 9u + r) * 2u + 1u];
        if (y < extra.y || y > extra.z) {
            continue;
        }
        float4 edges = columns[(column * 9u + r) * 2u];
        float2 top = edges.xy;
        float2 bottom = edges.zw;
        float4 style = RIB_STYLE[r];
        float3 col = style.rgb;
        float alpha = style.a;

        // Fill: Brush::mesh over the fill path's bounds.
        float4 fill_bounds = uniforms.bounds[r / 2u];
        float lo = (r & 1u) == 0u ? fill_bounds.x : fill_bounds.z;
        float hi = (r & 1u) == 0u ? fill_bounds.y : fill_bounds.w;
        float2 mesh_uv = saturate(float2(u, (uv.y - lo) / max(hi - lo, 1e-6f)));
        float2 h = mesh_uv * mesh_uv * (3.0f - 2.0f * mesh_uv);
        float4 c0 = float4(min(col.r, 0.75f), min(col.g + 0.14f, 0.90f), min(col.b + 0.10f, 1.0f), alpha * 0.40f);
        float4 c1 = float4(min(col.r + 0.25f, 1.0f), col.g * 0.82f, min(col.b + 0.03f, 1.0f), alpha * 0.65f);
        float4 c2 = float4(col.r * 0.65f, col.g * 0.93f, col.b, alpha);
        float4 c3 = float4(min(col.r + 0.13f, 1.0f), col.g * 0.67f, col.b * 0.94f, alpha * 0.90f);
        float4 pm = mix(mix(assoc(c0), assoc(c1), h.x), mix(assoc(c2), assoc(c3), h.x), h.y);
        float3 fill = pm.rgb / max(pm.a, 1e-6f);
        if (grain) {
            fill = mix(fill, soft_light(fill, grain_signal(pixel, r)), 0.035f);
        }
        acc = over(acc, fill, pm.a * band(top, bottom, 0.0f, 1.0f, y, size));

        if (r >= 5u) {
            float s = extra.x;
            // Inward bleed from the top edge with an exponential depth profile.
            float thickness = max(bottom.x - top.x, 1e-5f);
            float depth = (uv.y - top.x) / thickness;
            float reach = 0.12f + 0.43f * s;
            float density = 0.32f + 0.95f * s;
            float veil = below(top, y, size)
                * (1.0f - exp(-density * veil_profile(clamp(depth / reach, 0.0f, 1.0f))));
            acc = over(acc, mix(float3(0.20f, 0.43f, 0.70f), float3(0.43f, 0.19f, 0.60f), u), veil);

            // Variable-width rim along the top edge's normal, -0.2..0.8 of width.
            float slope = top.y * size.y / size.x;
            float normal = (y - top.x * size.y) * rsqrt(1.0f + slope * slope);
            float width = (0.35f + 2.25f * s) * scale;
            float rim = clamp(min(0.8f * width, normal + 0.5f) - max(-0.2f * width, normal - 0.5f), 0.0f, 1.0f);
            float3 rim_color = mix(float3(0.18f, 0.46f, 0.66f), float3(0.63f, 0.20f, 0.53f), u) * (1.0f - 0.28f * s);
            acc = over(acc, rim_color, rim * (0.12f + 0.65f * s));
        }

        float lip_alpha = r >= 5u ? 0.10f : 0.20f;
        acc = over(acc, float3(0.98f, 0.98f, 1.0f), lip_alpha * band(top, bottom, 0.006f, 0.016f, y, size));
        if (grain) {
            float threads = 0.0f;
            for (uint k = 0u; k < 5u; k++) {
                threads += band(top, bottom, THREADS[k], THREADS[k] + 0.004f, y, size);
            }
            acc = over(acc, float3(1.0f, 0.96f, 1.0f), 0.045f * min(threads, 1.0f));
        }
    }
    // Opaque, so straight and associated alpha agree.
    return float4(saturate(acc), 1.0f);
}
