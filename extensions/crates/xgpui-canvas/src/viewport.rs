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

/// Procedural grid settings. Color is straight RGBA in the shader's color space.
#[derive(Clone, Copy, Debug)]
pub struct GridOptions {
    pub style: GridStyle,
    pub spacing: f64,
    pub major_interval: u32,
    pub color: [f32; 4],
}

impl Default for GridOptions {
    fn default() -> Self {
        Self {
            style: GridStyle::Lines,
            spacing: 25.,
            major_interval: 5,
            color: [0.32, 0.43, 0.58, 1.],
        }
    }
}

impl GridOptions {
    fn validate(self) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.spacing.is_finite() && (0.001..=1e12).contains(&self.spacing),
            "grid spacing must be finite and between 0.001 and 1e12"
        );
        anyhow::ensure!(
            (2..=16).contains(&self.major_interval),
            "grid major interval must be 2..=16"
        );
        anyhow::ensure!(
            self.color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "grid color channels must be finite and 0..=1"
        );
        Ok(self)
    }
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
        self.paint_with_options(
            camera,
            GridOptions {
                style,
                ..Default::default()
            },
            bounds,
            window,
        )
    }

    pub fn paint_with_options(
        &self,
        camera: Camera2D,
        options: GridOptions,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> anyhow::Result<()> {
        let options = options.validate()?;
        let viewport = Point::new(
            f32::from(bounds.size.width) as f64,
            f32::from(bounds.size.height) as f64,
        );
        window.paint_custom_shader(bounds, &self.0, grid_instance(camera, viewport, options))
    }
}

fn grid_instance(camera: Camera2D, viewport: Point, options: GridOptions) -> GridInstance {
    // Successive powers of five keep the grid sparse at every supported zoom.
    // Fade the finer level to zero at a transition so the previous major grid
    // becomes the new minor grid without a hard visibility change.
    let interval = options.major_interval as f64;
    let level = (20. / (options.spacing * camera.zoom())).ln() / interval.ln();
    let world_step = options.spacing * interval.powf(level.floor());
    let step = world_step * camera.zoom();
    let major_step = step * interval;
    let minimum = 20. / interval;
    let origin = camera.screen_to_world(Point::default(), viewport);
    let phase =
        |coordinate: f64, period: f64| (coordinate.rem_euclid(period) * camera.zoom()) as f32;
    GridInstance {
        phase: [
            phase(origin.x, world_step),
            phase(origin.y, world_step),
            phase(origin.x, world_step * interval),
            phase(origin.y, world_step * interval),
        ],
        metrics: [
            step as f32,
            major_step as f32,
            ((step - minimum) / (20. - minimum)).clamp(0., 1.) as f32 * 0.65,
            if matches!(options.style, GridStyle::Dots) {
                1.
            } else {
                0.
            },
        ],
        color: options.color,
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
        Self::with_grid_options(
            camera,
            grid,
            GridOptions {
                style,
                ..Default::default()
            },
        )
    }

    pub fn with_grid_options(camera: Camera2D, grid: &GridPainter, options: GridOptions) -> Self {
        let grid = grid.clone();
        Self {
            base: div().relative().overflow_hidden().child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        if let Err(error) = grid.paint_with_options(camera, options, bounds, window)
                        {
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
    fn configurable_grid_rejects_invalid_values_and_preserves_spacing_ratio() {
        for interval in [2, 3, 5, 16] {
            let options = GridOptions {
                spacing: 73.,
                major_interval: interval,
                color: [0.1, 0.2, 0.3, 0.4],
                ..Default::default()
            };
            options.validate().unwrap();
            let instance = grid_instance(Camera2D::default(), Point::new(500., 300.), options);
            assert!((instance.metrics[1] / instance.metrics[0] - interval as f32).abs() < 1e-5);
            assert_eq!(instance.color, options.color);
        }
        assert!(
            GridOptions {
                spacing: f64::NAN,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            GridOptions {
                major_interval: 1,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            GridOptions {
                color: [f32::INFINITY; 4],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

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
            let instance = grid_instance(camera, viewport, GridOptions::default());
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
