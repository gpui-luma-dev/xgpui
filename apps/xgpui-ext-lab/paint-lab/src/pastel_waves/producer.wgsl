// Native producer for pastel waves on wgpu (used as-is) and D3D11 (translated
// to HLSL shader model 5.0 with naga). The same two passes as the Metal
// producer in pastel_waves.metal; keep the two in step. Constants (RIB_EDGES,
// RIB_STYLE, FOLD_CRESTS, FIELD_GEOM, FIELD_TINT, BG_POS, BG_COLOR) are
// generated from pastel_waves.rs and prepended to this source.
//
// pastel_columns evaluates every ribbon curve once per pixel column (the
// curves depend only on x). pastel_fragment then shades each pixel from those
// columns: no spline, trig or fold-strength math per pixel.
//
// Bindings map to D3D11 registers by number: b0, u1 (compute), t2 (fragment).

struct Uniforms {
    frame: vec4<f32>,         // x: eased phase, y: grain (0/1), z: scale factor
    extent: vec4<f32>,        // x: width, y: height in device pixels
    bounds: array<vec4<f32>, 5>, // per-ribbon fill y bounds (min, max), normalized
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var<storage, read_write> columns_out: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> columns: array<vec4<f32>>;

const THREADS: array<f32, 5> = array<f32, 5>(0.15, 0.32, 0.52, 0.73, 0.88);

// Catmull-Rom edge (x: value, y: d/du), matching edge_y in pastel_waves.rs.
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
    let slope = 3. * (c1 + 2. * c2 * t + 3. * c3 * t * t);
    return vec2(value, select(0., slope, u >= 0. && u <= 1.));
}

fn ribbon_motion(r: u32, u: f32, phase: f32) -> vec2<f32> {
    let offset = u * 5. + f32(r) * 0.7;
    return vec2(0.012 * (sin(phase + offset) - sin(offset)),
                0.06 * (cos(phase + offset) - cos(offset)));
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

// Per column x and ribbon r, two vec4s at (x * 9 + r) * 2:
//   [0] top value, top d/du, bottom value, bottom d/du (drift included)
//   [1] fold strength (foreground ribbons), then the device-pixel y range
//       outside which every layer of this ribbon has exactly zero coverage
@compute @workgroup_size(64)
fn pastel_columns(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let width = u32(uniforms.extent.x);
    if (x >= width) {
        return;
    }
    let size = max(uniforms.extent.xy, vec2(1.));
    let u = (f32(x) + 0.5) / f32(width);
    for (var r = 0u; r < 9u; r += 1u) {
        let drift = ribbon_motion(r, u, uniforms.frame.x);
        let top = ribbon_edge(r, 0u, u) + drift;
        let bottom = ribbon_edge(r, 1u, u) + drift;
        var strength = 0.;
        if (r >= 5u) {
            strength = fold_strength(r, u);
        }

        // Coverage uses perpendicular distance, so a half-pixel edge spans
        // 0.5 * sqrt(1 + slope^2) vertically. Bands between top and bottom
        // interpolate value and slope, so the steeper edge bounds them all.
        let top_slope = top.y * size.y / size.x;
        let bottom_slope = bottom.y * size.y / size.x;
        let top_stretch = sqrt(1. + top_slope * top_slope);
        let max_stretch = sqrt(1. + max(top_slope * top_slope, bottom_slope * bottom_slope));
        let top_y = top.x * size.y;
        let bottom_y = bottom.x * size.y;
        var lo = min(top_y, bottom_y) - 0.5 * max_stretch;
        var hi = max(top_y, bottom_y) + 0.5 * max_stretch;
        if (r >= 5u) {
            // Rim spans -0.2..0.8 of its width around the top edge; the veil
            // reaches `reach` of the thickness below it.
            let rim = (0.35 + 2.25 * strength) * uniforms.frame.z;
            let reach = 0.12 + 0.43 * strength;
            lo = min(lo, top_y - (0.2 * rim + 0.5) * top_stretch);
            hi = max(hi, top_y + (0.8 * rim + 0.5) * top_stretch);
            hi = max(hi, (top.x + reach * max(bottom.x - top.x, 1e-5)) * size.y);
        }
        columns_out[(x * 9u + r) * 2u] = vec4(top, bottom);
        // One extra pixel absorbs rounding differences with the fragment pass.
        columns_out[(x * 9u + r) * 2u + 1u] = vec4(strength, lo - 1., hi + 1., 0.);
    }
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
}

// One oversized triangle covering the target.
@vertex
fn pastel_vertex(@builtin(vertex_index) id: u32) -> VertexOut {
    let uv = vec2(f32((id << 1u) & 2u), f32(id & 2u));
    var out: VertexOut;
    out.position = vec4(uv * vec2(2., -2.) + vec2(-1., 1.), 0., 1.);
    return out;
}

// Coverage below a curve (value, d/du) from perpendicular distance in device px.
fn below(curve: vec2<f32>, y: f32, size: vec2<f32>) -> f32 {
    let slope = curve.y * size.y / size.x;
    let distance = (y - curve.x * size.y) * inverseSqrt(1. + slope * slope);
    return clamp(0.5 + distance, 0., 1.);
}

fn band(top: vec2<f32>, bottom: vec2<f32>, fa: f32, fb: f32, y: f32, size: vec2<f32>) -> f32 {
    return abs(below(top + (bottom - top) * fa, y, size) - below(top + (bottom - top) * fb, y, size));
}

fn veil_profile(v: f32) -> f32 {
    let floor_value = exp(-3.5);
    return (exp(-3.5 * v) - floor_value) / (1. - floor_value);
}

fn assoc(c: vec4<f32>) -> vec4<f32> {
    return vec4(c.rgb * c.a, c.a);
}

fn over(dst: vec3<f32>, color: vec3<f32>, alpha: f32) -> vec3<f32> {
    let a = clamp(alpha, 0., 1.);
    return saturate(color) * a + dst * (1. - a);
}

fn soft_light(base: vec3<f32>, s: f32) -> vec3<f32> {
    let d = select(sqrt(base), ((16. * base - 12.) * base + 4.) * base, base <= vec3(0.25));
    return select(base + (2. * s - 1.) * (d - base), base - (1. - 2. * s) * base * (1. - base),
        s <= 0.5);
}

fn grain_signal(pixel: vec2<f32>, r: u32) -> f32 {
    let cell = bitcast<vec2<u32>>(vec2<i32>(floor(pixel)));
    var h = ((cell.x + r * 7919u) * 0x9e3779b9u) ^ ((cell.y + r * 104729u) * 0x85ebca6bu);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h & 0x00ffffffu) / 16777216.;
}

