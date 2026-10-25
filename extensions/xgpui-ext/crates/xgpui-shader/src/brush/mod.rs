//! Main-pass SDR brushes. Coordinates cover the full, unclipped receiver bounds.
//! Colors interpolate in premultiplied encoded sRGB; the host associates alpha
//! after analytic coverage and inherited opacity. No intermediate color or
//! geometry-mask texture is allocated for analytic fills; text reuses the glyph
//! atlas, while paths reuse the existing native path coverage target/pass.
use crate::instanced::{InstanceField, ShaderInstance, ShaderProgram, WindowShaderExt};
use anyhow::{Result, ensure};
use gpui::{BorderStyle, Bounds, Corners, PaintQuad, Pixels, Window};
use std::sync::OnceLock;

/// Maximum stops fitting alongside shape coverage in the portable 128-byte ABI.
pub const MAX_BRUSH_STOPS: usize = 3;
mod cache;
use cache::ProgramCache;
mod hermite;
pub use hermite::BrushMeshPoint;
mod noise;
mod path;
mod spatial;
pub use spatial::{BrushPoint, MAX_FREEFORM_POINTS};
pub(crate) mod text;
pub use noise::{BlendMode, Noise, NoiseKind};

const BASE_BRUSH_COLOR: &str = "\nfn brush_color(input: ShaderInput, p: Parameters) -> vec4<f32> { return base_color(input,p); }";

fn base_program() -> Result<ShaderProgram<BrushInstance>> {
    static PROGRAM: OnceLock<std::result::Result<ShaderProgram<BrushInstance>, String>> =
        OnceLock::new();
    PROGRAM
        .get_or_init(|| {
            let source = format!("{}{BASE_BRUSH_COLOR}", include_str!("brush.wgsl"));
            ShaderProgram::new(&source).map_err(|error| format!("{error:#}"))
        })
        .as_ref()
        .map(Clone::clone)
        .map_err(|error| anyhow::anyhow!("{error}"))
}

/// Normalized stop position and straight-alpha encoded-sRGB color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushStop {
    pub offset: f32,
    pub color: [f32; 4],
}
impl BrushStop {
    pub fn new(offset: f32, color: [f32; 4]) -> Self {
        Self { offset, color }
    }
}

