fn glyph_texel(input: ShaderInput, p: Parameters) -> vec2<f32> {
    return p.widths.xy + input.uv * p.widths.zw;
}
fn shade(input: ShaderInput, p: Parameters) -> vec4<f32> {
    let position = input.position + p.radii.xy;
    return brush_color(ShaderInput(position / p.radii.zw, position, p.radii.zw, input.scale_factor), p);
}
