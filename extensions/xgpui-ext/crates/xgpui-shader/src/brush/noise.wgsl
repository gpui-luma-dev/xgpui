fn cell_hash(cell: vec2<i32>) -> u32 {
    var h = (bitcast<u32>(cell.x) * 0x9e3779b9u) ^ (bitcast<u32>(cell.y) * 0x85ebca6bu);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    return h ^ (h >> 16u);
}
fn hash_unit(cell: vec2<i32>) -> f32 {
    return f32(cell_hash(cell) & 0x00ffffffu) / 16777216.;
}
fn simplex_gradient(cell: vec2<i32>) -> vec2<f32> {
    let h = cell_hash(cell) & 7u;
    if (h == 0u) { return vec2(1.,0.); }
    if (h == 1u) { return vec2(-1.,0.); }
    if (h == 2u) { return vec2(0.,1.); }
    if (h == 3u) { return vec2(0.,-1.); }
    if (h == 4u) { return vec2(0.70710678,0.70710678); }
    if (h == 5u) { return vec2(-0.70710678,0.70710678); }
    if (h == 6u) { return vec2(0.70710678,-0.70710678); }
    return vec2(-0.70710678,-0.70710678);
}
fn simplex_corner(cell: vec2<i32>, offset: vec2<f32>) -> f32 {
    let weight = max(0.5-dot(offset,offset),0.);
    return weight*weight*weight*weight * dot(simplex_gradient(cell),offset);
}
fn simplex_signal(position: vec2<f32>) -> f32 {
    let skew = (position.x+position.y) * 0.3660254;
    let cell = vec2<i32>(floor(position+vec2(skew)));
    let unskew = f32(cell.x+cell.y) * 0.21132487;
    let first = position-vec2<f32>(cell)+vec2(unskew);
    let step = select(vec2<i32>(0,1),vec2<i32>(1,0),first.x > first.y);
    let second = first-vec2<f32>(step)+vec2(0.21132487);
    let third = first-vec2(1.)+vec2(0.42264974);
    let value = 99.2 * (simplex_corner(cell,first)+simplex_corner(cell+step,second)+simplex_corner(cell+vec2<i32>(1),third));
    return clamp(0.5+0.5*value,0.,1.);
}
fn worley_signal(position: vec2<f32>) -> f32 {
    let cell = vec2<i32>(floor(position));
    let local = fract(position);
    var nearest = 32.;
    var second = 32.;
    // With one feature per cell, the own cell and a neighboring cell provide
    // two features within sqrt(3.25). Cells beyond this 5x5 neighborhood are
    // at least two units away, so they cannot change either nearest distance.
    for (var y = -2; y <= 2; y += 1) {
        for (var x = -2; x <= 2; x += 1) {
            let offset = vec2<i32>(x,y);
            let feature = vec2(hash_unit(cell+offset),hash_unit(cell+offset+vec2<i32>(113,271)));
            let delta = vec2<f32>(offset)+feature-local;
            let distance = dot(delta,delta);
            second = min(second,max(nearest,distance));
            nearest = min(nearest,distance);
        }
    }
    return clamp(sqrt(select(nearest,second,NOISE_KIND == 3u))/1.41421356,0.,1.);
}
fn dither_signal(position: vec2<f32>) -> f32 {
    // Wrap before integer conversion, including negative and large coordinates.
    let cell = vec2<u32>(floor(position-floor(position/16.)*16.));
    return (f32(BLUE_NOISE[cell.y*16u+cell.x])+0.5)/256.;
}
fn noise_signal(input: ShaderInput, frequency: f32) -> f32 {
    if (NOISE_KIND == 2u) { let physical = input.position * input.scale_factor;
        return dither_signal(physical); }
    // Bound lattice coordinates before integer conversion, including very large
    // receivers. Noise repeats at 65536 logical pixels; clipping never changes it.
    var position = (input.position - floor(input.position / 65536.) * 65536.) * frequency;
    var weight = 1.;
    var total = 0.;
    var normalization = 0.;
    for (var octave = 0u; octave < NOISE_OCTAVES; octave += 1u) {
        var value = 0.;
        if (NOISE_KIND == 0u) { value = simplex_signal(position); }
        else { value = worley_signal(position); }
        total += value * weight;
        normalization += weight;
        position *= 2.;
        weight *= 0.5;
    }
    return total / normalization;
}
fn blend_noise(base: vec3<f32>, signal: f32) -> vec3<f32> {
    let noise = vec3(signal);
    if (BLEND_MODE == 0u) { return noise; }
    if (BLEND_MODE == 1u) { return base * noise; }
    if (BLEND_MODE == 2u) { return 1.-(1.-base)*(1.-noise); }
    if (BLEND_MODE == 3u) { return select(1.-2.*(1.-base)*(1.-noise),2.*base*noise,base <= vec3(0.5)); }
    if (BLEND_MODE == 4u) { let denominator = select(1.-signal,1.,signal >= 1.);
        let result = select(min(vec3(1.),base/denominator),vec3(1.),noise >= vec3(1.));
        return select(result,vec3(0.),base <= vec3(0.)); }
    if (BLEND_MODE == 5u) {
        let d = select(sqrt(base),((16.*base-12.)*base+4.)*base,base <= vec3(0.25));
        return select(base+(2.*signal-1.)*(d-base),base-(1.-2.*signal)*base*(1.-base),noise <= vec3(0.5));
    }
    return max(vec3(0.),base+noise-1.);
}
fn brush_color(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let base = base_color(input,p);
    let signal = noise_signal(input,p.options.z);
    return vec4(mix(base.rgb,blend_noise(base.rgb,signal),p.options.w),base.a);
}
