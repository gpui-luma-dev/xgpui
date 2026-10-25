use super::*;

/// Straight-alpha CPU color; RGB may be extended, alpha must be in [0,1].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRgba {
    rgb: LinearRgb,
    alpha: f64,
}
impl LinearRgba {
    pub fn new(rgb: LinearRgb, alpha: f64) -> Result<Self> {
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Err(ColorError);
        }
        Ok(Self { rgb, alpha })
    }
    pub fn rgb(self) -> LinearRgb {
        self.rgb
    }
    pub fn alpha(self) -> f64 {
        self.alpha
    }
    /// Output association happens after interpolation and optional gamut mapping.
    pub fn premultiplied(self) -> [f64; 4] {
        let [r, g, b] = self.rgb.components();
        [r * self.alpha, g * self.alpha, b * self.alpha, self.alpha]
    }
    pub fn map_gamut(self, destination: RgbSpace, policy: GamutMapping) -> Result<Self> {
        Self::new(self.rgb.map_gamut(destination, policy)?, self.alpha)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HueInterpolation {
    Shorter,
    Longer,
    Increasing,
    Decreasing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorInterpolation {
    /// Linear RGB in the gradient's output primaries.
    LinearRgb,
    Oklab,
    Oklch(HueInterpolation),
}
/// Two-stop gradient reference. Coordinates are in caller-defined common units.
/// Stops are fixed at 0 and 1; values beyond them extend the endpoint colors.
/// Coincident endpoints select the second stop. No gamut mapping is implicit.
#[derive(Clone, Copy, Debug)]
pub struct LinearGradient {
    start: [f64; 2],
    end: [f64; 2],
    colors: [LinearRgba; 2],
    output: RgbSpace,
    interpolation: ColorInterpolation,
}
impl LinearGradient {
    pub fn new(
        start: [f64; 2],
        end: [f64; 2],
        colors: [LinearRgba; 2],
        output: RgbSpace,
        interpolation: ColorInterpolation,
    ) -> Result<Self> {
        if !start.into_iter().chain(end).all(f64::is_finite) {
            return Err(ColorError);
        }
        Ok(Self {
            start,
            end,
            colors,
            output,
            interpolation,
        })
    }
    /// Project a point onto the gradient axis. Non-finite arithmetic is rejected.
    pub fn sample(self, position: [f64; 2]) -> Result<LinearRgba> {
        if !position.into_iter().all(f64::is_finite) {
            return Err(ColorError);
        }
        if self.start == self.end {
            return self.sample_at(1.);
        }
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        let length = dx.hypot(dy);
        if !length.is_finite() || length == 0. {
            return Err(ColorError);
        }
        let t = ((position[0] - self.start[0]) * (dx / length)
            + (position[1] - self.start[1]) * (dy / length))
            / length;
        self.sample_at(t)
    }
    /// Sample normalized axis position, clamped to [0,1].
    pub fn sample_at(self, t: f64) -> Result<LinearRgba> {
        if !t.is_finite() {
            return Err(ColorError);
        }
        let t = t.clamp(0., 1.);
        let [a, b] = self.colors;
        let alpha = lerp(a.alpha, b.alpha, t);
        if alpha == 0. {
            return LinearRgba::new(LinearRgb::new(self.output, [0.; 3])?, 0.);
        }
        // Preserve exact endpoint RGB and avoid needless perceptual round trips.
        if t == 0. || t == 1. {
            let endpoint = if t == 0. { a } else { b };
            return LinearRgba::new(endpoint.rgb.convert(self.output)?, endpoint.alpha);
        }
        let coords = |color: LinearRgba| -> Result<[f64; 3]> {
            Ok(match self.interpolation {
                ColorInterpolation::LinearRgb => color.rgb.convert(self.output)?.components(),
                ColorInterpolation::Oklab => color.rgb.to_oklab()?.components(),
                ColorInterpolation::Oklch(_) => color.rgb.to_oklab()?.to_oklch()?.components(),
            })
        };
        let mut x = coords(a)?;
        let mut y = coords(b)?;
        if let ColorInterpolation::Oklch(policy) = self.interpolation {
            // Neutral and transparent endpoints have no meaningful hue here.
            // Threshold accommodates rounded OkLab coefficients at neutral RGB.
            let powerless_a = x[1] <= 1e-7 || a.alpha == 0.;
            let powerless_b = y[1] <= 1e-7 || b.alpha == 0.;
            match (powerless_a, powerless_b) {
                (true, true) => {
                    x[2] = 0.;
                    y[2] = 0.;
                }
                (true, false) => x[2] = y[2],
                (false, true) => y[2] = x[2],
                _ => {}
            }
            // Both powerless hues stay zero, including under the longer policy.
            if !(powerless_a && powerless_b) {
                y[2] = x[2] + hue_delta(x[2], y[2], policy);
            }
        }
        let mut value = std::array::from_fn(|i| lerp(x[i] * a.alpha, y[i] * b.alpha, t) / alpha);
        let rgb = match self.interpolation {
            ColorInterpolation::LinearRgb => LinearRgb::new(self.output, value)?,
            ColorInterpolation::Oklab => Oklab::new(value)?.to_linear(self.output)?,
            ColorInterpolation::Oklch(_) => {
                // Hue is angular and must not be premultiplied by alpha.
                value[2] = lerp(x[2], y[2], t);
                Oklch::new(value[0], value[1], value[2])?
                    .to_oklab()?
                    .to_linear(self.output)?
            }
        };
        LinearRgba::new(rgb, alpha)
    }
}
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a * (1. - t) + b * t
}
fn hue_delta(a: f64, b: f64, policy: HueInterpolation) -> f64 {
    let delta = b - a;
    match policy {
        HueInterpolation::Shorter => {
            if delta > 180. {
                delta - 360.
            } else if delta < -180. {
                delta + 360.
            } else {
                delta
            }
        }
        HueInterpolation::Longer => {
            if delta > 0. && delta < 180. {
                delta - 360.
            } else if delta > -180. && delta <= 0. {
                delta + 360.
            } else {
                delta
            }
        }
        HueInterpolation::Increasing => {
            if delta < 0. {
                delta + 360.
            } else {
                delta
            }
        }
        HueInterpolation::Decreasing => {
            if delta > 0. {
                delta - 360.
            } else {
                delta
            }
        }
    }
}

#[cfg(test)]
#[path = "gradient_tests.rs"]
mod tests;
