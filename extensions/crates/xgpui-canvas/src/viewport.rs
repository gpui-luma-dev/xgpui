use crate::{Camera2D, Point};
use gpui::{prelude::*, *};
use xgpui_shader::{
    WindowShaderExt,
    instanced::{InstanceField, ShaderInstance, ShaderProgram},
};

#[derive(Clone, Copy, Debug, Default)]
pub enum GridStyle {
    #[default]
    Lines,
    Dots,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridInstance {
    phase: [f32; 4],
    metrics: [f32; 4],
    color: [f32; 4],
}

impl ShaderInstance for GridInstance {
    const FIELDS: &'static [InstanceField] = &[
        InstanceField {
            name: "phase",
            offset: std::mem::offset_of!(Self, phase),
            components: 4,
        },
        InstanceField {
            name: "metrics",
            offset: std::mem::offset_of!(Self, metrics),
            components: 4,
        },
        InstanceField {
            name: "color",
            offset: std::mem::offset_of!(Self, color),
            components: 4,
        },
    ];
}

/// Retain once per view. Clones share the compiled shader and backend pipelines.
#[derive(Clone)]
pub struct GridPainter(ShaderProgram<GridInstance>);

impl GridPainter {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self(ShaderProgram::new(include_str!("grid.wgsl"))?))
    }

    pub fn paint(
        &self,
        camera: Camera2D,
        style: GridStyle,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> anyhow::Result<()> {
        let viewport = Point::new(
            f32::from(bounds.size.width) as f64,
            f32::from(bounds.size.height) as f64,
        );
        window.paint_custom_shader(bounds, &self.0, grid_instance(camera, viewport, style))
    }
}

fn grid_instance(camera: Camera2D, viewport: Point, style: GridStyle) -> GridInstance {
    // Successive powers of five keep the grid sparse at every supported zoom.
    // Fade the finer level to zero at a transition so the previous major grid
    // becomes the new minor grid without a hard visibility change.
    let level = (20. / (25. * camera.zoom())).ln() / 5_f64.ln();
    let world_step = 25. * 5_f64.powf(level.floor());
    let step = world_step * camera.zoom(); // 4..20 logical pixels
    let major_step = step * 5.;
    let origin = camera.screen_to_world(Point::default(), viewport);
    let phase =
        |coordinate: f64, period: f64| (coordinate.rem_euclid(period) * camera.zoom()) as f32;
    GridInstance {
        phase: [
            phase(origin.x, world_step),
            phase(origin.y, world_step),
            phase(origin.x, world_step * 5.),
            phase(origin.y, world_step * 5.),
        ],
        metrics: [
            step as f32,
            major_step as f32,
            ((step - 4.) / 16.).clamp(0., 1.) as f32 * 0.65,
            if matches!(style, GridStyle::Dots) {
                1.
            } else {
                0.
            },
        ],
        color: [0.32, 0.43, 0.58, 1.],
    }
}

/// Clipped spatial viewport with one procedural background quad.
///
/// Children use viewport-local screen coordinates. The caller projects bounds
/// and sizes and culls before constructing children; this does not transform
/// arbitrary GPUI subtrees. Input/state ownership remains with the caller.
#[derive(IntoElement)]
pub struct Canvas {
    base: Div,
}

impl Canvas {
    pub fn new(camera: Camera2D, grid: &GridPainter, style: GridStyle) -> Self {
        let grid = grid.clone();
        Self {
            base: div().relative().overflow_hidden().child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        if let Err(error) = grid.paint(camera, style, bounds, window) {
                            eprintln!("canvas grid: {error}");
                        }
                    },
                )
                .absolute()
                .size_full(),
            ),
        }
    }
}

impl Styled for Canvas {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}
impl ParentElement for Canvas {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}
impl RenderOnce for Canvas {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn shader_compiles_and_validates_parameter_layout() {
        GridPainter::new().unwrap();
    }
    #[::core::prelude::v1::test]
    fn grid_phase_matches_camera_and_is_bounded_at_large_coordinates() {
        let viewport = Point::new(1100., 700.);
        let mut camera = Camera2D::default();
        camera.pan_by(Point::new(1e10 + 13., -1e10 + 7.));
        for factor in [0.01, 100., 64.] {
            camera.zoom_at(Point::new(137., 247.), viewport, factor);
            let instance = grid_instance(camera, viewport, GridStyle::Lines);
            let origin = camera.world_to_screen(Point::default(), viewport);
            for (axis, coordinate) in [origin.x, origin.y].into_iter().enumerate() {
                let step = instance.metrics[1] as f64;
                let remainder = (coordinate + instance.phase[axis + 2] as f64).rem_euclid(step);
                assert!(remainder.min(step - remainder) < 0.001);
                assert!(
                    instance.phase[axis + 2] >= 0.
                        && instance.phase[axis + 2] <= instance.metrics[1]
                );
            }
        }
    }
}
