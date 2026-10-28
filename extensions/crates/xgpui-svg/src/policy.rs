use anyhow::{Result, ensure};
use gpui::{DevicePixels, Size, size};
use std::time::Duration;

pub const MIB: usize = 1024 * 1024;

/// Bounds image buffers, not resvg's filter intermediates or GPU residency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterPolicy {
    pub max_output_bytes: usize,
    pub preview_edge: i32,
    pub settle_delay: Duration,
}
impl Default for RasterPolicy {
    fn default() -> Self {
        Self {
            max_output_bytes: 16 * MIB,
            preview_edge: 640,
            settle_delay: Duration::from_millis(120),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub requested: Size<DevicePixels>,
    pub output: Size<DevicePixels>,
}
impl Resolution {
    pub fn limited(self) -> bool {
        self.requested != self.output
    }
}
pub fn image_bytes(pixels: Size<DevicePixels>) -> usize {
    (i32::from(pixels.width).max(0) as usize)
        .saturating_mul(i32::from(pixels.height).max(0) as usize)
        .saturating_mul(4)
}
impl RasterPolicy {
    pub fn resolution(self, logical: Size<f32>, zoom: f32, density: f32) -> Result<Resolution> {
        ensure!(
            (4..=256 * MIB).contains(&self.max_output_bytes),
            "output budget must be 4 bytes..=256 MiB"
        );
        ensure!(
            self.preview_edge > 0 && self.preview_edge <= 8192,
            "preview edge must be 1..=8192"
        );
        ensure!(
            [logical.width, logical.height, zoom, density]
                .iter()
                .all(|v| v.is_finite() && *v > 0.),
            "size, zoom and density must be finite and positive"
        );
        let w = f64::from(logical.width) * f64::from(zoom) * f64::from(density);
        let h = f64::from(logical.height) * f64::from(zoom) * f64::from(density);
        ensure!(
            w <= f64::from(i32::MAX) && h <= f64::from(i32::MAX),
            "requested size exceeds device pixel range"
        );
        let requested = size(
            DevicePixels(w.ceil().max(1.) as i32),
            DevicePixels(h.ceil().max(1.) as i32),
        );
        Ok(Resolution {
            requested,
            output: fit(requested, 8192, self.max_output_bytes),
        })
    }
    pub fn preview(self, output: Size<DevicePixels>) -> Size<DevicePixels> {
        fit(output, self.preview_edge, self.max_output_bytes)
    }
}
fn fit(requested: Size<DevicePixels>, edge: i32, bytes: usize) -> Size<DevicePixels> {
    let edge = edge.clamp(1, 8192);
    let w = f64::from(i32::from(requested.width).max(1));
    let h = f64::from(i32::from(requested.height).max(1));
    let pixels = (bytes / 4).max(1);
    let scale = 1_f64
        .min(f64::from(edge) / w.max(h))
        .min((pixels as f64 / (w * h)).sqrt());
    // Flooring and the integer correction also cover extremely thin documents.
    let mut width = (w * scale).floor().max(1.) as usize;
    let mut height = (h * scale).floor().max(1.) as usize;
    width = width.min(edge as usize).min(pixels);
    height = height.min(edge as usize).min(pixels / width);
    size(
        DevicePixels(width as i32),
        DevicePixels(height.max(1) as i32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budgets_bound_square_portrait_landscape_and_extremely_thin_images() {
        for budget in [4, 4096, 16 * MIB, 32 * MIB] {
            for logical in [
                size(320., 320.),
                size(320., 80.),
                size(80., 320.),
                size(0.0001, 320.),
                size(320.00003, 206.93335),
            ] {
                let policy = RasterPolicy {
                    max_output_bytes: budget,
                    ..Default::default()
                };
                let r = policy.resolution(logical, 16., 4.).unwrap();
                assert!(image_bytes(r.output) <= budget);
                assert!(i32::from(r.output.width) > 0 && i32::from(r.output.height) > 0);
                assert!(i32::from(r.output.width) <= 8192 && i32::from(r.output.height) <= 8192);
                assert!(image_bytes(policy.preview(r.output)) <= budget);
            }
        }
    }
    #[test]
    fn limits_are_explicit_and_invalid_requests_fail() {
        let policy = RasterPolicy::default();
        assert!(
            !policy
                .resolution(size(320., 240.), 1., 2.)
                .unwrap()
                .limited()
        );
        assert!(
            policy
                .resolution(size(320., 240.), 16., 2.)
                .unwrap()
                .limited()
        );
        for value in [0., -1., f32::NAN, f32::INFINITY] {
            assert!(policy.resolution(size(320., 240.), value, 2.).is_err());
        }
    }
}