/// Immutable main-pass brush. Inline materials share one base program, allowing
/// different colors and gradient kinds in a single batch.
/// Gradients accept 2..=3 sorted stops; duplicate offsets form hard edges, with
/// the last stop winning at the exact offset. Endpoint colors clamp outside stops.
/// Freeform fields support up to 16 points; mesh fields support regular color
/// corners or a certified Hermite grid with up to four patches. Up to three freeform points share the common program;
/// larger immutable point sets specialize a program without reducing precision.
#[derive(Clone)]
pub struct Brush {
    program: ShaderProgram<BrushInstance>,
    unmodulated: ShaderProgram<BrushInstance>,
    identity: std::sync::Arc<()>,
    parameters: BrushInstance,
}
impl std::fmt::Debug for Brush {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Brush")
            .field("parameters", &self.parameters)
            .finish()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BrushInstance {
    first: [f32; 4],
    second: [f32; 4],
    third: [f32; 4],
    // Analytic stops/kind; spatial brushes reuse this slot for points/fourth color.
    stops: [f32; 4],
    // Linear unit direction or normalized radial center/radii.
    gradient: [f32; 4],
    // Clockwise: top-left, top-right, bottom-right, bottom-left.
    radii: [f32; 4],
    // Clockwise: top, right, bottom, left.
    widths: [f32; 4],
    // Stop count or spatial tag (-1..=-3 inline freeform, -4 mesh, -5 specialized freeform), border flag, noise frequency, noise strength.
    options: [f32; 4],
}
impl ShaderInstance for BrushInstance {
    const FIELDS: &'static [InstanceField] = &[
        InstanceField {
            name: "first",
            offset: 0,
            components: 4,
        },
        InstanceField {
            name: "second",
            offset: 16,
            components: 4,
        },
        InstanceField {
            name: "third",
            offset: 32,
            components: 4,
        },
        InstanceField {
            name: "stops",
            offset: 48,
            components: 4,
        },
        InstanceField {
            name: "gradient",
            offset: 64,
            components: 4,
        },
        InstanceField {
            name: "radii",
            offset: 80,
            components: 4,
        },
        InstanceField {
            name: "widths",
            offset: 96,
            components: 4,
        },
        InstanceField {
            name: "options",
            offset: 112,
            components: 4,
        },
    ];
}
impl Brush {
    fn new(stops: &[BrushStop], kind: f32, gradient: [f32; 4]) -> Result<Self> {
        ensure!(
            (1..=MAX_BRUSH_STOPS).contains(&stops.len()),
            "brush stop count exceeds the portable ABI"
        );
        for stop in stops {
            ensure!(
                stop.offset.is_finite() && (0. ..=1.).contains(&stop.offset),
                "brush offsets must be in [0,1]"
            );
            ensure!(
                stop.color
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "brush colors must be finite encoded-sRGB RGBA in [0,1]"
            );
        }
        ensure!(
            stops.windows(2).all(|s| s[0].offset <= s[1].offset),
            "brush stops must be sorted"
        );
        let program = base_program()?;
        let last = *stops.last().unwrap();
        let second = stops.get(1).copied().unwrap_or(last);
        Ok(Self {
            unmodulated: program.clone(),
            program,
            identity: std::sync::Arc::new(()),
            parameters: BrushInstance {
                first: stops[0].color,
                second: second.color,
                third: last.color,
                stops: [stops[0].offset, second.offset, last.offset, kind],
                gradient,
                radii: [0.; 4],
                widths: [0.; 4],
                options: [stops.len() as f32, 0., 0., 0.],
            },
        })
    }
    pub fn solid(color: [f32; 4]) -> Result<Self> {
        Self::new(&[BrushStop::new(0., color)], 0., [0.; 4])
    }
    /// Degrees clockwise from up. Direction uses logical pixel distances, so an
    /// angle stays unchanged on non-square receivers. Zero and one reach the
    /// extrema of the receiver's projection along that direction.
    pub fn linear(angle: f32, stops: Vec<BrushStop>) -> Result<Self> {
        ensure!(angle.is_finite(), "brush angle must be finite");
        ensure!(
            (2..=MAX_BRUSH_STOPS).contains(&stops.len()),
            "gradients require 2..=3 stops"
        );
        let angle = angle.rem_euclid(360.).to_radians();
        Self::new(&stops, 1., [angle.sin(), -angle.cos(), 0., 0.])
    }
    /// Center and elliptical radii are fractions of the full receiver bounds.
    /// Radius 0.5 reaches the edge midpoint for a centered brush.
    pub fn radial(center: [f32; 2], radius: [f32; 2], stops: Vec<BrushStop>) -> Result<Self> {
        ensure!(
            center
                .iter()
                .all(|v| v.is_finite() && (-16. ..=16.).contains(v)),
            "brush center must be in [-16,16]"
        );
        ensure!(
            radius
                .iter()
                .all(|v| v.is_finite() && (0.0001..=16.).contains(v)),
            "brush radii must be in [0.0001,16]"
        );
        ensure!(
            (2..=MAX_BRUSH_STOPS).contains(&stops.len()),
            "gradients require 2..=3 stops"
        );
        Self::new(&stops, 2., [center[0], center[1], radius[0], radius[1]])
    }
    fn instance(&self, bounds: Bounds<Pixels>, radii: Corners<Pixels>) -> Result<BrushInstance> {
        ensure!(
            [
                f32::from(bounds.origin.x),
                bounds.origin.y.into(),
                bounds.size.width.into(),
                bounds.size.height.into()
            ]
            .iter()
            .all(|v| v.is_finite()),
            "brush bounds must be finite"
        );
        let mut instance = self.parameters;
        instance.radii = [
            radii.top_left.into(),
            radii.top_right.into(),
            radii.bottom_right.into(),
            radii.bottom_left.into(),
        ];
        ensure!(
            instance.radii.iter().all(|v| v.is_finite() && *v >= 0.),
            "brush corner radii must be finite and nonnegative"
        );
        let radii = radii.clamp_radii_for_quad_size(bounds.size);
        instance.radii = [
            radii.top_left.into(),
            radii.top_right.into(),
            radii.bottom_right.into(),
            radii.bottom_left.into(),
        ];
        Ok(instance)
    }
    fn border_instance(&self, quad: &PaintQuad) -> Result<BrushInstance> {
        ensure!(
            quad.border_style == BorderStyle::Solid,
            "main-pass brushes require solid borders; use border_paint for dashed borders"
        );
        let mut instance = self.instance(quad.bounds, quad.corner_radii)?;
        instance.widths = [
            quad.border_widths.top.into(),
            quad.border_widths.right.into(),
            quad.border_widths.bottom.into(),
            quad.border_widths.left.into(),
        ];
        ensure!(
            instance.widths.iter().all(|v| v.is_finite() && *v >= 0.),
            "brush border widths must be finite and nonnegative"
        );
        instance.options[1] = 1.;
        Ok(instance)
    }
    /// Record a filled rounded rectangle in the host's current pass and order.
    pub fn paint_background(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radii: Corners<Pixels>,
    ) -> Result<()> {
        if bounds.is_empty() {
            return Ok(());
        }
        window.paint_custom_shader(bounds, &self.program, self.instance(bounds, radii)?)
    }
    /// Record a solid-style border ring. Unequal widths and corner radii are
    /// supported; gradient coordinates still cover the full outer rectangle.
    pub fn paint_border(&self, window: &mut Window, quad: &PaintQuad) -> Result<()> {
        if quad.bounds.is_empty() {
            return Ok(());
        }
        let instance = self.border_instance(quad)?;
        if instance.widths == [0.; 4] {
            return Ok(());
        }
        window.paint_custom_shader(quad.bounds, &self.program, instance)
    }
}

#[cfg(test)]
mod tests;
