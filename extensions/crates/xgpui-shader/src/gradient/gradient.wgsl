struct Stop { color: vec4<f32>, offset: vec4<f32> }
struct Params { size_kind_count: vec4<f32>, center_radius: vec4<f32>, direction: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
struct StopTable {
    data0: mat4x4<f32>,
    data1: mat4x4<f32>,
    data2: mat4x4<f32>,
    data3: mat4x4<f32>,
    data4: mat4x4<f32>,
    data5: mat4x4<f32>,
    data6: mat4x4<f32>,
    data7: mat4x4<f32>,
    data8: mat4x4<f32>,
    data9: mat4x4<f32>,
    data10: mat4x4<f32>,
    data11: mat4x4<f32>,
    data12: mat4x4<f32>,
    data13: mat4x4<f32>,
    data14: mat4x4<f32>,
    data15: mat4x4<f32>,
}
@group(0) @binding(1) var<uniform> stops: StopTable;
fn stop_at(index: u32) -> Stop {
    var data: mat4x4<f32>;
    switch index / 2u {
        case 0u: { data = stops.data0; }
        case 1u: { data = stops.data1; }
        case 2u: { data = stops.data2; }
        case 3u: { data = stops.data3; }
        case 4u: { data = stops.data4; }
        case 5u: { data = stops.data5; }
        case 6u: { data = stops.data6; }
        case 7u: { data = stops.data7; }
        case 8u: { data = stops.data8; }
        case 9u: { data = stops.data9; }
        case 10u: { data = stops.data10; }
        case 11u: { data = stops.data11; }
        case 12u: { data = stops.data12; }
        case 13u: { data = stops.data13; }
        case 14u: { data = stops.data14; }
        default: { data = stops.data15; }
    }
    let column = (index % 2u) * 2u;
    return Stop(data[column], data[column + 1u]);
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
    if ratio < stop_at(0).offset.x { return stop_at(0).color; }
    for (var i=1u; i<count; i++) {
        if ratio < stop_at(i).offset.x {
            let t = (ratio-stop_at(i-1u).offset.x)/(stop_at(i).offset.x-stop_at(i-1u).offset.x);
            let a = stop_at(i-1u).color;
            let b = stop_at(i).color;
            let alpha = mix(a.a,b.a,t);
            let premultiplied = mix(a.rgb*a.a,b.rgb*b.a,t);
            if alpha > 0. { return vec4(premultiplied/alpha,alpha); }
            return vec4(0.);
        }
    }
    return stop_at(count-1u).color;
}
