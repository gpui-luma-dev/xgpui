//! Shared WGSL/HLSL uniform layout for the built-in effects.
pub(crate) fn mesh(paint: &crate::MeshGradient) -> Vec<[f32; 4]> {
    let mut params = vec![[0.; 4]; 769];
    params[0] = [
        paint.grid().width() as f32,
        paint.grid().height() as f32,
        paint.subdivisions() as f32,
        0.,
    ];
    for (i, p) in paint.grid().points().iter().enumerate() {
        params[1 + i * 3] = [p.position[0], p.position[1], p.u_tangent[0], p.u_tangent[1]];
        params[2 + i * 3] = [p.v_tangent[0], p.v_tangent[1], 0., 0.];
        params[3 + i * 3] = [p.color[0], p.color[1], p.color[2], 1.];
    }
    params
}
