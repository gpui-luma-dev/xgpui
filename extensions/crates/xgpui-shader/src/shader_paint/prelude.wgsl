struct PaintInput { uv: vec2<f32>, position: vec2<f32>, size: vec2<f32> }
struct XgpuiGeometry { dimensions: vec4<f32> }
@group(0) @binding(0) var<uniform> xgpui_geometry: XgpuiGeometry;
