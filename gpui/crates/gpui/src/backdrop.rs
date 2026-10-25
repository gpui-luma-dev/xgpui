//! Explicit bounded sampling of the already painted SDR framebuffer.
use crate::*;
use anyhow::{Result, ensure};
use std::sync::Arc;

/// Maximum backdrop boundaries encoded in a frame.
pub const MAX_BACKDROPS: usize = 4;
/// Maximum full-resolution capture side, including blur halo.
pub const MAX_BACKDROP_SIDE: u32 = 1024;
/// Downsampling factor used by the separable filter.
pub const BACKDROP_DOWNSAMPLE: u32 = 4;

/// Retained scene description for an explicit framebuffer pass barrier.
#[derive(Clone, Debug)]
pub struct BackdropDraw {
    /// Scene ordering barrier.
    pub order: DrawOrder,
    /// Sampling footprint, including the blur halo.
    pub bounds: Bounds<ScaledPixels>,
    /// Full viewport used to order the pass barrier.
    pub content_mask: ContentMask<ScaledPixels>,
    /// Rectangle receiving the filtered pixels.
    pub surface_bounds: Bounds<ScaledPixels>,
    /// Ancestor content clip.
    pub clip: ContentMask<ScaledPixels>,
    /// Clamped surface corner radii in device pixels.
    pub corner_radii: Corners<ScaledPixels>,
    /// Gaussian support radius in device pixels.
    pub radius: f32,
    /// Filter strength, including element opacity.
    pub opacity: f32,
    /// Immutable shared filter program.
    pub program: Arc<SceneShaderProgram>,
}
/// Checked allocation/dispatch plan shared by every backend.
#[derive(Clone, Debug)]
pub struct BackdropPlan {
    /// Full-resolution source crop origin.
    pub origin: [u32; 2],
    /// Full-resolution source crop extent.
    pub size: [u32; 2],
    /// Quarter-resolution ping-pong extent.
    pub reduced: [u32; 2],
    /// Horizontal/downsample, vertical, and main-target composition uniforms.
    pub parameters: [[[f32; 4]; 6]; 3],
}
impl BackdropDraw {
    /// Validate before recording GPU work. The working set is at most 18 MiB
    /// across four boundaries (excluding driver and in-flight retention).
    pub fn plan(&self, viewport: Size<DevicePixels>) -> Result<BackdropPlan> {
        ensure!(
            viewport.width.0 > 0 && viewport.height.0 > 0,
            "invalid backdrop viewport"
        );
        let b = self.surface_bounds;
        let clip = self.clip.bounds;
        let values = [
            b.origin.x.0,
            b.origin.y.0,
            b.size.width.0,
            b.size.height.0,
            clip.origin.x.0,
            clip.origin.y.0,
            clip.size.width.0,
            clip.size.height.0,
            self.radius,
            self.opacity,
            self.corner_radii.top_left.0,
            self.corner_radii.top_right.0,
            self.corner_radii.bottom_right.0,
            self.corner_radii.bottom_left.0,
        ];
        ensure!(
            values.iter().all(|v| v.is_finite()),
            "non-finite backdrop geometry"
        );
        ensure!(
            self.radius > 0. && self.radius <= 128.,
            "backdrop radius must be in (0,128] device pixels"
        );
        ensure!(
            (0. ..=1.).contains(&self.opacity),
            "invalid backdrop opacity"
        );
        ensure!(
            b.size.width.0 > 0. && b.size.height.0 > 0.,
            "empty backdrop surface"
        );
        ensure!(
            values[10..].iter().all(|v| *v >= 0.),
            "invalid backdrop corner radii"
        );
        // Include reconstruction/bilinear support beyond the Gaussian taps.
        let halo = self.radius + BACKDROP_DOWNSAMPLE as f32;
        let x = (b.origin.x.0 - halo)
            .floor()
            .max(0.)
            .min(viewport.width.0 as f32) as u32;
        let y = (b.origin.y.0 - halo)
            .floor()
            .max(0.)
            .min(viewport.height.0 as f32) as u32;
        let right = (b.origin.x.0 + b.size.width.0 + halo)
            .ceil()
            .max(0.)
            .min(viewport.width.0 as f32) as u32;
        let bottom = (b.origin.y.0 + b.size.height.0 + halo)
            .ceil()
            .max(0.)
            .min(viewport.height.0 as f32) as u32;
        let size = [right.saturating_sub(x), bottom.saturating_sub(y)];
        ensure!(
            size.iter().all(|v| *v > 0 && *v <= MAX_BACKDROP_SIDE),
            "backdrop capture including halo must fit within 1024x1024 device pixels"
        );
        let reduced = [
            size[0].div_ceil(BACKDROP_DOWNSAMPLE),
            size[1].div_ceil(BACKDROP_DOWNSAMPLE),
        ];
        let mut parameters = [[[0.; 4]; 6]; 3];
        for pass in 0..2 {
            parameters[pass][0] = [reduced[0] as f32, reduced[1] as f32, pass as f32, 0.];
            parameters[pass][1] = [0., 0., reduced[0] as f32, reduced[1] as f32];
            parameters[pass][2] = [0., 0., 1., 1.];
        }
        parameters[0][4][0] = self.radius / 4.;
        parameters[1][4][1] = self.radius / 4. * reduced[1] as f32 / size[1] as f32;
        parameters[2][0] = [viewport.width.0 as f32, viewport.height.0 as f32, 2., 0.];
        parameters[2][1] = [b.origin.x.0, b.origin.y.0, b.size.width.0, b.size.height.0];
        parameters[2][2] = [
            (b.origin.x.0 - x as f32) / size[0] as f32,
            (b.origin.y.0 - y as f32) / size[1] as f32,
            b.size.width.0 / size[0] as f32,
            b.size.height.0 / size[1] as f32,
        ];
        parameters[2][3] = [
            clip.origin.x.0,
            clip.origin.y.0,
            clip.origin.x.0 + clip.size.width.0,
            clip.origin.y.0 + clip.size.height.0,
        ];
        parameters[2][4][2] = self.opacity;
        parameters[2][5] = [
            self.corner_radii.top_left.0,
            self.corner_radii.top_right.0,
            self.corner_radii.bottom_right.0,
            self.corner_radii.bottom_left.0,
        ];
        Ok(BackdropPlan {
            origin: [x, y],
            size,
            reduced,
            parameters,
        })
    }
}
