//! Small analytic presets; all colors use straight-alpha SDR.
use super::*;
/// Parameters shared by the rounded-box, glow and particle presets.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct EffectInstance {
    /// Straight-alpha encoded-sRGB tint.
    pub color: [f32; 4],
    /// Radius, phase, intensity and reserved padding.
    pub effect: [f32; 4],
}
impl ShaderInstance for EffectInstance {
    const FIELDS: &'static [InstanceField] = &[
        InstanceField {
            name: "color",
            offset: std::mem::offset_of!(Self, color),
            components: 4,
        },
        InstanceField {
            name: "effect",
            offset: std::mem::offset_of!(Self, effect),
            components: 4,
        },
    ];
}
/// Antialiased rounded rectangle.
pub const ROUNDED_BOX: &str = include_str!("rounded_box.wgsl");
/// Pulsing box glow inside the full effect rectangle (reserve a 12px inset).
pub const PULSE_GLOW: &str = include_str!("pulse_glow.wgsl");
/// Soft radial particle, suitable for thousands of independently placed instances.
pub const PARTICLE: &str = include_str!("particle.wgsl");
