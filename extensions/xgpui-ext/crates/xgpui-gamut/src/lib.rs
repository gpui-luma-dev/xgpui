//! Opt-in wide SDR content producers. Window presentation remains a host concern.
//! Colors are explicitly linear Display P3/D65; native GPUI colors remain sRGB.
/// Shared CPU color math, also available independently as `xgpui-color`.
pub use xgpui_color as color;

#[cfg(feature = "instanced")]
pub mod instanced;

use anyhow::{Result, ensure};
use gpui::{CustomContent, DevicePixels, Size};
use std::any::Any;

#[cfg(test)]
static PRODUCER_ALLOCATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
mod dx11;
#[cfg(target_os = "macos")]
mod metal;

/// Immutable bounded-SDR color in linear Display P3/D65, with straight input alpha.
/// Prepared textures store associated RGB/alpha and require a wide-capable host.
///
/// Native Metal and D3D11 window presentation accept this output when the host
/// has enabled `WideGamutSdr`. Windows native execution/display validation remains
/// outstanding. Other backends return an explicit preparation error.
/// Preparation allocates a texture; output caching/pooling is not provided yet.
#[derive(Debug)]
pub struct WideColor {
    rgba: [f32; 4],
}
impl WideColor {
    /// Construct from finite straight linear P3 RGB and alpha in 0..=1.
    /// This does not decode sRGB, load an image profile or enable a window mode.
    pub fn linear_p3(rgba: [f32; 4]) -> Result<Self> {
        ensure!(
            rgba.iter().all(|c| c.is_finite() && (0. ..=1.).contains(c)),
            "linear P3 SDR channels must be finite and in 0..=1"
        );
        Ok(Self { rgba })
    }
}
impl CustomContent for WideColor {
    fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        validate_extent(extent)?;
        #[cfg(target_os = "macos")]
        {
            self.prepare_metal(backend, extent)
        }
        #[cfg(target_os = "windows")]
        {
            self.prepare_dx11(backend, extent)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = backend;
            anyhow::bail!("WideColor requires native Metal or D3D11 preparation")
        }
    }
}
fn validate_extent(extent: Size<DevicePixels>) -> Result<()> {
    ensure!(
        (1..=16384).contains(&extent.width.0) && (1..=16384).contains(&extent.height.0),
        "invalid wide color extent"
    );
    ensure!(
        extent.width.0 as u64 * extent.height.0 as u64 * 8 <= 64 * 1024 * 1024,
        "wide color exceeds 64 MiB"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;
    #[test]
    fn rejects_invalid_color_channels() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
            for i in 0..4 {
                let mut rgba = [0.5; 4];
                rgba[i] = bad;
                assert!(WideColor::linear_p3(rgba).is_err());
            }
        }
        assert!(WideColor::linear_p3([0.; 4]).is_ok());
        assert!(WideColor::linear_p3([1.; 4]).is_ok());
    }
    #[test]
    fn checks_float16_budget_and_extent_before_backend_work() {
        for (w, h) in [(0, 1), (-1, 1), (1, 0), (16385, 1), (4096, 2049)] {
            assert!(validate_extent(size(DevicePixels(w), DevicePixels(h))).is_err());
        }
        assert!(validate_extent(size(DevicePixels(4096), DevicePixels(2048))).is_ok());
        let color = WideColor::linear_p3([1., 0., 0., 0.5]).unwrap();
        assert!(
            color
                .prepare(&(), size(DevicePixels(0), DevicePixels(1)))
                .unwrap_err()
                .to_string()
                .contains("extent")
        );
    }
    #[test]
    fn rejects_unknown_backend_without_reinterpreting_color() {
        let color = WideColor::linear_p3([1., 0., 0., 0.5]).unwrap();
        assert!(
            color
                .prepare(&(), size(DevicePixels(8), DevicePixels(8)))
                .is_err()
        );
    }
}
