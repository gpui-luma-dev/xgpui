//! Demo-only analytic shader previews while the portable wgpu executor is pending.
//! This uses the existing public shader-paint API, never a backend context.
use xgpui_shader::{
    Gradient, GradientKind, Paint,
    shader_paint::{ShaderParameters, ShaderProgram},
};
pub fn paint(gradient: &Gradient) -> Paint {
    let (kind, center, radius, angle) = match gradient.kind() {
        GradientKind::Linear { angle } => (0, [0.5; 2], [0.5; 2], angle),
        GradientKind::Radial { center, radius } => (1, center, radius, 0.),
        GradientKind::Conic { center, angle } => (2, center, [0.5; 2], angle),
        GradientKind::Diamond {
            center,
            radius,
            angle,
        } => (3, center, radius, angle),
    };
    let radians = angle.rem_euclid(360.).to_radians();
    let direction = if kind == 0 {
        [radians.sin(), -radians.cos(), radians, 0.]
    } else {
        [radians.cos(), radians.sin(), radians, 0.]
    };
    let values = |v: [f32; 4]| format!("vec4<f32>({:?},{:?},{:?},{:?})", v[0], v[1], v[2], v[3]);
    let stops = gradient
        .stops()
        .iter()
        .map(|stop| {
            format!(
                "Stop({}, {})",
                values(stop.color),
                values([stop.offset, 0., 0., 0.])
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let segments = (1..gradient.stops().len())
        .map(|i| {
            format!(
                r#"
    if ratio<stops[{i}].offset.x {{
        let t=(ratio-stops[{prev}].offset.x)/(stops[{i}].offset.x-stops[{prev}].offset.x);
        let a=stops[{prev}].color;let b=stops[{i}].color;let alpha=mix(a.a,b.a,t);
        if alpha>0. {{return vec4(mix(a.rgb*a.a,b.rgb*b.a,t)/alpha,alpha);}}
        return vec4(0.);
    }}
"#,
                prev = i - 1
            )
        })
        .collect::<String>();
    let source = format!(
        r#"
struct Parameters {{ dummy: f32 }}
struct Stop {{ color: vec4<f32>, offset: vec4<f32> }}
fn paint(input: PaintInput) -> vec4<f32> {{
    let stops=array<Stop,{count}>({stops});
    let center_radius={center};let direction={direction};
    let size=input.size;let position=input.position;let uv=input.uv;
    let delta=position-center_radius.xy*size;
    var ratio=parameters.dummy;
    if {kind}u==0u {{ratio+=0.5+dot(position-size*0.5,direction.xy)/dot(abs(direction.xy),size);}}
    else if {kind}u==1u {{ratio+=length((uv-center_radius.xy)/center_radius.zw);}}
    else if {kind}u==2u {{if dot(delta,delta)>0. {{ratio+=fract((atan2(delta.y,delta.x)+1.57079632679-direction.z)/6.28318530718+1.);}}}}
    else {{let v=(uv-center_radius.xy)/center_radius.zw;let rotated=vec2(v.x*direction.x+v.y*direction.y,-v.x*direction.y+v.y*direction.x);ratio+=abs(rotated.x)+abs(rotated.y);}}
    if ratio<stops[0].offset.x {{return stops[0].color;}}
    {segments}
    return stops[{last}].color;
}}
"#,
        count = gradient.stops().len(),
        last = gradient.stops().len() - 1,
        center = values([center[0], center[1], radius[0], radius[1]]),
        direction = values(direction)
    );
    ShaderProgram::new(&source)
        .expect("valid bundled analytic preview")
        .paint(ShaderParameters::new().float("dummy", 0.))
        .unwrap()
        .into()
}
