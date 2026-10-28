@group(0) @binding(1) var<uniform> parameters: Parameters;
@vertex fn paint_vertex(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>,6>(vec2(-1.,-1.),vec2(1.,-1.),vec2(-1.,1.),vec2(-1.,1.),vec2(1.,-1.),vec2(1.,1.));
    return vec4(positions[id],0.,1.);
}
@fragment fn paint_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / xgpui_geometry.dimensions.zw;
    return clamp(paint(PaintInput(uv,uv*xgpui_geometry.dimensions.xy,xgpui_geometry.dimensions.xy)),vec4(0.),vec4(1.));
}
