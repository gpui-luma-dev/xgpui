// xgpui-curve spine helpers. A spine is the quadratic Bézier p0 → c → p1.
// Every degenerate case (straight spine, coincident points, p ≈ 0 in the
// depressed cubic, acos/sqrt at the edge of their domains) stays finite.

fn curve_spine_eval(p0: vec2<f32>, c: vec2<f32>, p1: vec2<f32>, t: f32) -> vec2<f32> {
    let u = 1.0 - t;
    return p0 * (u * u) + c * (2.0 * u * t) + p1 * (t * t);
}

fn curve_spine_deriv(p0: vec2<f32>, c: vec2<f32>, p1: vec2<f32>, t: f32) -> vec2<f32> {
    return 2.0 * ((c - p0) * (1.0 - t) + (p1 - c) * t);
}

fn curve_spine_cbrt(x: f32) -> f32 {
    return sign(x) * pow(abs(x), 1.0 / 3.0);
}

// Arc length between t0 and t1 (absolute), closed form.
fn curve_spine_arclen(p0: vec2<f32>, c: vec2<f32>, p1: vec2<f32>, t0: f32, t1: f32) -> f32 {
    let lo = min(t0, t1);
    let hi = max(t0, t1);
    let av = c - p0;
    let bv = p0 - 2.0 * c + p1;
    let a = dot(bv, bv);
    let b = dot(av, bv);
    let cc = dot(av, av);
    if (a <= 1e-10 * max(cc, 1e-30)) {
        return 2.0 * sqrt(cc) * (hi - lo);
    }
    let disc = a * cc - b * b;
    if (disc <= 1e-6 * a * cc) {
        let r = -b / a;
        let g_hi = 0.5 * (hi - r) * abs(hi - r);
        let g_lo = 0.5 * (lo - r) * abs(lo - r);
        return 2.0 * sqrt(a) * (g_hi - g_lo);
    }
    let root = sqrt(disc);
    let k = disc / (a * sqrt(a));
    let q_hi = sqrt(max(a * hi * hi + 2.0 * b * hi + cc, 0.0));
    let q_lo = sqrt(max(a * lo * lo + 2.0 * b * lo + cc, 0.0));
    let f_hi = (hi + b / a) * q_hi + k * asinh((a * hi + b) / root);
    let f_lo = (lo + b / a) * q_lo + k * asinh((a * lo + b) / root);
    return f_hi - f_lo;
}

fn curve_spine_polish(k: vec4<f32>, t: f32) -> f32 {
    var x = t;
    for (var i = 0; i < 2; i++) {
        let f = ((k.x * x + k.y) * x + k.z) * x + k.w;
        let df = (3.0 * k.x * x + 2.0 * k.y) * x + k.z;
        if (abs(df) > 1e-20) {
            let next = x - f / df;
            let f_next = ((k.x * next + k.y) * next + k.z) * next + k.w;
            if (abs(f_next) <= abs(f)) {
                x = next;
            }
        }
    }
    return x;
}

// Project p onto the spine restricted to [t_min, t_max]. Returns
// (signed distance, t, normalized arc position s, total length).
fn curve_spine_project(p: vec2<f32>, p0: vec2<f32>, c: vec2<f32>, p1: vec2<f32>, t_min: f32, t_max: f32) -> vec4<f32> {
    let lo = clamp(min(t_min, t_max), 0.0, 1.0);
    let hi = clamp(max(t_min, t_max), 0.0, 1.0);
    let av = c - p0;
    let bv = p0 - 2.0 * c + p1;
    let dv = p0 - p;
    let k = vec4<f32>(dot(bv, bv), 3.0 * dot(av, bv), 2.0 * dot(av, av) + dot(dv, bv), dot(dv, av));

    var roots = vec3<f32>(lo, lo, lo);
    let scale = max(max(abs(k.y), abs(k.z)), abs(k.w));
    if (abs(k.x) <= 1e-6 * scale || k.x == 0.0) {
        // Quadratic or linear (a straight, uniform spine).
        if (abs(k.y) <= 1e-6 * max(abs(k.z), abs(k.w)) || k.y == 0.0) {
            if (k.z != 0.0) { roots.x = -k.w / k.z; }
        } else {
            let disc = k.z * k.z - 4.0 * k.y * k.w;
            if (disc < 0.0) {
                roots.x = -k.z / (2.0 * k.y);
            } else {
                let s = select(-1.0, 1.0, k.z >= 0.0);
                let q = -0.5 * (k.z + s * sqrt(disc));
                roots.x = q / k.y;
                if (q != 0.0) { roots.y = k.w / q; }
            }
        }
    } else {
        let b = k.y / k.x;
        let cc = k.z / k.x;
        let d = k.w / k.x;
        let pp = cc - b * b / 3.0;
        let q = 2.0 * b * b * b / 27.0 - b * cc / 3.0 + d;
        let shift = -b / 3.0;
        let disc = 0.25 * q * q + pp * pp * pp / 27.0;
        if (disc > 0.0) {
            let s = select(-1.0, 1.0, q >= 0.0);
            let a = -s * curve_spine_cbrt(0.5 * abs(q) + sqrt(disc));
            var r = 0.0;
            if (a != 0.0) { r = a - pp / (3.0 * a); }
            roots = vec3<f32>(r + shift);
        } else if (pp >= 0.0) {
            roots = vec3<f32>(shift);
        } else {
            let r = sqrt(-pp / 3.0);
            let cosv = clamp(1.5 * q / pp * sqrt(-3.0 / pp), -1.0, 1.0);
            let phi = acos(cosv) / 3.0;
            roots = vec3<f32>(
                2.0 * r * cos(phi) + shift,
                2.0 * r * cos(phi - 2.0943951) + shift,
                2.0 * r * cos(phi - 4.1887902) + shift,
            );
        }
        roots = vec3<f32>(curve_spine_polish(k, roots.x), curve_spine_polish(k, roots.y), curve_spine_polish(k, roots.z));
    }

    var best_t = lo;
    var best_d = dot(curve_spine_eval(p0, c, p1, lo) - p, curve_spine_eval(p0, c, p1, lo) - p);
    let candidates = array<f32, 4>(hi, roots.x, roots.y, roots.z);
    for (var i = 0; i < 4; i++) {
        let t = clamp(candidates[i], lo, hi);
        let e = curve_spine_eval(p0, c, p1, t) - p;
        let d2 = dot(e, e);
        if (d2 < best_d) {
            best_d = d2;
            best_t = t;
        }
    }

    let point = curve_spine_eval(p0, c, p1, best_t);
    var tangent = curve_spine_deriv(p0, c, p1, best_t);
    if (dot(tangent, tangent) <= 1e-20) {
        tangent = select(bv, -bv, best_t >= 1.0);
        if (dot(tangent, tangent) <= 1e-20) { tangent = p1 - p0; }
    }
    let normal = vec2<f32>(-tangent.y, tangent.x);
    let distance = sqrt(best_d);
    let sd = select(distance, -distance, dot(normal, p - point) < 0.0);
    let total = curve_spine_arclen(p0, c, p1, 0.0, 1.0);
    var s = 0.0;
    if (total > 0.0) { s = curve_spine_arclen(p0, c, p1, 0.0, best_t) / total; }
    return vec4<f32>(sd, best_t, s, total);
}
