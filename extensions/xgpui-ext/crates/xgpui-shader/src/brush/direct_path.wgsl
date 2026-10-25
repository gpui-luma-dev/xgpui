// Exact box-filter coverage of a non-overlapping polygon triangulation, using
// Green's theorem: area = integral x dy. Shared triangle edges cancel on the
// host; remaining T-junction segments cancel in the integral without a seam.
fn edge_pixel_area(a: vec2<f32>, b: vec2<f32>) -> f32 {
 let dy = b.y - a.y;
 if dy == 0. { return 0.; }
 let low = max(min(a.y,b.y),0.);
 let high = min(max(a.y,b.y),1.);
 if high <= low { return 0.; }
 let x0 = a.x + (low-a.y) / dy * (b.x-a.x);
 let x1 = a.x + (high-a.y) / dy * (b.x-a.x);
 let left = min(x0,x1); let right = max(x0,x1);
 var integral = clamp((left+right)*0.5,0.,1.);
 let dx = right-left;
 if dx > 0.000001 {
  let t0 = clamp(-left/dx,0.,1.);
  let t1 = clamp((1.-left)/dx,0.,1.);
  integral = (t1-t0)*(clamp(left,0.,1.)+clamp(right,0.,1.))*0.5 + (1.-t1);
 }
 return sign(dy)*(high-low)*integral;
}
fn shade(input: ShaderInput,p: Parameters) -> vec4<f32> {
 var field = input;
 field.position = input.position-p.radii.xy;
 field.size = p.radii.zw;
 field.uv = field.position/field.size;
 let pixel = field.position * input.scale_factor - vec2(0.5);
 let extent = field.size * input.scale_factor;
 var area = 0.;
 for (var i = 0u; i < PATH_EDGE_COUNT; i += 1u) {
  let edge = PATH_EDGES[i];
  area += edge_pixel_area(edge.xy*extent-pixel,edge.zw*extent-pixel);
 }
 let color = brush_color(field,p);
 return vec4(color.rgb,color.a*clamp(area,0.,1.));
}
