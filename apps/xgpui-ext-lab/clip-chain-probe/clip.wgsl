// Replaced by the harness: storage-buffer or uint-texture record loading.
// Each shape: bounds xywh, horizontal radii TL/TR/BR/BL,
// vertical radii TL/TR/BR/BL, kind (0=rounded rect, 1=ellipse).
__TRANSPORT__
@group(0) @binding(1) var<uniform> params: vec4<u32>;
fn inside(p: vec2<f32>, index: u32) -> bool {
    let b = record(index * 4u);
    let q = p - b.xy;
    if (any(q < vec2<f32>(0.0)) || any(q > b.zw)) { return false; }
    if (record(index * 4u + 3u).x == 1.0) {
        let d = (q - b.zw * 0.5) / (b.zw * 0.5);
        return dot(d, d) <= 1.0;
    }
    let rx = record(index * 4u + 1u);
    let ry = record(index * 4u + 2u);
    let centers = array<vec2<f32>, 4>(
        vec2<f32>(rx.x, ry.x), vec2<f32>(b.z - rx.y, ry.y),
        vec2<f32>(b.z - rx.z, b.w - ry.z), vec2<f32>(rx.w, b.w - ry.w));
    for (var c = 0u; c < 4u; c++) {
        if (rx[c] == 0.0 || ry[c] == 0.0) { continue; }
        let d = q - centers[c];
        let sx = select(-1.0, 1.0, c == 1u || c == 2u);
        let sy = select(-1.0, 1.0, c >= 2u);
        if (d.x * sx > 0.0 && d.y * sy > 0.0) {
            let e = d / vec2<f32>(rx[c], ry[c]);
            if (dot(e, e) > 1.0) { return false; }
        }
    }
    return true;
}
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 3>(vec2<f32>(-1.0,-1.0), vec2<f32>(3.0,-1.0), vec2<f32>(-1.0,3.0));
    return vec4<f32>(p[i], 0.0, 1.0);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) f32 {
    var coverage = 0.0;
    // Intersection BEFORE coverage: identical clips do not square alpha.
    for (var y = 0u; y < 4u; y++) {
        for (var x = 0u; x < 4u; x++) {
            let s = floor(p.xy) + (vec2<f32>(f32(x), f32(y)) + 0.5) / 4.0;
            var visible = true;
            for (var c = 0u; c < params.x; c++) {
                visible = visible && inside(s, c);
            }
            coverage += select(0.0, 0.0625, visible);
        }
    }
    return coverage;
}
