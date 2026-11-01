@group(0) @binding(1) var<uniform> parameters: Parameters;
@fragment fn paint_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / xgpui_geometry.dimensions.zw;
    return clamp(paint(PaintInput(uv,uv*xgpui_geometry.dimensions.xy,xgpui_geometry.dimensions.xy)),vec4(0.),vec4(1.));
}
