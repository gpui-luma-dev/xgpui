// Single-pass pastel waves. Evaluates the whole composition per pixel in one
// draw: no paths, coverage passes or per-layer programs. Data constants
// (RIB_EDGES, RIB_STYLE, FOLD_CRESTS, FIELD_GEOM, FIELD_TINT, BG_POS, BG_COLOR)
// are generated from pastel_waves.rs and prepended to this source.
//
// data0.x: eased phase, data0.y: grain (0/1)
// data1..data5: per-ribbon normalized y bounds (min, max) of the fill path,
//               which the Brush::mesh fill is evaluated against.
struct Parameters {
    data0: vec4<f32>, data1: vec4<f32>, data2: vec4<f32>, data3: vec4<f32>,
    data4: vec4<f32>, data5: vec4<f32>, data6: vec4<f32>, data7: vec4<f32>,
}

const THREADS: array<f32, 5> = array<f32, 5>(0.15, 0.32, 0.52, 0.73, 0.88);

// Catmull-Rom edge (x: value, y: d/du), matching edge_y in pastel_waves.rs.
// side 0 is the top edge, 1 the bottom edge.
fn ribbon_edge(r: u32, side: u32, u: f32) -> vec2<f32> {
    let x = clamp(u, 0., 1.) * 6.;
    let i = min(i32(floor(x)), 5);
    let t = x - f32(i);
    let base = r * 14u + side * 7u;
    let a = RIB_EDGES[base + u32(max(i - 1, 0))];
    let b = RIB_EDGES[base + u32(i)];
    let c = RIB_EDGES[base + u32(i + 1)];
    let d = RIB_EDGES[base + u32(min(i + 2, 6))];
    let c1 = -a + c;
    let c2 = 2. * a - 5. * b + 4. * c - d;
    let c3 = -a + 3. * b - 3. * c + d;
    let value = 0.5 * (2. * b + c1 * t + c2 * t * t + c3 * t * t * t);
    // d/du = 6 * 0.5 * d/dt; the clamp flattens the curve outside [0, 1].
    let slope = 3. * (c1 + 2. * c2 * t + 3. * c3 * t * t);
    return vec2(value, select(0., slope, u >= 0. && u <= 1.));
}

// Vertical drift (x: value, y: d/du), matching `position` in pastel_waves.rs.
fn ribbon_motion(r: u32, u: f32, phase: f32) -> vec2<f32> {
    let offset = u * 5. + f32(r) * 0.7;
    return vec2(
        0.012 * (sin(phase + offset) - sin(offset)),
        0.06 * (cos(phase + offset) - cos(offset)),
    );
}

// Curve at `fraction` of the thickness between top and bottom, plus drift.
fn boundary(top: vec2<f32>, bottom: vec2<f32>, drift: vec2<f32>, fraction: f32) -> vec2<f32> {
    return top + (bottom - top) * fraction + drift;
}

// Coverage below a curve from its perpendicular distance in logical pixels;
// `aa` is one device pixel.
fn below(curve: vec2<f32>, y: f32, size: vec2<f32>, aa: f32) -> f32 {
    let slope = curve.y * size.y / size.x;
    let distance = (y - curve.x * size.y) / sqrt(1. + slope * slope);
    return clamp(0.5 + distance / aa, 0., 1.);
}

// Coverage of the band between two thickness fractions. Absolute difference
// keeps inverted spans filled, like the nonzero path fill.
fn band(top: vec2<f32>, bottom: vec2<f32>, drift: vec2<f32>, fa: f32, fb: f32,
        y: f32, size: vec2<f32>, aa: f32) -> f32 {
    return abs(below(boundary(top, bottom, drift, fa), y, size, aa)
        - below(boundary(top, bottom, drift, fb), y, size, aa));
}

fn fold_strength(r: u32, u: f32) -> f32 {
    let crest = FOLD_CRESTS[r - 5u];
    let e = 0.008;
    let bend = abs(ribbon_edge(r, 0u, u + e).x - 2. * ribbon_edge(r, 0u, u).x
        + ribbon_edge(r, 0u, u - e).x) / (e * e);
    let curvature = bend / (bend + 22.);
    let z = (u - crest) / 0.105;
    let fold = exp(-0.5 * z * z);
    return clamp(0.68 * fold + 0.32 * curvature, 0., 1.);
}

