use super::*;

/// Explicit policies for producing bounded SDR RGB. Neither is an ICC intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamutMapping {
    /// Clip destination linear channels independently to [0,1].
    Clip,
    /// Reduce OkLCh chroma at fixed lightness/hue using a bounded binary search.
    /// Lightness <=0 becomes black; >=1 becomes white. This is not CSS MINDE.
    ReduceChroma,
}
impl LinearRgb {
    /// True only when every channel is in the closed destination RGB cube.
    pub fn is_in_gamut(self) -> bool {
        self.components.iter().all(|v| (0.0..=1.0).contains(v))
    }

    /// Convert first, then apply an explicit bounded-SDR policy.
    /// Already in-gamut destination values are returned unchanged.
    pub fn map_gamut(self, destination: RgbSpace, policy: GamutMapping) -> Result<Self> {
        let rgb = self.convert(destination)?;
        if rgb.is_in_gamut() {
            return Ok(rgb);
        }
        let clip = |color: Self| Self::new(destination, color.components.map(|v| v.clamp(0., 1.)));
        if policy == GamutMapping::Clip {
            return clip(rgb);
        }
        let [l, c, h] = rgb.to_oklab()?.to_oklch()?.components();
        if l <= 0. {
            return Self::new(destination, [0.; 3]);
        }
        if l >= 1. {
            return Self::new(destination, [1.; 3]);
        }
        // Start with a neutral fallback; clipping absorbs rounded OkLab matrix error.
        let mut best = clip(Oklch::new(l, 0., h)?.to_oklab()?.to_linear(destination)?)?;
        let mut low = 0.;
        let mut high = c;
        // Absolute chroma tolerance, at most 64 iterations even for extreme inputs.
        // Search assumes the neutral-connected in-gamut segment at fixed L/h.
        for _ in 0..64 {
            if high - low <= 1e-7 {
                break;
            }
            let mid = low + (high - low) * 0.5;
            let candidate = Oklch::new(l, mid, h)?.to_oklab()?.to_linear(destination)?;
            if candidate.is_in_gamut() {
                low = mid;
                best = candidate;
            } else {
                high = mid;
            }
        }
        Ok(best)
    }
}
