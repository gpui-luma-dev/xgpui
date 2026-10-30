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

/// Working resolution for a captured subtree Gaussian blur.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerBlurQuality {
    /// Full resolution below 4 device pixels, half below 8, quarter otherwise.
    #[default]
    Adaptive,
    /// Full-resolution reference, preserving the original Gaussian sampling.
    FullResolution,
}

/// Blend a completed isolated subtree with its saved destination.
/// Uses encoded sRGB by default, or linear P3 in Metal WideGamutSdr.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum LayerBlendMode {
    /// Ordinary source-over composition.
    #[default]
    Normal = 0,
    /// Multiply straight source and destination RGB.
    Multiply = 1,
    /// Complementary multiply.
    Screen = 2,
    /// Multiply/screen selected by destination RGB.
    Overlay = 3,
    /// Brighten destination by dividing by complementary source RGB.
    ColorDodge = 4,
    /// Smooth source-controlled darkening or lightening.
    SoftLight = 5,
    /// Clamp source plus destination minus one.
    LinearBurn = 6,
}

/// Capture-domain color operations in saturation/brightness/contrast/inversion order,
/// followed by group opacity and destination blending. Default captures use
/// encoded sRGB; Metal WideGamutSdr captures use associated extended linear P3.
/// Linear filters preserve extended channels; artistic blend modes retain their
/// bounded operand definitions.
#[derive(Clone, Copy, Debug)]
pub struct LayerColorFilter {
    /// Working resolution policy for subtree blur.
    pub blur_quality: LayerBlurQuality,
    /// Gaussian support radius in logical pixels; zero disables blur.
    pub blur_radius: Pixels,
    /// Destination blend, applied after filtering and group opacity.
    pub blend_mode: LayerBlendMode,
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
            blur_quality: LayerBlurQuality::Adaptive,
            blur_radius: px(0.),
            blend_mode: LayerBlendMode::Normal,
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
    /// Working resolution policy for subtree blur.
    pub blur_quality: LayerBlurQuality,
    /// Gaussian support radius in device pixels, in [0,64].
    pub blur_radius: ScaledPixels,
    /// Destination blend for the completed group.
    pub blend_mode: LayerBlendMode,
    /// Source sampling rectangle, including any halo; bounded by the viewport/parent.
    pub bounds: Bounds<ScaledPixels>,
    /// Visible output clip, applied after filtering. None uses the source bounds.
    pub clip: Option<Bounds<ScaledPixels>>,
    /// Opacity of the completed group, including inherited opacity exactly once.
    pub opacity: f32,
    /// Capture-domain saturation in [0,2], with 0 grayscale and 1 unchanged.
    pub saturation: f32,
    /// Capture-domain brightness multiplier in [0,2].
    pub brightness: f32,
    /// Capture-domain contrast in [0,2], centered on straight RGB 0.5.
    pub contrast: f32,
    /// Inversion strength in [0,1], with 0 unchanged and 1 complemented RGB.
    pub invert: f32,
    /// Shared capture/composition shader. Metal wide captures set viewport.w to 2
    /// for extended linear-P3 filtering; 1 retains bounded P3, and 0 encoded sRGB.
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
    /// Clear, composition, horizontal and vertical blur blocks (six float4 each).
    pub parameters: [[[f32; 4]; 6]; 4],
    /// Device-pixel Gaussian support radius; zero skips filter passes.
    pub blur_radius: f32,
    /// Horizontal and vertical filter target extent.
    pub blur_size: [u32; 2],
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
                capture.invert,
                capture.blur_radius.0
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
        ensure!(
            (0. ..=64.).contains(&capture.blur_radius.0),
            "layer blur radius must be in [0,64] device pixels"
        );
        let factor = match capture.blur_quality {
            LayerBlurQuality::FullResolution => 1,
            LayerBlurQuality::Adaptive if capture.blur_radius.0 >= 8. => 4,
            LayerBlurQuality::Adaptive if capture.blur_radius.0 >= 4. => 2,
            LayerBlurQuality::Adaptive => 1,
        };
        let blur_size = size.map(|v| v.div_ceil(factor));
        let output_clip = capture.clip.unwrap_or(b);
        ensure!(
            [
                output_clip.origin.x.0,
                output_clip.origin.y.0,
                output_clip.size.width.0,
                output_clip.size.height.0
            ]
            .iter()
            .all(|v| v.is_finite())
                && output_clip.size.width.0 >= 0.
                && output_clip.size.height.0 >= 0.,
            "layer output clip must be finite and nonnegative"
        );
        let clip = output_clip.intersect(&Bounds::new(
            Point::default(),
            viewport.map(|p| ScaledPixels(p.0 as f32)),
        ));
        ensure!(
            [
                clip.origin.x.0,
                clip.origin.y.0,
                clip.size.width.0,
                clip.size.height.0
            ]
            .iter()
            .all(|v| v.is_finite())
                && clip.intersect(&b) == clip,
            "layer output clip must fit within its source capture"
        );
        let mut parameters = [[[0.; 4]; 6]; 4];
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
            if pass == 1 {
                parameters[pass][3] =
                    [clip.left().0, clip.top().0, clip.right().0, clip.bottom().0];
            }
            parameters[pass][4][2] = capture.opacity;
            parameters[pass][4][3] = capture.saturation;
            parameters[pass][5] = [
                capture.brightness,
                capture.contrast,
                capture.invert,
                capture.blend_mode as u8 as f32,
            ];
        }
        for pass in 0..2 {
            parameters[pass + 2][0] = [blur_size[0] as f32, blur_size[1] as f32, 5., 0.];
            parameters[pass + 2][1] = [0., 0., blur_size[0] as f32, blur_size[1] as f32];
            parameters[pass + 2][2] = [0., 0., 1., 1.];
            parameters[pass + 2][4] = [
                if pass == 0 { 1. } else { 0. },
                if pass == 1 { 1. } else { 0. },
                capture.blur_radius.0 * blur_size[pass] as f32 / size[pass] as f32,
                0.,
            ];
        }
        Ok(Self {
            blur_radius: capture.blur_radius.0,
            blur_size,
            begin_index,
            end_index: 0,
            origin,
            size,
            parameters,
            revision: Arc::from([]),
            reusable: true,
        })
    }
    /// Whether the two Gaussian passes must run for this frame. Raw child pixels
    /// can survive radius edits, but filtered pixels must match the current radius.
    pub fn needs_blur(&self, previous: Option<&Self>) -> bool {
        self.blur_radius > 0.
            && previous.is_none_or(|old| {
                !self.matches(old)
                    || old.blur_radius != self.blur_radius
                    || old.blur_size != self.blur_size
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
