//! Shared WGSL/HLSL uniform layout for the built-in effects.
use gpui::{DevicePixels, Size};
pub(crate) fn gradient(paint: &crate::Gradient, extent: Size<DevicePixels>) -> Vec<[f32; 4]> {
    use crate::GradientKind::*;
    let (kind, center, radius, angle) = match paint.kind() {
        Linear { angle } => (0., [0.5; 2], [0.5; 2], angle),
        Radial { center, radius } => (1., center, radius, 0.),
        Conic { center, angle } => (2., center, [0.5; 2], angle),
        Diamond {
            center,
            radius,
            angle,
        } => (3., center, radius, angle),
    };
    let radians = angle.rem_euclid(360.).to_radians();
    let direction = if kind == 0. {
        [radians.sin(), -radians.cos(), radians, 0.]
    } else {
        [radians.cos(), radians.sin(), radians, 0.]
    };
    let mut params = vec![[0.; 4]; 67];
    params[0] = [
        extent.width.0 as f32,
        extent.height.0 as f32,
        kind,
        paint.stops().len() as f32,
    ];
    params[1] = [center[0], center[1], radius[0], radius[1]];
    params[2] = direction;
    for (i, stop) in paint.stops().iter().enumerate() {
        params[3 + i * 2] = stop.color;
        params[4 + i * 2] = [stop.offset, 0., 0., 0.];
    }
    params
}
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
