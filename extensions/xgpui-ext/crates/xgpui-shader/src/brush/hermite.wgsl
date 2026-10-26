// PATCH_COEFFICIENTS is a bicubic power basis in row-major (v,u) order.
struct PatchFrame { position: vec2<f32>, du: vec2<f32>, dv: vec2<f32> }
fn patch_cell(uv:vec2<f32>) -> vec2<u32> {
    let count=vec2(f32(PATCH_COLUMNS),f32(PATCH_ROWS));
    return vec2<u32>(clamp(floor(uv*count),vec2(0.),count-vec2(1.)));
}
fn patch_frame(uv: vec2<f32>) -> PatchFrame {
    let count=vec2(f32(PATCH_COLUMNS),f32(PATCH_ROWS));
    let cell=patch_cell(uv);
    let local=uv*count-vec2<f32>(cell);
    let offset=(cell.y*PATCH_COLUMNS+cell.x)*16u;
    var rows: array<vec2<f32>,4>;
    var derivatives: array<vec2<f32>,4>;
    for (var j = 0u; j < 4u; j += 1u) {
        let a = PATCH_COEFFICIENTS[offset+j*4u];
        let b = PATCH_COEFFICIENTS[offset+j*4u+1u];
        let c = PATCH_COEFFICIENTS[offset+j*4u+2u];
        let d = PATCH_COEFFICIENTS[offset+j*4u+3u];
        rows[j] = ((d*local.x+c)*local.x+b)*local.x+a;
        derivatives[j] = (3.*d*local.x+2.*c)*local.x+b;
    }
    return PatchFrame(
        ((rows[3]*local.y+rows[2])*local.y+rows[1])*local.y+rows[0],
        (((derivatives[3]*local.y+derivatives[2])*local.y+derivatives[1])*local.y+derivatives[0])*count.x,
        ((3.*rows[3]*local.y+2.*rows[2])*local.y+rows[1])*count.y);
}
fn mesh_color(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let receiver = input.uv;
    var uv = clamp(receiver,vec2(0.),vec2(1.));
    // The host certifies ||I-J||_infinity <= 0.5 over the entire patch.
    // Projected fixed-point iterations give a close initial inverse estimate.
    for (var i = 0u; i < 6u; i += 1u) {
        uv = clamp(uv+receiver-patch_frame(uv).position,vec2(0.),vec2(1.));
    }
    // Allow a small exterior inverse for antialiased patch edges. Large exterior
    // receivers stay bounded and fail the residual/parameter checks below.
    for (var i = 0u; i < 6u; i += 1u) {
        let frame = patch_frame(uv);
        let delta = frame.position-receiver;
        let determinant = frame.du.x*frame.dv.y-frame.du.y*frame.dv.x;
        if (determinant <= 0.0001) { return vec4(0.); }
        let step = vec2(delta.x*frame.dv.y-delta.y*frame.dv.x,
                        frame.du.x*delta.y-frame.du.y*delta.x)/determinant;
        uv = clamp(uv-step,vec2(-0.25),vec2(1.25));
    }
    let frame = patch_frame(uv);
    if (any(abs(frame.position-receiver) > vec2(0.00001))) { return vec4(0.); }
    let determinant = frame.du.x*frame.dv.y-frame.du.y*frame.dv.x;
    if (determinant <= 0.0001) { return vec4(0.); }
    let device_size = input.size*input.scale_factor;
    let du_pixel = length(vec2(frame.dv.y,-frame.dv.x)/determinant/device_size);
    let dv_pixel = length(vec2(-frame.du.y,frame.du.x)/determinant/device_size);
    let edge = min(min(uv.x,1.-uv.x)/max(du_pixel,0.0000001),
                   min(uv.y,1.-uv.y)/max(dv_pixel,0.0000001));
    let coverage = clamp(0.5+edge,0.,1.);
    let global=clamp(uv,vec2(0.),vec2(1.));
    let cell=patch_cell(global);
    let t=global*vec2(f32(PATCH_COLUMNS),f32(PATCH_ROWS))-vec2<f32>(cell);
    let color_offset=(cell.y*PATCH_COLUMNS+cell.x)*4u;
    let h = t*t*(3.-2.*t);
    let top = mix(associated(PATCH_COLORS[color_offset+0u]),associated(PATCH_COLORS[color_offset+1u]),h.x);
    let bottom = mix(associated(PATCH_COLORS[color_offset+2u]),associated(PATCH_COLORS[color_offset+3u]),h.x);
    let color = straight(mix(top,bottom,h.y));
    return vec4(color.rgb,color.a*coverage);
}
