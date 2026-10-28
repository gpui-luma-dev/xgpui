//! Opt-in direct main-pass wide SDR fills. Retain a painter or clone it to share batches.
use crate::{WideColor, color::*};
use anyhow::{Result, ensure};
use gpui::{Bounds, Pixels, Window};
use std::sync::OnceLock;
use xgpui_shader::instanced::{InstanceField, ShaderInstance, ShaderProgram, WindowShaderExt};

#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Parameters {
    first: [f32; 4],
    second: [f32; 4],
    axis: [f32; 4],
    options: [f32; 4],
    shape: [f32; 4],
}
impl ShaderInstance for Parameters {
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
            name: "axis",
            offset: 32,
            components: 4,
        },
        InstanceField {
            name: "options",
            offset: 48,
            components: 4,
        },
        InstanceField {
            name: "shape",
            offset: 64,
            components: 4,
        },
    ];
}
/// Immutable two-stop linear gradient. Coordinates are normalized to fill bounds.
/// Stops remain extended linear P3 until interpolation and explicit output mapping.
#[derive(Clone, Debug)]
pub struct WideGradient {
    parameters: Parameters,
}
impl WideGradient {
    /// Construct a two-stop gradient with explicit interpolation and mapping.
    /// Coincident endpoints select the second stop; outside positions extend stops.
    /// Upload channels and projection coefficients must fit finite f32 with magnitude
    /// <=65504, keeping GPU intermediate arithmetic within the supported SDR range.
    pub fn linear(
        start: [f64; 2],
        end: [f64; 2],
        colors: [LinearRgba; 2],
        interpolation: ColorInterpolation,
        mapping: GamutMapping,
    ) -> Result<Self> {
        ensure!(
            start.into_iter().chain(end).all(f64::is_finite),
            "gradient geometry must be finite"
        );
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let axis = if start == end {
            [0., 0., 1., 0.]
        } else {
            let length = dx.hypot(dy);
            ensure!(
                length.is_finite() && length > 0.,
                "gradient axis cannot be represented"
            );
            let x = (dx / length) / length;
            let y = (dy / length) / length;
            [x, y, -start[0] * x - start[1] * y, 0.]
        };
        let upload = |color: LinearRgba| -> Result<[f32; 4]> {
            let [r, g, b] = color.rgb().convert(RgbSpace::DisplayP3)?.components();
            checked([r, g, b, color.alpha()])
        };
        let (model, hue) = match interpolation {
            ColorInterpolation::LinearRgb => (0., 0.),
            ColorInterpolation::Oklab => (1., 0.),
            ColorInterpolation::Oklch(hue) => (
                2.,
                match hue {
                    HueInterpolation::Shorter => 0.,
                    HueInterpolation::Longer => 1.,
                    HueInterpolation::Increasing => 2.,
                    HueInterpolation::Decreasing => 3.,
                },
            ),
        };
        Ok(Self {
            parameters: Parameters {
                first: upload(colors[0])?,
                second: upload(colors[1])?,
                axis: checked(axis)?,
                options: [
                    model,
                    hue,
                    if mapping == GamutMapping::Clip {
                        0.
                    } else {
                        1.
                    },
                    0.,
                ],
                shape: [0.; 4],
            },
        })
    }
}
fn checked(values: [f64; 4]) -> Result<[f32; 4]> {
    ensure!(
        values.iter().all(|v| v.is_finite() && v.abs() <= 65504.),
        "gradient upload exceeds finite SDR range (65504)"
    );
    Ok(values.map(|v| v as f32))
}
/// Shared compiled solid/gradient program. All painter instances share program
/// identity; consecutive compatible fills can batch without producer textures.
#[derive(Clone)]
pub struct WidePainter {
    program: ShaderProgram<Parameters>,
}
impl WidePainter {
    pub fn new() -> Result<Self> {
        static PROGRAM: OnceLock<std::result::Result<ShaderProgram<Parameters>, String>> =
            OnceLock::new();
        let program = PROGRAM
            .get_or_init(|| {
                ShaderProgram::new_linear_p3(&format!(
                    "{WGSL_COLOR_MATH}\n{}",
                    include_str!("instanced.wgsl")
                ))
                .map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(|e| anyhow::anyhow!("wide fill program: {e}"))?
            .clone();
        Ok(Self { program })
    }
    /// Paint a bounded P3 solid with a uniform logical-pixel corner radius.
    /// Host clipping and inherited opacity are applied by the shader SPI.
    pub fn paint_color(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radius: Pixels,
        color: &WideColor,
    ) -> Result<()> {
        let parameters = Parameters {
            first: color.rgba,
            second: color.rgba,
            axis: [0.; 4],
            options: [0.; 4],
            shape: [0.; 4],
        };
        self.paint(window, bounds, radius, parameters)
    }
    /// Paint a two-stop gradient. Mapping precedes host output association.
    pub fn paint_gradient(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radius: Pixels,
        gradient: &WideGradient,
    ) -> Result<()> {
        self.paint(window, bounds, radius, gradient.parameters)
    }
    fn paint(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radius: Pixels,
        mut parameters: Parameters,
    ) -> Result<()> {
        let [x, y, w, h] = [
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        ];
        let radius = f32::from(radius);
        ensure!(
            [x, y, w, h, radius].iter().all(|v| v.is_finite())
                && w >= 0.
                && h >= 0.
                && radius >= 0.,
            "invalid wide fill bounds or radius"
        );
        if w == 0. || h == 0. {
            return Ok(());
        }
        parameters.shape[0] = radius.min(w.min(h) * 0.5);
        window.paint_custom_shader(bounds, &self.program, parameters)
    }
}

#[cfg(test)]
#[path = "instanced_tests.rs"]
mod tests;
