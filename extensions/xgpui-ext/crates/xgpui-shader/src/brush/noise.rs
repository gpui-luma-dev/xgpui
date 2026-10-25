//! Bounded procedural modulation of a brush, without framebuffer sampling.
use super::*;
use std::sync::Mutex;

/// Procedural signal, including a deterministic blue-noise rank tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoiseKind {
    Simplex,
    /// Nearest feature distance (F1) in a bounded 5x5 cell neighborhood.
    Worley,
    /// Second-nearest feature distance (F2), sharing the same feature points.
    WorleyF2,
    /// Toroidal 16x16 blue-noise tile at physical pixel resolution; one octave.
    Dither,
}
/// Blend the procedural grayscale signal with the brush's own RGB. These modes
/// never read or blend against the destination framebuffer; brush alpha is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    ColorDodge,
    SoftLight,
    LinearBurn,
}
/// Noise frequency in cycles per logical pixel, octaves and modulation strength.
/// Dither uses physical pixels and requires frequency=1 and octaves=1.
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    pub kind: NoiseKind,
    pub frequency: f32,
    pub octaves: u8,
    pub strength: f32,
}
pub(super) const BLUE_NOISE: &[u8; 256] = include_bytes!("blue_noise.bin");

impl Noise {
    pub fn simplex(frequency: f32, octaves: u8, strength: f32) -> Self {
        Self {
            kind: NoiseKind::Simplex,
            frequency,
            octaves,
            strength,
        }
    }
    pub fn worley(frequency: f32, strength: f32) -> Self {
        Self {
            kind: NoiseKind::Worley,
            frequency,
            octaves: 1,
            strength,
        }
    }
    /// Second-nearest cellular feature distance at the same scale as `worley`.
    /// One feature per cell, a 5x5 neighborhood, and no texture allocations.
    pub fn worley_f2(frequency: f32, strength: f32) -> Self {
        Self {
            kind: NoiseKind::WorleyF2,
            frequency,
            octaves: 1,
            strength,
        }
    }
    /// Blue-noise grain repeating every 16 physical pixels.
    /// Strength=0.02 is a useful starting point; alpha is unchanged.
    pub fn dither(strength: f32) -> Self {
        Self {
            kind: NoiseKind::Dither,
            frequency: 1.,
            octaves: 1,
            strength,
        }
    }
    fn validate(self) -> Result<()> {
        ensure!(
            self.frequency.is_finite() && (0.0001..=4.).contains(&self.frequency),
            "noise frequency must be in [0.0001,4]"
        );
        ensure!(
            (1..=4).contains(&self.octaves),
            "noise requires 1..=4 octaves"
        );
        ensure!(
            self.strength.is_finite() && (0. ..=1.).contains(&self.strength),
            "noise strength must be in [0,1]"
        );
        if self.kind == NoiseKind::Dither {
            ensure!(
                self.frequency == 1. && self.octaves == 1,
                "dither requires frequency=1 and one octave"
            );
        }
        Ok(())
    }
}
impl Brush {
    /// Replace any previous noise modulation. Zero strength restores the original
    /// unmodulated program. Numeric frequency/strength edits share a program;
    /// signal kind, octave count and blend mode select a bounded program variant.
    pub fn with_noise(mut self, noise: Noise, blend: BlendMode) -> Result<Self> {
        noise.validate()?;
        self.identity = std::sync::Arc::new(());
        if noise.strength == 0. {
            self.program = self.unmodulated.clone();
            self.parameters.options[2] = 0.;
            self.parameters.options[3] = 0.;
            return Ok(self);
        }
        type Key = (u64, NoiseKind, u8, BlendMode);
        type Cache = ProgramCache<Key>;
        static PROGRAMS: OnceLock<Mutex<Cache>> = OnceLock::new();
        let mut programs = PROGRAMS.get_or_init(Default::default).lock().unwrap();
        let key = (self.unmodulated.id(), noise.kind, noise.octaves, blend);
        let program = if let Some(program) = programs.get(&key) {
            program.clone()
        } else {
            let kind = match noise.kind {
                NoiseKind::Simplex => 0,
                NoiseKind::Worley => 1,
                NoiseKind::Dither => 2,
                NoiseKind::WorleyF2 => 3,
            };
            let mode = match blend {
                BlendMode::Normal => 0,
                BlendMode::Multiply => 1,
                BlendMode::Screen => 2,
                BlendMode::Overlay => 3,
                BlendMode::ColorDodge => 4,
                BlendMode::SoftLight => 5,
                BlendMode::LinearBurn => 6,
            };
            let ranks = BLUE_NOISE
                .iter()
                .map(|rank| format!("{rank}u"))
                .collect::<Vec<_>>()
                .join(",");
            let source = format!(
                "{}\nconst BLUE_NOISE: array<u32,256> = array<u32,256>({ranks});\nconst NOISE_KIND: u32 = {kind}u;\nconst NOISE_OCTAVES: u32 = {}u;\nconst BLEND_MODE: u32 = {mode}u;\n{}",
                self.unmodulated
                    .source
                    .strip_suffix(BASE_BRUSH_COLOR)
                    .expect("unmodulated brush source"),
                noise.octaves,
                include_str!("noise.wgsl")
            );
            let program = ShaderProgram::new(&source)?;
            programs.insert(key, program.clone());
            program
        };
        self.program = program;
        self.parameters.options[2] = noise.frequency;
        self.parameters.options[3] = noise.strength;
        Ok(self)
    }
}
