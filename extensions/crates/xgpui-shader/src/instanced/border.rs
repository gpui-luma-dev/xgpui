//! Rotating conic highlights on an analytic rounded border. Animation is host driven.
use super::*;
use gpui::px;

/// Reusable compiled border program. Retain in view state, clone cheaply.
#[derive(Clone)]
pub struct AnimatedBorder {
    program: ShaderProgram<BorderInstance>,
}
/// Appearance of a rotating conic border; colors are straight-alpha sRGB.
#[derive(Clone, Debug)]
pub struct AnimatedBorderStyle {
    /// Width inside the receiver, in logical pixels.
    pub width: Pixels,
    /// Soft highlight halo radius in logical pixels; zero gives a crisp ring.
    pub softness: Pixels,
    /// Base ring color.
    pub track: [f32; 4],
    /// Two opposite highlight colors.
    pub highlights: [[f32; 4]; 2],
    /// Width of each fading highlight, as a fraction of one revolution (greater than zero, at most 0.5).
    pub trail: f32,
}
impl Default for AnimatedBorderStyle {
    fn default() -> Self {
        Self {
            width: px(2.),
            softness: px(3.),
            track: [0.88, 0.89, 0.91, 1.],
            highlights: [[0.16, 0.45, 1., 1.], [1., 0.5, 0.16, 1.]],
            trail: 0.15,
        }
    }
}
/// Validated border frame. Supply phase in turns; advance it from your animation clock.
#[derive(Clone)]
pub struct AnimatedBorderLayer {
    program: AnimatedBorder,
    style: AnimatedBorderStyle,
    phase: f32,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BorderInstance {
    shape: [f32; 4],
    metrics: [f32; 4],
    track: [f32; 4],
    first: [f32; 4],
    second: [f32; 4],
}
impl ShaderInstance for BorderInstance {
    const FIELDS: &'static [InstanceField] = &[
        InstanceField {
            name: "shape",
            offset: 0,
            components: 4,
        },
        InstanceField {
            name: "metrics",
            offset: 16,
            components: 4,
        },
        InstanceField {
            name: "track",
            offset: 32,
            components: 4,
        },
        InstanceField {
            name: "first",
            offset: 48,
            components: 4,
        },
        InstanceField {
            name: "second",
            offset: 64,
            components: 4,
        },
    ];
}
impl AnimatedBorder {
    /// Compile once; requires the `portable-shaders` feature.
    pub fn new() -> Result<Self> {
        Ok(Self {
            program: ShaderProgram::new(include_str!("border.wgsl"))?,
        })
    }
    /// Build a frame. Negative phase runs backward; any finite phase wraps in turns.
    pub fn layer(&self, style: AnimatedBorderStyle, phase: f32) -> Result<AnimatedBorderLayer> {
        ensure!(
            f32::from(style.softness).is_finite() && style.softness >= px(0.),
            "invalid border softness"
        );
        ensure!(phase.is_finite(), "border phase must be finite");
        ensure!(
            f32::from(style.width).is_finite() && style.width >= px(0.),
            "invalid border width"
        );
        ensure!(
            style.trail.is_finite() && style.trail > 0. && style.trail <= 0.5,
            "border trail must be in (0, 0.5]"
        );
        ensure!(
            style
                .track
                .iter()
                .chain(style.highlights.iter().flatten())
                .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "invalid border color"
        );
        Ok(AnimatedBorderLayer {
            program: self.clone(),
            style,
            phase: phase.rem_euclid(1.),
        })
    }
}
impl AnimatedBorderLayer {
    pub(crate) fn paint(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radius: Pixels,
    ) -> Result<()> {
        let padding = self.style.softness * 3.;
        let effect_bounds = Bounds::new(
            bounds.origin - gpui::point(padding, padding),
            bounds.size + gpui::size(padding * 2., padding * 2.),
        );
        window.paint_custom_shader(
            effect_bounds,
            &self.program.program,
            BorderInstance {
                shape: [
                    bounds.size.width.into(),
                    bounds.size.height.into(),
                    padding.into(),
                    self.style.softness.into(),
                ],
                metrics: [
                    radius.into(),
                    self.style.width.into(),
                    self.phase,
                    self.style.trail,
                ],
                track: self.style.track,
                first: self.style.highlights[0],
                second: self.style.highlights[1],
            },
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_and_wraps_border_frames() {
        let border = AnimatedBorder::new().unwrap();
        assert_eq!(border.layer(Default::default(), -0.25).unwrap().phase, 0.75);
        assert!(border.layer(Default::default(), f32::NAN).is_err());
        assert!(
            border
                .layer(
                    AnimatedBorderStyle {
                        trail: 0.,
                        ..Default::default()
                    },
                    0.
                )
                .is_err()
        );
        assert!(
            border
                .layer(
                    AnimatedBorderStyle {
                        width: px(-1.),
                        ..Default::default()
                    },
                    0.
                )
                .is_err()
        );
        assert!(
            border
                .layer(
                    AnimatedBorderStyle {
                        track: [2.; 4],
                        ..Default::default()
                    },
                    0.
                )
                .is_err()
        );
    }
}