// Accumulated weight of all veil layers at normalized depth v (1 at the rim,
// 0 at the deepest layer): the continuous limit of the 24 stacked veils.
fn veil_profile(v: f32) -> f32 {
    return (exp(-3.5 * v) - exp(-3.5)) / (1. - exp(-3.5));
}

fn assoc(c: vec4<f32>) -> vec4<f32> {
    return vec4(c.rgb * c.a, c.a);
}

fn over(dst: vec3<f32>, color: vec3<f32>, alpha: f32) -> vec3<f32> {
    let a = clamp(alpha, 0., 1.);
    return clamp(color, vec3(0.), vec3(1.)) * a + dst * (1. - a);
}

fn soft_light(base: vec3<f32>, s: f32) -> vec3<f32> {
    let d = select(sqrt(base), ((16. * base - 12.) * base + 4.) * base, base <= vec3(0.25));
    return select(base + (2. * s - 1.) * (d - base), base - (1. - 2. * s) * base * (1. - base),
        vec3(s) <= vec3(0.5));
}

// Per-device-pixel grain in [0, 1), decorrelated per ribbon.
fn grain_signal(pixel: vec2<f32>, r: u32) -> f32 {
    let cell = bitcast<vec2<u32>>(vec2<i32>(floor(pixel)));
    var h = ((cell.x + r * 7919u) * 0x9e3779b9u) ^ ((cell.y + r * 104729u) * 0x85ebca6bu);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h & 0x00ffffffu) / 16777216.;
}

fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let size = max(input.size, vec2(1.));
    let uv = input.uv;
    let u = clamp(uv.x, 0., 1.);
    let y = input.position.y;
    let aa = 1. / input.scale_factor;
    let phase = p.data0.x;
    let grain = p.data0.y > 0.5;
    var bounds = array<f32, 18>(
        p.data1.x, p.data1.y, p.data1.z, p.data1.w,
        p.data2.x, p.data2.y, p.data2.z, p.data2.w,
        p.data3.x, p.data3.y, p.data3.z, p.data3.w,
        p.data4.x, p.data4.y, p.data4.z, p.data4.w,
        p.data5.x, p.data5.y,
    );

    // Background: inverse-square freeform field of five opaque colors.
    var distances: array<f32, 5>;
    var nearest = 1e30;
    for (var i = 0u; i < 5u; i += 1u) {
        let delta = uv - BG_POS[i];
        let dd = max(dot(delta, delta), 1e-12);
        distances[i] = dd;
        nearest = min(nearest, dd);
    }
    var acc = vec3(0.);
    var total = 0.;
    for (var i = 0u; i < 5u; i += 1u) {
        let w = nearest / distances[i];
        acc += BG_COLOR[i] * w;
        total += w;
    }
    acc /= total;

    // Fixed radial color fields: stops 0, 0.48, 1 at alpha 1, 0.45, 0.
    for (var i = 0u; i < 9u; i += 1u) {
        let geom = FIELD_GEOM[i];
        let tint = FIELD_TINT[i];
        let t = length((uv - geom.xy) / geom.zw);
        let fade = select(mix(0.45, 0., clamp((t - 0.48) / 0.52, 0., 1.)),
            mix(1., 0.45, t / 0.48), t < 0.48);
        acc = over(acc, tint.rgb, fade * tint.a);
    }

    for (var r = 0u; r < 9u; r += 1u) {
        let style = RIB_STYLE[r];
        let col = style.rgb;
        let alpha = style.a;
        let top = ribbon_edge(r, 0u, u);
        let bottom = ribbon_edge(r, 1u, u);
        let drift = ribbon_motion(r, u, phase);

        // Skip ribbons that cannot touch this row: every layer below would
        // composite with zero alpha. Coverage uses perpendicular distance, so
        // a half-pixel edge spans 0.5 * sqrt(1 + slope^2) vertically; bands
        // interpolate value and slope, so the steeper edge bounds them all.
        // The rim (at most 2.6 wide, -0.2..0.8 around the top edge) and veil
        // (at most 0.55 of the thickness) use their largest fold strength.
        let edge_top = top + drift;
        let edge_bottom = bottom + drift;
        let top_slope = edge_top.y * size.y / size.x;
        let bottom_slope = edge_bottom.y * size.y / size.x;
        let top_stretch = sqrt(1. + top_slope * top_slope);
        let max_stretch = sqrt(1. + max(top_slope * top_slope, bottom_slope * bottom_slope));
        let top_y = edge_top.x * size.y;
        let bottom_y = edge_bottom.x * size.y;
        var row_lo = min(top_y, bottom_y) - 0.5 * aa * max_stretch;
        var row_hi = max(top_y, bottom_y) + 0.5 * aa * max_stretch;
        if (r >= 5u) {
            row_lo = min(row_lo, top_y - (0.52 + 0.5 * aa) * top_stretch);
            row_hi = max(row_hi, top_y + (2.08 + 0.5 * aa) * top_stretch);
            row_hi = max(row_hi, (edge_top.x + 0.55 * max(bottom.x - top.x, 1e-5)) * size.y);
        }
        // One extra device pixel absorbs rounding in the bounds themselves.
        if (y < row_lo - aa || y > row_hi + aa) {
            continue;
        }

        // Fill: Brush::mesh over the fill path's bounds.
        let lo = bounds[r * 2u];
        let hi = bounds[r * 2u + 1u];
        let mesh_uv = clamp(vec2(u, (y / size.y - lo) / max(hi - lo, 1e-6)), vec2(0.), vec2(1.));
        let h = mesh_uv * mesh_uv * (3. - 2. * mesh_uv);
        let c0 = vec4(min(col.r, 0.75), min(col.g + 0.14, 0.90), min(col.b + 0.10, 1.), alpha * 0.40);
        let c1 = vec4(min(col.r + 0.25, 1.), col.g * 0.82, min(col.b + 0.03, 1.), alpha * 0.65);
        let c2 = vec4(col.r * 0.65, col.g * 0.93, col.b, alpha);
        let c3 = vec4(min(col.r + 0.13, 1.), col.g * 0.67, col.b * 0.94, alpha * 0.90);
        let pm = mix(mix(assoc(c0), assoc(c1), h.x), mix(assoc(c2), assoc(c3), h.x), h.y);
        var fill = pm.rgb / max(pm.a, 1e-6);
        if (grain) {
            fill = mix(fill, soft_light(fill, grain_signal(input.position * input.scale_factor, r)), 0.035);
        }
        acc = over(acc, fill, pm.a * band(top, bottom, drift, 0., 1., y, size, aa));

        if (r >= 5u) {
            let s = fold_strength(r, u);
            // Inward bleed from the top edge with an exponential depth profile.
            let thickness = max(bottom.x - top.x, 1e-5);
            let depth = (y / size.y - (top.x + drift.x)) / thickness;
            let reach = 0.12 + 0.43 * s;
            let density = 0.32 + 0.95 * s;
            let veil = below(top + drift, y, size, aa)
                * (1. - exp(-density * veil_profile(clamp(depth / reach, 0., 1.))));
            acc = over(acc, mix(vec3(0.20, 0.43, 0.70), vec3(0.43, 0.19, 0.60), u), veil);

            // Variable-width rim along the top edge's screen-space normal,
            // spanning -0.2..0.8 of its width.
            let rim_curve = top + drift;
            let rim_slope = rim_curve.y * size.y / size.x;
            let normal = (y - rim_curve.x * size.y) / sqrt(1. + rim_slope * rim_slope);
            let width = 0.35 + 2.25 * s;
            let rim = clamp((min(0.8 * width, normal + 0.5 * aa)
                - max(-0.2 * width, normal - 0.5 * aa)) / aa, 0., 1.);
            let rim_color = mix(vec3(0.18, 0.46, 0.66), vec3(0.63, 0.20, 0.53), u) * (1. - 0.28 * s);
            acc = over(acc, rim_color, rim * (0.12 + 0.65 * s));
        }

        let lip_alpha = select(0.20, 0.10, r >= 5u);
        acc = over(acc, vec3(0.98, 0.98, 1.), lip_alpha * band(top, bottom, drift, 0.006, 0.016, y, size, aa));
        if (grain) {
            var threads = 0.;
            for (var k = 0u; k < 5u; k += 1u) {
                let f = THREADS[k];
                threads += band(top, bottom, drift, f, f + 0.004, y, size, aa);
            }
            acc = over(acc, vec3(1., 0.96, 1.), 0.045 * min(threads, 1.));
        }
    }
    return vec4(clamp(acc, vec3(0.), vec3(1.)), 1.);
}
