struct Params { bounds: vec4<f32>, mask: vec4<f32>, radii: vec4<f32>, viewport: vec4<f32>, color_r: vec4<f32>, color_g: vec4<f32>, color_b: vec4<f32>, vignette: vec4<f32>, linear: vec4<f32>, placement: vec4<f32>, source_uv: vec4<f32>, source_clamp: vec4<f32>, sampling: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var paint_sampler: sampler;
@group(0) @binding(4) var image_sampler: sampler;
@group(0) @binding(2) var image: texture_2d<f32>;
@group(0) @binding(3) var shape_mask: texture_2d<f32>;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn custom_vertex(@builtin(vertex_index) id: u32) -> Vertex {
    let vertices = array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
    let uv = vertices[id];
    let xy = p.bounds.xy + uv * p.bounds.zw;
    return Vertex(vec4(xy.x / p.viewport.x * 2. - 1., 1. - xy.y / p.viewport.y * 2., 0., 1.), uv);
}
fn decode_srgb(c: f32) -> f32 {
    return select(c / 12.92, pow((c + 0.055) / 1.055, 2.4), c > 0.04045);
}
fn encode_srgb(c: f32) -> f32 {
    return select(12.92 * c, 1.055 * pow(c, 1.0 / 2.4) - 0.055, c > 0.0031308);
}
fn p3_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return vec3(dot(c,vec3(1.22494018,-0.22494018,0.)),dot(c,vec3(-0.04205695,1.04205695,0.)),dot(c,vec3(-0.01963755,-0.07863605,1.09827360)));
}
fn srgb_to_p3(c: vec3<f32>) -> vec3<f32> {
    return vec3(dot(c,vec3(0.82246197,0.17753803,0.)),dot(c,vec3(0.03319420,0.96680580,0.)),dot(c,vec3(0.01708263,0.07239744,0.91051993)));
}
// Linear SDR Rec.2020 -> P3; retain extended channels until preview mapping.
fn rec2020_to_p3(c: vec3<f32>) -> vec3<f32> {
    return vec3(dot(c,vec3(1.343578253,-0.282179671,-0.061398582)),dot(c,vec3(-0.065297453,1.075787916,-0.010490463)),dot(c,vec3(0.002821787,-0.019598495,1.016776707)));
}
@fragment fn custom_fragment(in: Vertex) -> @location(0) vec4<f32> {
    if any(in.position.xy < p.mask.xy) || any(in.position.xy >= p.mask.xy + p.mask.zw) { discard; }
    let local = in.uv * p.bounds.zw;
    let right = local.x > p.bounds.z * 0.5;
    let bottom = local.y > p.bounds.w * 0.5;
    let r = select(select(p.radii.x, p.radii.y, right), select(p.radii.w, p.radii.z, right), bottom);
    let q = abs(local - p.bounds.zw * 0.5) - p.bounds.zw * 0.5 + r;
    let distance = length(max(q, vec2(0.))) + min(max(q.x,q.y),0.) - r;
    var coverage = clamp(0.5 - distance,0.,1.);
    if (u32(p.viewport.w) & 1u) != 0u { coverage = textureSample(shape_mask,paint_sampler,in.uv).r; }
    let mapped = (in.uv - p.placement.xy) / p.placement.zw;
    let raw_uv = p.source_uv.xy + mapped * p.source_uv.zw;
    let dx = dpdx(raw_uv); let dy = dpdy(raw_uv);
    var lo = p.source_clamp.xy; var hi = p.source_clamp.zw;
    if p.sampling.x > 0.5 {
        let dims = vec2<f32>(textureDimensions(image, 0));
        let footprint = max(length(dx * dims), length(dy * dims));
        let lod = clamp(log2(max(footprint, 1.)), 0., p.sampling.w);
        let level = u32(select(floor(lod + 0.5), ceil(lod), p.sampling.x > 1.5));
        let half_texel = vec2(0.5) / vec2<f32>(textureDimensions(image, level));
        let base_half = vec2(0.5) / dims;
        let center = (lo + hi) * 0.5;
        lo = min(lo - base_half + half_texel, center);
        hi = max(hi + base_half - half_texel, center);
    }
    let uv = clamp(raw_uv, lo, hi);
    var color = textureSampleGrad(image,image_sampler,uv,dx,dy);
    if any(mapped < vec2(0.)) || any(mapped >= vec2(1.)) { discard; }
    if p.linear.w>0.5 {
        if p.linear.w>4.5 {color=vec4(1.,1.,1.,color.r);}
        else {
            if p.linear.z<0.5 {color.a=1.;}
            if p.linear.z>1.5 {if color.a>0. {color=vec4(color.rgb/color.a,color.a);}else{color=vec4(0.);}}
            var rgb = color.rgb;
            if p.linear.w>3.5 {rgb=rec2020_to_p3(rgb);}
            if p.sampling.z>1.5 {
                // Decode/unassociate first, then convert to the chosen working primaries.
                if p.linear.w<1.5 {rgb=vec3(decode_srgb(rgb.r),decode_srgb(rgb.g),decode_srgb(rgb.b));}
                if p.linear.w>2.5 && p.sampling.y<0.5 {rgb=p3_to_srgb(rgb);}
                if p.linear.w<2.5 && p.sampling.y>0.5 {rgb=srgb_to_p3(rgb);}
                let v=vec4(rgb,1.);
                rgb=vec3(dot(p.color_r,v),dot(p.color_g,v),dot(p.color_b,v));
                let dims=vec2<f32>(textureDimensions(image,0));
                let q=raw_uv*2.-vec2(1.);
                let d=mix(length(q),length(q*dims/min(dims.x,dims.y)),p.vignette.w);
                let t=clamp((d-p.vignette.y)/p.vignette.z,0.,1.);
                rgb*=1.-p.vignette.x*t*t*(3.-2.*t);
                if p.sampling.y>0.5 {rgb=p3_to_srgb(rgb);}
                let c=clamp(rgb,vec3(0.),vec3(1.));
                rgb=vec3(encode_srgb(c.r),encode_srgb(c.g),encode_srgb(c.b));
            } else if p.sampling.z>0.5 {
                let v=vec4(rgb,1.);
                rgb=clamp(vec3(dot(p.color_r,v),dot(p.color_g,v),dot(p.color_b,v)),vec3(0.),vec3(1.));
            } else if p.linear.w>1.5 {
                if p.linear.w>2.5 {rgb=p3_to_srgb(rgb);}
                let c=clamp(rgb,vec3(0.),vec3(1.));
                rgb=vec3(encode_srgb(c.r),encode_srgb(c.g),encode_srgb(c.b));
            }
            color=vec4(select(vec3(0.),rgb,color.a>0.),color.a);
        }
        return vec4(color.rgb,color.a*coverage*p.viewport.z);
    }
    if p.linear.y > 2.5 { color = vec4(rec2020_to_p3(color.rgb), color.a); }
    if p.viewport.w > 1.5 && color.a > 0. { color = vec4(color.rgb / color.a, color.a); }
    let processing = p.linear.x > 0.5 || p.linear.y > 0.5;
    var rgb = color.rgb;
    if p.linear.y > 0.5 {
        rgb = vec3(0.);
        if color.a > 0. { rgb = color.rgb / color.a; }
        if p.linear.z < 0.5 { rgb = p3_to_srgb(rgb); }
    } else if processing {
        let c = clamp(rgb, vec3(0.), vec3(1.));
        rgb = vec3(decode_srgb(c.r), decode_srgb(c.g), decode_srgb(c.b));
        if p.linear.z > 0.5 { rgb = srgb_to_p3(rgb); }
    }
    color = vec4(rgb, color.a);
    let rgb1 = vec4(color.rgb, 1.);
    color = vec4(dot(p.color_r,rgb1), dot(p.color_g,rgb1), dot(p.color_b,rgb1), color.a);
    if processing {
        let dims = vec2<f32>(textureDimensions(image, 0));
        let q = in.uv * 2. - vec2(1.);
        let ellipse = length(q);
        let circle = length(q * dims / min(dims.x, dims.y));
        let d = mix(ellipse, circle, p.vignette.w);
        let t = clamp((d - p.vignette.y) / p.vignette.z, 0., 1.);
        let factor = 1. - p.vignette.x * t * t * (3. - 2. * t);
        rgb = color.rgb * factor;
        if p.linear.z > 0.5 { rgb = p3_to_srgb(rgb); }
        let c = clamp(rgb, vec3(0.), vec3(1.));
        color = vec4(encode_srgb(c.r), encode_srgb(c.g), encode_srgb(c.b), color.a);
        if color.a == 0. { color = vec4(0.); }
    }
    return vec4(color.rgb, color.a * coverage * p.viewport.z);
}
