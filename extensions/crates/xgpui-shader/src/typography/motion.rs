//! Explicit visual snapshots: the host owns the animation clock.
use anyhow::{Result, ensure};
use gpui::{Pixels, px};

/// Paint-only effects. Source text, canonical carets and accessibility never change.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum KineticEffect {
    #[default]
    Static,
    /// Reveals complete shaped clusters as progress advances through source graphemes.
    Typewriter { progress: f32 },
    /// Resolve complete source clusters from left to right. Unresolved eligible
    /// clusters use a deterministic, same-font shaped hexadecimal alphabet.
    Scramble { progress: f32, seed: u64, tick: u64 },
    /// All glyphs in a shaped cluster share a vertical offset. Phase is in radians.
    Wave {
        amplitude: Pixels,
        phase: f32,
        cluster_phase: f32,
    },
}
impl KineticEffect {
    pub(super) fn validate(self) -> Result<()> {
        match self {
            Self::Static => {}
            Self::Typewriter { progress } | Self::Scramble { progress, .. } => ensure!(
                progress.is_finite() && (0. ..=1.).contains(&progress),
                "typography progress must be in [0,1]"
            ),
            Self::Wave {
                amplitude,
                phase,
                cluster_phase,
            } => {
                let amplitude = f32::from(amplitude);
                ensure!(
                    amplitude.is_finite() && (0. ..=12.).contains(&amplitude),
                    "wave amplitude must be in [0,12] logical pixels"
                );
                ensure!(
                    phase.is_finite() && cluster_phase.is_finite(),
                    "wave phases must be finite"
                );
            }
        }
        Ok(())
    }
    pub(super) fn padding(self) -> Pixels {
        match self {
            Self::Wave { amplitude, .. } => amplitude,
            _ => px(0.),
        }
    }
    pub(super) fn reveal_byte(self, boundaries: &[usize]) -> usize {
        let count = boundaries.len() - 1;
        match self {
            Self::Typewriter { progress } => {
                boundaries[((progress * count as f32).floor() as usize).min(count)]
            }
            _ => *boundaries.last().unwrap(),
        }
    }
    pub(super) fn resolved_byte(self, boundaries: &[usize]) -> usize {
        match self {
            Self::Scramble { progress, .. } => {
                Self::Typewriter { progress }.reveal_byte(boundaries)
            }
            _ => *boundaries.last().unwrap(),
        }
    }
    pub(super) fn offset(self, ordinal: usize) -> Pixels {
        match self {
            // Reduce before multiplying so even very large finite inputs stay finite.
            Self::Wave {
                amplitude,
                phase,
                cluster_phase,
            } => {
                amplitude
                    * (phase.rem_euclid(std::f32::consts::TAU)
                        + (ordinal as f64 * f64::from(cluster_phase))
                            .rem_euclid(std::f64::consts::TAU) as f32)
                        .sin()
            }
            _ => px(0.),
        }
    }
}

/// Glyph cluster starts snapped to graphemes. Multiple glyphs and ligatures must
/// appear together, including fallback runs sharing the same source cluster.
pub(super) fn clusters(
    boundaries: &[usize],
    indices: impl Iterator<Item = usize>,
) -> Vec<(usize, usize, usize)> {
    let mut starts: Vec<_> = indices
        .map(|byte| {
            let ordinal = boundaries.partition_point(|b| *b <= byte).saturating_sub(1);
            boundaries[ordinal]
        })
        .collect();
    starts.push(0);
    starts.sort_unstable();
    starts.dedup();
    let len = *boundaries.last().unwrap();
    starts
        .iter()
        .enumerate()
        .map(|(i, start)| {
            (
                *start,
                starts.get(i + 1).copied().unwrap_or(len),
                boundaries.binary_search(start).unwrap(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effects_validate_and_extreme_finite_phases_stay_bounded() {
        for progress in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert!(KineticEffect::Typewriter { progress }.validate().is_err());
            assert!(
                KineticEffect::Scramble {
                    progress,
                    seed: 0,
                    tick: 0
                }
                .validate()
                .is_err()
            );
        }
        for amplitude in [-1., 12.1, f32::NAN] {
            assert!(
                KineticEffect::Wave {
                    amplitude: px(amplitude),
                    phase: 0.,
                    cluster_phase: 1.
                }
                .validate()
                .is_err()
            );
        }
        let effect = KineticEffect::Wave {
            amplitude: px(12.),
            phase: f32::MAX,
            cluster_phase: f32::MAX,
        };
        effect.validate().unwrap();
        assert!(f32::from(effect.offset(1000)).abs() <= 12.);
    }
    #[test]
    fn reveal_preserves_graphemes_and_shaped_ligatures() {
        // fi ligature, combining e+accent, and joined emoji. Glyph 4 belongs to é.
        let boundaries = [0, 1, 2, 5, 20];
        let groups = clusters(&boundaries, [0, 2, 4, 5, 5].into_iter());
        assert_eq!(groups, vec![(0, 2, 0), (2, 5, 2), (5, 20, 3)]);
        for (progress, byte, visible) in [
            (0., 0, 0),
            (0.25, 1, 0),
            (0.5, 2, 1),
            (0.75, 5, 2),
            (1., 20, 3),
        ] {
            let cutoff = KineticEffect::Typewriter { progress }.reveal_byte(&boundaries);
            assert_eq!(cutoff, byte);
            assert_eq!(
                groups.iter().filter(|(_, end, _)| *end <= cutoff).count(),
                visible
            );
        }
        assert_eq!(
            KineticEffect::Typewriter { progress: 0. }.reveal_byte(&[0]),
            0
        );
    }
}
