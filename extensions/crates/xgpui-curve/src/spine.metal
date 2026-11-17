// xgpui-curve spine helpers (Metal Shading Language), mirroring spine.wgsl.
// A spine is the quadratic Bézier p0 → c → p1.

static inline float2 curve_spine_eval(float2 p0, float2 c, float2 p1, float t) {
    float u = 1.0f - t;
    return p0 * (u * u) + c * (2.0f * u * t) + p1 * (t * t);
}

static inline float2 curve_spine_deriv(float2 p0, float2 c, float2 p1, float t) {
    return 2.0f * ((c - p0) * (1.0f - t) + (p1 - c) * t);
}

static inline float curve_spine_cbrt(float x) {
    return metal::sign(x) * metal::pow(metal::abs(x), 1.0f / 3.0f);
}

// Arc length between t0 and t1 (absolute), closed form.
static inline float curve_spine_arclen(float2 p0, float2 c, float2 p1, float t0, float t1) {
    float lo = metal::min(t0, t1);
    float hi = metal::max(t0, t1);
    float2 av = c - p0;
    float2 bv = p0 - 2.0f * c + p1;
    float a = metal::dot(bv, bv);
    float b = metal::dot(av, bv);
    float cc = metal::dot(av, av);
    if (a <= 1e-10f * metal::max(cc, 1e-30f)) {
        return 2.0f * metal::sqrt(cc) * (hi - lo);
    }
    float disc = a * cc - b * b;
    if (disc <= 1e-6f * a * cc) {
        float r = -b / a;
        float g_hi = 0.5f * (hi - r) * metal::abs(hi - r);
        float g_lo = 0.5f * (lo - r) * metal::abs(lo - r);
        return 2.0f * metal::sqrt(a) * (g_hi - g_lo);
    }
    float root = metal::sqrt(disc);
    float k = disc / (a * metal::sqrt(a));
    float q_hi = metal::sqrt(metal::max(a * hi * hi + 2.0f * b * hi + cc, 0.0f));
    float q_lo = metal::sqrt(metal::max(a * lo * lo + 2.0f * b * lo + cc, 0.0f));
    float f_hi = (hi + b / a) * q_hi + k * metal::asinh((a * hi + b) / root);
    float f_lo = (lo + b / a) * q_lo + k * metal::asinh((a * lo + b) / root);
    return f_hi - f_lo;
}

static inline float curve_spine_polish(float4 k, float t) {
    float x = t;
    for (int i = 0; i < 2; i++) {
        float f = ((k.x * x + k.y) * x + k.z) * x + k.w;
        float df = (3.0f * k.x * x + 2.0f * k.y) * x + k.z;
        if (df != 0.0f) {
            float next = x - f / df;
            float f_next = ((k.x * next + k.y) * next + k.z) * next + k.w;
            if (metal::abs(f_next) <= metal::abs(f)) {
                x = next;
            }
        }
    }
    return x;
}

// Project p onto the spine restricted to [t_min, t_max]. Returns
// (signed distance, t, normalized arc position s, total length).
static inline float4 curve_spine_project(float2 p, float2 p0, float2 c, float2 p1, float t_min, float t_max) {
    float lo = metal::clamp(metal::min(t_min, t_max), 0.0f, 1.0f);
    float hi = metal::clamp(metal::max(t_min, t_max), 0.0f, 1.0f);
    float2 av = c - p0;
    float2 bv = p0 - 2.0f * c + p1;
    float2 dv = p0 - p;
    float4 k = float4(metal::dot(bv, bv), 3.0f * metal::dot(av, bv), 2.0f * metal::dot(av, av) + metal::dot(dv, bv), metal::dot(dv, av));

    float3 roots = float3(lo, lo, lo);
    float scale = metal::max(metal::max(metal::abs(k.y), metal::abs(k.z)), metal::abs(k.w));
    if (metal::abs(k.x) <= 1e-6f * scale || k.x == 0.0f) {
        if (metal::abs(k.y) <= 1e-6f * metal::max(metal::abs(k.z), metal::abs(k.w)) || k.y == 0.0f) {
            if (k.z != 0.0f) { roots.x = -k.w / k.z; }
        } else {
            float disc = k.z * k.z - 4.0f * k.y * k.w;
            if (disc < 0.0f) {
                roots.x = -k.z / (2.0f * k.y);
            } else {
                float s = k.z >= 0.0f ? 1.0f : -1.0f;
                float q = -0.5f * (k.z + s * metal::sqrt(disc));
                roots.x = q / k.y;
                if (q != 0.0f) { roots.y = k.w / q; }
            }
        }
    } else {
        float b = k.y / k.x;
        float cc = k.z / k.x;
        float d = k.w / k.x;
        float pp = cc - b * b / 3.0f;
        float q = 2.0f * b * b * b / 27.0f - b * cc / 3.0f + d;
        float shift = -b / 3.0f;
        float disc = 0.25f * q * q + pp * pp * pp / 27.0f;
        if (disc > 0.0f) {
            float s = q >= 0.0f ? 1.0f : -1.0f;
            float a = -s * curve_spine_cbrt(0.5f * metal::abs(q) + metal::sqrt(disc));
            float r = 0.0f;
            if (a != 0.0f) { r = a - pp / (3.0f * a); }
            roots = float3(r + shift);
        } else if (pp >= 0.0f) {
            roots = float3(shift);
        } else {
            float r = metal::sqrt(-pp / 3.0f);
            float cosv = metal::clamp(1.5f * q / pp * metal::sqrt(-3.0f / pp), -1.0f, 1.0f);
            float phi = metal::acos(cosv) / 3.0f;
            roots = float3(
                2.0f * r * metal::cos(phi) + shift,
                2.0f * r * metal::cos(phi - 2.0943951f) + shift,
                2.0f * r * metal::cos(phi - 4.1887902f) + shift);
        }
        roots = float3(curve_spine_polish(k, roots.x), curve_spine_polish(k, roots.y), curve_spine_polish(k, roots.z));
    }

    float best_t = lo;
    float2 e0 = curve_spine_eval(p0, c, p1, lo) - p;
    float best_d = metal::dot(e0, e0);
    float candidates[4] = { hi, roots.x, roots.y, roots.z };
    for (int i = 0; i < 4; i++) {
        float t = metal::clamp(candidates[i], lo, hi);
        float2 e = curve_spine_eval(p0, c, p1, t) - p;
        float d2 = metal::dot(e, e);
        if (d2 < best_d) {
            best_d = d2;
            best_t = t;
        }
    }

    float2 point = curve_spine_eval(p0, c, p1, best_t);
    float2 tangent = curve_spine_deriv(p0, c, p1, best_t);
    if (metal::dot(tangent, tangent) <= 1e-20f) {
        tangent = best_t >= 1.0f ? -bv : bv;
        if (metal::dot(tangent, tangent) <= 1e-20f) { tangent = p1 - p0; }
    }
    float2 normal = float2(-tangent.y, tangent.x);
    float distance = metal::sqrt(best_d);
    float sd = metal::dot(normal, p - point) < 0.0f ? -distance : distance;
    float total = curve_spine_arclen(p0, c, p1, 0.0f, 1.0f);
    float s = 0.0f;
    if (total > 0.0f) { s = curve_spine_arclen(p0, c, p1, 0.0f, best_t) / total; }
    return float4(sd, best_t, s, total);
}