@fragment
fn pastel_fragment(input: VertexOut) -> @location(0) vec4<f32> {
    let size = max(uniforms.extent.xy, vec2(1.));
    let pixel = input.position.xy;
    let uv = pixel / size;
    let u = clamp(uv.x, 0., 1.);
    let y = pixel.y;
    let scale = uniforms.frame.z;
    let grain = uniforms.frame.y > 0.5;
    let column = min(u32(pixel.x), u32(size.x) - 1u);

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
        // Skip ribbons that cannot touch this row: every layer below would
        // composite with zero alpha.
        let extra = columns[(column * 9u + r) * 2u + 1u];
        if (y < extra.y || y > extra.z) {
            continue;
        }
        let edges = columns[(column * 9u + r) * 2u];
        let top = edges.xy;
        let bottom = edges.zw;
        let style = RIB_STYLE[r];
        let col = style.rgb;
        let alpha = style.a;

        // Fill: Brush::mesh over the fill path's bounds.
        let fill_bounds = uniforms.bounds[r / 2u];
        let even = (r & 1u) == 0u;
        let lo = select(fill_bounds.z, fill_bounds.x, even);
        let hi = select(fill_bounds.w, fill_bounds.y, even);
        let mesh_uv = saturate(vec2(u, (uv.y - lo) / max(hi - lo, 1e-6)));
        let h = mesh_uv * mesh_uv * (3. - 2. * mesh_uv);
        let c0 = vec4(min(col.r, 0.75), min(col.g + 0.14, 0.90), min(col.b + 0.10, 1.), alpha * 0.40);
        let c1 = vec4(min(col.r + 0.25, 1.), col.g * 0.82, min(col.b + 0.03, 1.), alpha * 0.65);
        let c2 = vec4(col.r * 0.65, col.g * 0.93, col.b, alpha);
        let c3 = vec4(min(col.r + 0.13, 1.), col.g * 0.67, col.b * 0.94, alpha * 0.90);
        let pm = mix(mix(assoc(c0), assoc(c1), h.x), mix(assoc(c2), assoc(c3), h.x), h.y);
        var fill = pm.rgb / max(pm.a, 1e-6);
        if (grain) {
            fill = mix(fill, soft_light(fill, grain_signal(pixel, r)), 0.035);
        }
        acc = over(acc, fill, pm.a * band(top, bottom, 0., 1., y, size));

        if (r >= 5u) {
            let s = extra.x;
            // Inward bleed from the top edge with an exponential depth profile.
            let thickness = max(bottom.x - top.x, 1e-5);
            let depth = (uv.y - top.x) / thickness;
            let reach = 0.12 + 0.43 * s;
            let density = 0.32 + 0.95 * s;
            let veil = below(top, y, size)
                * (1. - exp(-density * veil_profile(clamp(depth / reach, 0., 1.))));
            acc = over(acc, mix(vec3(0.20, 0.43, 0.70), vec3(0.43, 0.19, 0.60), u), veil);

            // Variable-width rim along the top edge's normal, -0.2..0.8 of width.
            let slope = top.y * size.y / size.x;
            let normal = (y - top.x * size.y) * inverseSqrt(1. + slope * slope);
            let width = (0.35 + 2.25 * s) * scale;
            let rim = clamp(min(0.8 * width, normal + 0.5) - max(-0.2 * width, normal - 0.5), 0., 1.);
            let rim_color = mix(vec3(0.18, 0.46, 0.66), vec3(0.63, 0.20, 0.53), u) * (1. - 0.28 * s);
            acc = over(acc, rim_color, rim * (0.12 + 0.65 * s));
        }

        let lip_alpha = select(0.20, 0.10, r >= 5u);
        acc = over(acc, vec3(0.98, 0.98, 1.), lip_alpha * band(top, bottom, 0.006, 0.016, y, size));
        if (grain) {
            var threads = 0.;
            for (var k = 0u; k < 5u; k += 1u) {
                threads += band(top, bottom, THREADS[k], THREADS[k] + 0.004, y, size);
            }
            acc = over(acc, vec3(1., 0.96, 1.), 0.045 * min(threads, 1.));
        }
    }
    // Opaque, so straight and associated alpha agree.
    return vec4(saturate(acc), 1.);
}
