struct Stop { color: vec4<f32>, offset: vec4<f32> }
struct Params { size_kind_count: vec4<f32>, center_radius: vec4<f32>, direction: vec4<f32>, stops: array<Stop,32> }
@group(0) @binding(0) var<uniform> p: Params;
@vertex fn paint_vertex(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let vertices = array<vec2<f32>,6>(vec2(-1.,-1.),vec2(1.,-1.),vec2(-1.,1.),vec2(-1.,1.),vec2(1.,-1.),vec2(1.,1.));
    return vec4(vertices[id],0.,1.);
}
@fragment fn paint_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = p.size_kind_count.xy;
    let uv = position.xy / size;
    let delta = position.xy - p.center_radius.xy * size;
    let kind = u32(p.size_kind_count.z);
    var ratio = 0.;
    if kind == 0u { ratio = 0.5 + dot(position.xy-size*0.5,p.direction.xy)/dot(abs(p.direction.xy),size); }
    else if kind == 1u { ratio = length((uv-p.center_radius.xy)/p.center_radius.zw); }
    else if kind == 2u {
        if dot(delta,delta) > 0. { ratio = fract((atan2(delta.y,delta.x)+1.57079632679-p.direction.z)/6.28318530718+1.); }
    } else {
        let v = (uv-p.center_radius.xy)/p.center_radius.zw;
        let rotated = vec2(v.x*p.direction.x+v.y*p.direction.y,-v.x*p.direction.y+v.y*p.direction.x);
        ratio = abs(rotated.x)+abs(rotated.y);
    }
    let count = u32(p.size_kind_count.w);
    if ratio < p.stops[0].offset.x { return p.stops[0].color; }
    for (var i=1u; i<count; i++) {
        if ratio < p.stops[i].offset.x {
            let t = (ratio-p.stops[i-1u].offset.x)/(p.stops[i].offset.x-p.stops[i-1u].offset.x);
            let a = p.stops[i-1u].color;
            let b = p.stops[i].color;
            let alpha = mix(a.a,b.a,t);
            let premultiplied = mix(a.rgb*a.a,b.rgb*b.a,t);
            if alpha > 0. { return vec4(premultiplied/alpha,alpha); }
            return vec4(0.);
        }
    }
    return p.stops[count-1u].color;
}
