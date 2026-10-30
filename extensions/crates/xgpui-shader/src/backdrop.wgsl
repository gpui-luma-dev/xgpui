struct Params {
 viewport: vec4<f32>, bounds: vec4<f32>, uv: vec4<f32>,
 clip: vec4<f32>, metrics: vec4<f32>, radii: vec4<f32>,
}
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(3) var original: texture_2d<f32>;
// Explicit bilinear fetches also translate to shader-model 5 without sampler heaps.
fn sample_image(tex: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
 let maximum = vec2<i32>(textureDimensions(tex)) - vec2(1);
 let position = uv * vec2<f32>(textureDimensions(tex)) - vec2(0.5);
 let lower = vec2<i32>(floor(position)); let f = fract(position);
 let a = textureLoad(tex,clamp(lower,vec2(0),maximum),0);
 let b = textureLoad(tex,clamp(lower+vec2(1,0),vec2(0),maximum),0);
 let c = textureLoad(tex,clamp(lower+vec2(0,1),vec2(0),maximum),0);
 let d = textureLoad(tex,clamp(lower+vec2(1,1),vec2(0),maximum),0);
 return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}
struct Varying { @builtin(position) screen: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn backdrop_vertex(@builtin(vertex_index) index: u32) -> Varying {
 let vertices = array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
 let uv = vertices[index];
 let position = p.bounds.xy + uv * p.bounds.zw;
 var out: Varying;
 out.screen = vec4(position / p.viewport.xy * vec2(2.,-2.) + vec2(-1.,1.),0.,1.);
 out.uv = p.uv.xy + uv * p.uv.zw;
 return out;
}
// Separable blend functions take straight RGB: destination first, source second.
fn blend_channel(b: f32, s: f32, mode: u32) -> f32 {
 switch mode {
  case 1u: { return b * s; }
  case 2u: { return b + s - b * s; }
  case 3u: {
   if b <= 0.5 { return 2. * b * s; }
   return 1. - 2. * (1. - b) * (1. - s);
  }
  case 4u: {
   if b == 0. { return 0.; }
   if s >= 1. { return 1.; }
   return min(1., b / (1. - s));
  }
  case 5u: {
   if s <= 0.5 { return b - (1. - 2. * s) * b * (1. - b); }
   var d = sqrt(b);
   if b <= 0.25 { d = ((16. * b - 12.) * b + 4.) * b; }
   return b + (2. * s - 1.) * (d - b);
  }
  case 6u: { return max(0., b + s - 1.); }
  default: { return s; }
 }
}
fn blend_group(source: vec4<f32>, destination: vec4<f32>, mode: u32) -> vec4<f32> {
 if mode == 0u { return source + destination * (1. - source.a); }
 // Transparent colors never participate in the blend term; avoid tiny-alpha
 // divisions while preserving source-only and destination-only contributions.
 if source.a <= 0. { return destination; }
 if destination.a <= 0. { return source; }
 let s = clamp(source.rgb / source.a, vec3(0.), vec3(1.));
 let b = clamp(destination.rgb / destination.a, vec3(0.), vec3(1.));
 let blended = vec3(blend_channel(b.r,s.r,mode), blend_channel(b.g,s.g,mode), blend_channel(b.b,s.b,mode));
 return vec4(source.rgb * (1. - destination.a) + destination.rgb * (1. - source.a)
     + source.a * destination.a * blended, source.a + destination.a * (1. - source.a));
}
// Area-average the source cell before horizontal filtering. For factors <=4,
// each footprint intersects at most five texels per axis, including odd sizes.
// Zero padding is applied in cell space, rather than repeating crop edges.
fn layer_cell(cell: vec2<i32>, output_size: vec2<i32>) -> vec4<f32> {
 if any(cell < vec2(0)) || any(cell >= output_size) { return vec4(0.); }
 let source_size = vec2<i32>(textureDimensions(image));
 if all(source_size == output_size) { return textureLoad(image,cell,0); }
 let ratio = vec2<f32>(source_size) / vec2<f32>(output_size);
 let low = vec2<f32>(cell)*ratio;
 let high = vec2<f32>(cell+vec2(1))*ratio;
 let start = vec2<i32>(floor(low));
 var color = vec4(0.);
 for (var y=0; y<5; y+=1) {
  for (var x=0; x<5; x+=1) {
   let texel = start+vec2(x,y);
   let overlap = max(vec2(0.), min(high,vec2<f32>(texel+vec2(1))) - max(low,vec2<f32>(texel)));
   if all(texel >= vec2(0)) && all(texel < source_size) && overlap.x*overlap.y > 0. {
    color += textureLoad(image,texel,0)*overlap.x*overlap.y;
   }
  }
 }
 return color/(ratio.x*ratio.y);
}
@fragment fn backdrop_fragment(v: Varying) -> @location(0) vec4<f32> {
 // Separable associated-RGBA Gaussian. The horizontal pass combines area
 // downsampling and filtering; the vertical pass stays at working resolution.
 if p.viewport.z == 5. {
  let radius = p.metrics.z;
  let support = i32(ceil(radius));
  let sigma = max(radius / 3., 0.1);
  let center = vec2<i32>(v.screen.xy);
  let direction = vec2<i32>(p.metrics.xy);
  let dimensions = vec2<i32>(p.viewport.xy);
  var color = vec4(0.); var total = 0.;
  for (var i = -support; i <= support; i += 1) {
   let weight = exp(-0.5 * f32(i * i) / (sigma * sigma));
   let texel = center + direction * i;
   color += layer_cell(texel,dimensions) * weight;
   total += weight;
  }
  return color / total;
 }
 // Clear the isolated crop without blending. Composition later restores the
 // saved background and applies opacity once to the completed group.
 if p.viewport.z == 3. { return vec4(0.); }
 if p.viewport.z == 4. {
  let destination = sample_image(original,v.uv);
  // The main target was used as temporary isolation storage over the halo.
  // Restore saved destination pixels outside the final output clip.
  if any(v.screen.xy < p.clip.xy) || any(v.screen.xy >= p.clip.zw) { return destination; }
  let captured = sample_image(image,v.uv);
  // The matrix is linear on associated RGB. Clamping to [0,alpha] is
  // equivalent to filtering/clamping straight RGB, without dividing tiny alpha.
  // Metal wide captures carry associated extended linear P3 (viewport.w=2).
  // Domain 1 retains the bounded P3 operation for older hosts.
  // Default captures retain their encoded-sRGB filter behavior.
  let luma = select(vec3(0.2126,0.7152,0.0722),
      vec3(0.2289745641,0.6917385218,0.0792869141),p.viewport.w >= 1.);
  let gray = dot(captured.rgb,luma);
  var filtered = vec3(gray) + (captured.rgb - vec3(gray)) * p.metrics.w;
  if p.viewport.w < 2. { filtered = clamp(filtered,vec3(0.),vec3(captured.a)); }
  var brightened = filtered * p.radii.x;
  if p.viewport.w < 2. { brightened = clamp(brightened,vec3(0.),vec3(captured.a)); }
  let pivot = vec3(0.5 * captured.a);
  var contrasted = (brightened - pivot) * p.radii.y + pivot;
  if p.viewport.w < 2. { contrasted = clamp(contrasted,vec3(0.),vec3(captured.a)); }
  // Complement associated RGB around alpha to preserve transparent pixels.
  let inverted = mix(contrasted,vec3(captured.a)-contrasted,p.radii.z);
  let group = vec4(inverted,captured.a) * p.metrics.z;
  return blend_group(group, destination, u32(p.radii.w));
 }
 if p.viewport.z < 2. {
  let step = p.metrics.xy / vec2<f32>(textureDimensions(image));
  var color = vec4(0.); var total = 0.;
  for(var i: i32 = -4; i <= 4; i += 1) {
   let f = f32(i); let weight = exp(-0.5 * f * f * 0.5625);
   color += sample_image(image,v.uv + step * f) * weight;
   total += weight;
  }
  return color / total;
 }
 if any(v.screen.xy < p.clip.xy) || any(v.screen.xy >= p.clip.zw) { discard; }
 let local = v.screen.xy - p.bounds.xy; let half_size = p.bounds.zw * 0.5;
 let centered = local - half_size;
 let left_radius = select(p.radii.w,p.radii.x,centered.y < 0.);
 let right_radius = select(p.radii.z,p.radii.y,centered.y < 0.);
 let radius = select(right_radius,left_radius,centered.x < 0.);
 let q = abs(centered) - half_size + vec2(radius);
 let distance = length(max(q,vec2(0.))) + min(max(q.x,q.y),0.) - radius;
 let coverage = clamp(0.5 - distance,0.,1.) * p.metrics.z;
 let color = sample_image(image,v.uv);
 let previous = sample_image(original,v.uv);
 return mix(previous,color,coverage);
}
