// D65 matrices, column-major WGSL constructors. Source: CSS Color 4.
// Space IDs: 0=sRGB, 1=Display P3, 2=Rec.2020, 3=Adobe RGB (1998). Inputs must be finite.
fn gamut_to_xyz(v: vec3<f32>, space: u32) -> vec3<f32> {
 switch space {
 case 0u: { return mat3x3<f32>(vec3<f32>(0.412390799266, 0.212639005872, 0.019330818716), vec3<f32>(0.357584339384, 0.715168678768, 0.119194779795), vec3<f32>(0.180480788402, 0.072192315361, 0.950532152250)) * v; }
 case 1u: { return mat3x3<f32>(vec3<f32>(0.486570948648, 0.228974564070, 0.000000000000), vec3<f32>(0.265667693169, 0.691738521837, 0.045113381859), vec3<f32>(0.198217285234, 0.079286914094, 1.043944368901)) * v; }
 case 2u: { return mat3x3<f32>(vec3<f32>(0.636958048301, 0.262700212011, 0.000000000000), vec3<f32>(0.144616903586, 0.677998071519, 0.028072693049), vec3<f32>(0.168880975164, 0.059301716470, 1.060985057711)) * v; }
 case 3u: { return mat3x3<f32>(vec3<f32>(0.576669042910, 0.297344975251, 0.027031361386), vec3<f32>(0.185558237907, 0.627363566255, 0.070688852536), vec3<f32>(0.188228646235, 0.075291458494, 0.991337536838)) * v; }
 default: { return v; }
 }
}
fn gamut_from_xyz(v: vec3<f32>, space: u32) -> vec3<f32> {
 switch space {
 case 0u: { return mat3x3<f32>(vec3<f32>(3.240969941905, -0.969243636281, 0.055630079697), vec3<f32>(-1.537383177570, 1.875967501508, -0.203976958889), vec3<f32>(-0.498610760293, 0.041555057407, 1.056971514243)) * v; }
 case 1u: { return mat3x3<f32>(vec3<f32>(2.493496911941, -0.829488969562, 0.035845830244), vec3<f32>(-0.931383617919, 1.762664060318, -0.076172389268), vec3<f32>(-0.402710784451, 0.023624685842, 0.956884524008)) * v; }
 case 2u: { return mat3x3<f32>(vec3<f32>(1.716651187971, -0.666684351832, 0.017639857445), vec3<f32>(-0.355670783776, 1.616481236635, -0.042770613258), vec3<f32>(-0.253366281374, 0.015768545814, 0.942103121235)) * v; }
 case 3u: { return mat3x3<f32>(vec3<f32>(2.041587903811, -0.969243636281, 0.013444280632), vec3<f32>(-0.565006974279, 1.875967501508, -0.118362392231), vec3<f32>(-0.344731350778, 0.041555057407, 1.015174994391)) * v; }
 default: { return v; }
 }
}
fn gamut_convert(v: vec3<f32>, source: u32, destination: u32) -> vec3<f32> {
    if source == destination { return v; }
    return gamut_from_xyz(gamut_to_xyz(v, source), destination);
}
fn gamut_decode_channel(v: f32, space: u32) -> f32 {
    let x = abs(v);
    if space == 3u { return sign(v) * pow(x, 563.0 / 256.0); }
    if space == 2u {
        if x < 0.0812428582986315 { return v / 4.5; }
        return sign(v) * pow((x + 0.09929682680944) / 1.09929682680944, 1.0 / 0.45);
    }
    if x <= 0.04045 { return v / 12.92; }
    return sign(v) * pow((x + 0.055) / 1.055, 2.4);
}
fn gamut_encode_channel(v: f32, space: u32) -> f32 {
    let x = abs(v);
    if space == 3u { return sign(v) * pow(x, 256.0 / 563.0); }
    if space == 2u {
        if x < 0.018053968510807 { return v * 4.5; }
        return sign(v) * (1.09929682680944 * pow(x, 0.45) - 0.09929682680944);
    }
    if x <= 0.0031308 { return v * 12.92; }
    return sign(v) * (1.055 * pow(x, 1.0 / 2.4) - 0.055);
}
fn gamut_decode(v: vec3<f32>, space: u32) -> vec3<f32> {
    return vec3(gamut_decode_channel(v.r, space), gamut_decode_channel(v.g, space), gamut_decode_channel(v.b, space));
}
fn gamut_encode(v: vec3<f32>, space: u32) -> vec3<f32> {
    return vec3(gamut_encode_channel(v.r, space), gamut_encode_channel(v.g, space), gamut_encode_channel(v.b, space));
}
fn gamut_cbrt(v: vec3<f32>) -> vec3<f32> { return sign(v) * pow(abs(v), vec3(1.0 / 3.0)); }
fn gamut_to_oklab(rgb: vec3<f32>, space: u32) -> vec3<f32> {
    let v = gamut_convert(rgb, space, 0u);
    let lms = gamut_cbrt(vec3(
        dot(v, vec3(0.4122214708, 0.5363325363, 0.0514459929)),
        dot(v, vec3(0.2119034982, 0.6806995451, 0.1073969566)),
        dot(v, vec3(0.0883024619, 0.2817188376, 0.6299787005))));
    return vec3(dot(lms, vec3(0.2104542553, 0.7936177850, -0.0040720468)),
        dot(lms, vec3(1.9779984951, -2.4285922050, 0.4505937099)),
        dot(lms, vec3(0.0259040371, 0.7827717662, -0.8086757660)));
}
fn gamut_from_oklab(lab: vec3<f32>, space: u32) -> vec3<f32> {
    let root = vec3(dot(lab, vec3(1.0, 0.3963377774, 0.2158037573)),
        dot(lab, vec3(1.0, -0.1055613458, -0.0638541728)),
        dot(lab, vec3(1.0, -0.0894841775, -1.2914855480)));
    let lms = root * root * root;
    return gamut_convert(vec3(dot(lms, vec3(4.0767416621, -3.3077115913, 0.2309699292)),
        dot(lms, vec3(-1.2684380046, 2.6097574011, -0.3413193965)),
        dot(lms, vec3(-0.0041960863, -0.7034186147, 1.7076147010))), 0u, space);
}
fn gamut_to_oklch(lab: vec3<f32>) -> vec3<f32> {
    let c = length(lab.yz);
    if c == 0.0 { return vec3(lab.x, 0.0, 0.0); }
    let h = degrees(atan2(lab.z, lab.y));
    return vec3(lab.x, c, (h % 360.0 + 360.0) % 360.0);
}
fn gamut_from_oklch(lch: vec3<f32>) -> vec3<f32> {
    let h = radians(lch.z);
    return vec3(lch.x, lch.y * cos(h), lch.y * sin(h));
}
fn gamut_in_bounds(v: vec3<f32>) -> bool { return all(v >= vec3(0.0)) && all(v <= vec3(1.0)); }
// Policy: 0=clip, 1=bounded fixed-L/h chroma reduction (not CSS MINDE).
fn gamut_map(v: vec3<f32>, space: u32, policy: u32) -> vec3<f32> {
    if gamut_in_bounds(v) { return v; }
    if policy == 0u { return clamp(v, vec3(0.0), vec3(1.0)); }
    let lch = gamut_to_oklch(gamut_to_oklab(v, space));
    if lch.x <= 0.0 { return vec3(0.0); }
    if lch.x >= 1.0 { return vec3(1.0); }
    var low = 0.0;
    var high = lch.y;
    var best = clamp(gamut_from_oklab(vec3(lch.x, 0.0, 0.0), space), vec3(0.0), vec3(1.0));
    for (var i = 0u; i < 64u; i++) {
        if high - low <= 0.0000001 { break; }
        let mid = low + (high - low) * 0.5;
        let candidate = gamut_from_oklab(gamut_from_oklch(vec3(lch.x, mid, lch.z)), space);
        if gamut_in_bounds(candidate) { low = mid; best = candidate; } else { high = mid; }
    }
    return best;
}
// Hue policy: 0=shorter, 1=longer, 2=increasing, 3=decreasing.
fn gamut_hue_delta(a: f32, b: f32, policy: u32) -> f32 {
    let d = b - a;
    switch policy {
        case 0u: { if d > 180.0 { return d - 360.0; } if d < -180.0 { return d + 360.0; } }
        case 1u: { if d > 0.0 && d < 180.0 { return d - 360.0; } if d > -180.0 && d <= 0.0 { return d + 360.0; } }
        case 2u: { if d < 0.0 { return d + 360.0; } }
        default: { if d > 0.0 { return d - 360.0; } }
    }
    return d;
}
// Straight linear RGB stops in output primaries. Model: 0=linear, 1=OkLab, 2=OkLCh.
// Returns straight linear RGB; mapping and output association remain explicit.
fn gamut_interpolate(a: vec4<f32>, b: vec4<f32>, position: f32, space: u32, model: u32, hue: u32) -> vec4<f32> {
    let t = clamp(position, 0.0, 1.0);
    let alpha = mix(a.a, b.a, t);
    if alpha == 0.0 { return vec4(0.0); }
    if t == 0.0 { return a; }
    if t == 1.0 { return b; }
    var x = a.rgb; var y = b.rgb;
    if model != 0u { x = gamut_to_oklab(x, space); y = gamut_to_oklab(y, space); }
    if model == 2u {
        x = gamut_to_oklch(x); y = gamut_to_oklch(y);
        let powerless_a = x.y <= 0.0000001 || a.a == 0.0;
        let powerless_b = y.y <= 0.0000001 || b.a == 0.0;
        if powerless_a && powerless_b { x.z = 0.0; y.z = 0.0; }
        else {
            if powerless_a { x.z = y.z; }
            if powerless_b { y.z = x.z; }
            y.z = x.z + gamut_hue_delta(x.z, y.z, hue);
        }
    }
    var value = mix(x * a.a, y * b.a, t) / alpha;
    if model == 2u { value.z = mix(x.z, y.z, t); value = gamut_from_oklch(value); }
    if model != 0u { value = gamut_from_oklab(value, space); }
    return vec4(value, alpha);
}
