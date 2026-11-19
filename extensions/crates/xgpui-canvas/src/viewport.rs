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

    /// Returns an error for invalid options or unrepresentable world grid periods.
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
        window.paint_custom_shader(bounds, &self.0, grid_instance(camera, viewport, options)?)
    }
}

fn grid_instance(
    camera: Camera2D,
    viewport: Point,
    options: GridOptions,
) -> anyhow::Result<GridInstance> {
    anyhow::ensure!(
        [camera.center.x, camera.center.y, viewport.x, viewport.y]
            .iter()
            .all(|v| v.is_finite()),
        "grid camera and viewport must be finite"
    );
    let interval = options.major_interval as f64;
    // Subtract logarithms: spacing * zoom and its reciprocal can overflow/underflow
    // even when the resulting world and screen periods are representable.
    let exponent =
        ((20_f64.ln() - options.spacing.ln() - camera.zoom().ln()) / interval.ln()).floor();
    let mut world_step = options.spacing * interval.powf(exponent);
    if !world_step.is_finite() || world_step == 0. {
        // The power alone may overflow/underflow before multiplication by spacing.
        world_step = (options.spacing.ln() + exponent * interval.ln()).exp();
    }
    let major_world_step = world_step * interval;
    let step = world_step * camera.zoom();
    let major_step = major_world_step * camera.zoom();
    anyhow::ensure!(
        [world_step, major_world_step, step, major_step]
            .iter()
            .all(|v| v.is_finite() && *v > 0.)
            && step >= f32::MIN_POSITIVE as f64
            && major_step <= f32::MAX as f64,
        "grid spacing is not representable at this zoom"
    );
    let minimum = 20. / interval;
    // Reduce the world center before adding the viewport offset. Forming their
    // world-space sum first loses entire grid periods far from the origin.
    let phase = |center: f64, offset: f64, period: f64, screen_period: f64| {
        ((center.rem_euclid(period) * camera.zoom() - (offset / 2.).rem_euclid(screen_period))
            .rem_euclid(screen_period)) as f32
    };
    Ok(GridInstance {
        phase: [
            phase(camera.center.x, viewport.x, world_step, step),
            phase(camera.center.y, viewport.y, world_step, step),
            phase(camera.center.x, viewport.x, major_world_step, major_step),
            phase(camera.center.y, viewport.y, major_world_step, major_step),
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
    })
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
    fn grid_is_finite_at_extreme_representable_zoom() {
        for spacing in [0.001, 25., 1e12] {
            for zoom in [1e-300, 1e300, 1e308] {
                let mut camera = Camera2D::default();
                camera.zoom_at_with_limits(Point::default(), Point::default(), zoom, zoom, zoom);
                camera.center_on(Point::new(1e10, -1e10));
                let instance = grid_instance(
                    camera,
                    Point::new(1100., 700.),
                    GridOptions {
                        spacing,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert!(
                    instance
                        .phase
                        .iter()
                        .chain(instance.metrics.iter())
                        .all(|v| v.is_finite())
                );
                assert!(instance.metrics[0] > 0. && instance.metrics[1] > instance.metrics[0]);
            }
        }
    }

    #[::core::prelude::v1::test]
    fn distant_deep_zoom_preserves_subpixel_pan_phase() {
        let zoom = 1e8;
        let mut camera = Camera2D::default();
        camera.zoom_at_with_limits(Point::default(), Point::default(), zoom, zoom, zoom);
        camera.center_on(Point::new(1e10, 1e10));
        let a = grid_instance(camera, Point::new(1100., 700.), GridOptions::default()).unwrap();
        let b = grid_instance(camera, Point::new(1101., 701.), GridOptions::default()).unwrap();
        for axis in 0..4 {
            let step = a.metrics[if axis < 2 { 0 } else { 1 }];
            let error = (b.phase[axis] - a.phase[axis] + 0.5).rem_euclid(step);
            assert!(
                error.min(step - error) < 1e-4,
                "phase lost half-pixel offset: {error}"
            );
        }
    }

    #[::core::prelude::v1::test]
    fn unrepresentable_world_period_returns_error() {
        let mut camera = Camera2D::default();
        camera.zoom_at_with_limits(Point::default(), Point::default(), 1e-310, 1e-310, 1.);
        assert!(grid_instance(camera, Point::new(1100., 700.), GridOptions::default()).is_err());
        assert!(
            grid_instance(
                Camera2D::default(),
                Point::new(f64::NAN, 700.),
                GridOptions::default()
            )
            .is_err()
        );
    }

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
            let instance =
                grid_instance(Camera2D::default(), Point::new(500., 300.), options).unwrap();
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
            let instance = grid_instance(camera, viewport, GridOptions::default()).unwrap();
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
