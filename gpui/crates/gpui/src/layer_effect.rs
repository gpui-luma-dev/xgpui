//! Bounded isolation and retained results for explicitly captured paint subtrees.
use crate::*;
use anyhow::{Result, ensure};
use std::sync::Arc;

/// Maximum captured groups in one frame (including nested groups).
pub const MAX_LAYER_GROUPS: usize = 4;
/// Maximum capture width or height in physical pixels.
pub const MAX_LAYER_SIDE: u32 = 1024;
/// Maximum retained command identities per group.
pub const MAX_LAYER_COMMANDS: usize = 65_536;

/// Encoded-sRGB color operations, applied in saturation/brightness/contrast/inversion order.
#[derive(Clone, Copy, Debug)]
pub struct LayerColorFilter {
    /// Saturation in [0,2], with 1 unchanged.
    pub saturation: f32,
    /// Brightness multiplier in [0,2], with 1 unchanged.
    pub brightness: f32,
    /// Contrast around straight RGB 0.5, in [0,2], with 1 unchanged.
    pub contrast: f32,
    /// Inversion strength in [0,1], with 0 unchanged and 1 complemented RGB.
    pub invert: f32,
}
impl Default for LayerColorFilter {
    fn default() -> Self {
        Self {
            saturation: 1.,
            brightness: 1.,
            contrast: 1.,
            invert: 0.,
        }
    }
}

/// Immutable capture description shared by its paired scene boundaries.
#[derive(Debug)]
pub struct LayerCapture {
    /// Physical capture bounds, already intersected with ancestor masks.
    pub bounds: Bounds<ScaledPixels>,
    /// Opacity of the completed group, including inherited opacity exactly once.
    pub opacity: f32,
    /// Encoded-sRGB saturation in [0,2], with 0 grayscale and 1 unchanged.
    pub saturation: f32,
    /// Encoded-sRGB brightness multiplier in [0,2].
    pub brightness: f32,
    /// Encoded-sRGB contrast in [0,2], centered on straight RGB 0.5.
    pub contrast: f32,
    /// Inversion strength in [0,1], with 0 unchanged and 1 complemented RGB.
    pub invert: f32,
    /// Shared encoded-sRGB capture/composition shader.
    pub program: Arc<SceneShaderProgram>,
}
/// A begin/end marker. The renderer must preserve these global pass barriers.
#[derive(Clone, Debug)]
pub struct LayerBoundary {
    /// Scene order.
    pub order: DrawOrder,
    /// Full viewport ordering fence.
    pub bounds: Bounds<ScaledPixels>,
    /// Full viewport mask for the ordering fence.
    pub content_mask: ContentMask<ScaledPixels>,
    /// Paired immutable description.
    pub capture: Arc<LayerCapture>,
    /// True at entry, false at exit.
    pub begin: bool,
}
/// Checked capture plan, independent of the GPU backend.
#[derive(Clone, Debug)]
pub struct LayerPlan {
    /// Begin marker index in Scene::layer_boundaries.
    pub begin_index: usize,
    /// End marker index in Scene::layer_boundaries.
    pub end_index: usize,
    /// Source rectangle origin.
    pub origin: [u32; 2],
    /// Physical texture extent.
    pub size: [u32; 2],
    /// Clear and group-composition uniform blocks (six float4 values each).
    pub parameters: [[[f32; 4]; 6]; 2],
    /// Exact immutable command identities; preserved by Scene::replay.
    pub revision: Arc<[u64]>,
    /// Live external content cannot use retained filtered results.
    pub reusable: bool,
}
/// Cumulative statistics, available through headless renderer test support.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct LayerCacheStats {
    /// Group results rendered and captured.
    pub captures: u64,
    /// Retained group results reused without drawing their descendants.
    pub hits: u64,
}
impl LayerPlan {
    pub(crate) fn new(
        capture: &LayerCapture,
        viewport: Size<DevicePixels>,
        begin_index: usize,
    ) -> Result<Self> {
        ensure!(
            capture.program.output_encoding() == ShaderOutputEncoding::EncodedSrgb
                && !capture.program.uses_glyph_mask()
                && !capture.program.uses_path_mask(),
            "layer capture requires an SDR composition program"
        );
        let b = capture.bounds;
        ensure!(
            [
                b.origin.x.0,
                b.origin.y.0,
                b.size.width.0,
                b.size.height.0,
                capture.opacity,
                capture.saturation,
                capture.brightness,
                capture.contrast,
                capture.invert
            ]
            .iter()
            .all(|v| v.is_finite()),
            "non-finite layer geometry"
        );
        ensure!(
            viewport.width.0 > 0
                && viewport.height.0 > 0
                && b.size.width.0 > 0.
                && b.size.height.0 > 0.,
            "empty layer capture"
        );
        ensure!(
            (0. ..=1.).contains(&capture.opacity),
            "invalid group opacity"
        );
        ensure!(
            capture.saturation.is_finite() && (0. ..=2.).contains(&capture.saturation),
            "invalid group saturation"
        );
        ensure!(
            (0. ..=2.).contains(&capture.brightness),
            "invalid group brightness"
        );
        ensure!(
            (0. ..=2.).contains(&capture.contrast),
            "invalid group contrast"
        );
        ensure!(
            (0. ..=1.).contains(&capture.invert),
            "invalid group inversion"
        );
        let clamp_x = |x: f32| x.max(0.).min(viewport.width.0 as f32) as u32;
        let clamp_y = |y: f32| y.max(0.).min(viewport.height.0 as f32) as u32;
        let origin = [clamp_x(b.origin.x.0.floor()), clamp_y(b.origin.y.0.floor())];
        let end = [
            clamp_x((b.origin.x.0 + b.size.width.0).ceil()),
            clamp_y((b.origin.y.0 + b.size.height.0).ceil()),
        ];
        let size = [
            end[0].saturating_sub(origin[0]),
            end[1].saturating_sub(origin[1]),
        ];
        ensure!(
            size.iter().all(|v| *v > 0 && *v <= MAX_LAYER_SIDE),
            "layer capture must fit within 1024x1024 device pixels"
        );
        let mut parameters = [[[0.; 4]; 6]; 2];
        for pass in 0..2 {
            parameters[pass][0] = [
                viewport.width.0 as f32,
                viewport.height.0 as f32,
                3. + pass as f32,
                0.,
            ];
            parameters[pass][1] = [
                origin[0] as f32,
                origin[1] as f32,
                size[0] as f32,
                size[1] as f32,
            ];
            parameters[pass][2] = [0., 0., 1., 1.];
            parameters[pass][3] = [
                origin[0] as f32,
                origin[1] as f32,
                end[0] as f32,
                end[1] as f32,
            ];
            parameters[pass][4][2] = capture.opacity;
            parameters[pass][4][3] = capture.saturation;
            parameters[pass][5] = [capture.brightness, capture.contrast, capture.invert, 0.];
        }
        Ok(Self {
            begin_index,
            end_index: 0,
            origin,
            size,
            parameters,
            revision: Arc::from([]),
            reusable: true,
        })
    }
    /// Whether a previous raw capture is safe to reuse. Own opacity/color filters
    /// are evaluated at composition and do not invalidate the raw child pixels.
    pub fn matches(&self, previous: &Self) -> bool {
        self.reusable
            && previous.reusable
            && self.origin == previous.origin
            && self.size == previous.size
            && self.revision == previous.revision
    }
}
