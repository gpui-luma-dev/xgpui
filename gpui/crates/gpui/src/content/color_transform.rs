//! Placement color transforms, independent of source pixels and upload identity.
use super::*;
use crate::LinearImagePaint;
const IDENTITY: [[f32; 4]; 3] = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Encoded([[f32; 4]; 3]),
    Linear(LinearImagePaint),
}
/// Checked, alpha-preserving color adjustment applied after source sampling and
/// unassociation, before output conversion, native coverage and element opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageColorTransform(Kind);
impl Default for ImageColorTransform {
    fn default() -> Self {
        Self(Kind::Encoded(IDENTITY))
    }
}
impl ImageColorTransform {
    /// Affine RGB rows in encoded sRGB. Nonidentity transforms require an encoded
    /// sRGB source. Clamp once after the combined transform; alpha is unchanged.
    pub fn encoded_srgb(rows: [[f32; 4]; 3]) -> Result<Self, ContentError> {
        LinearImagePaint::new(rows, [0., 0., 1., 1.]).map_err(|e| invalid(&e.to_string()))?;
        Ok(Self(Kind::Encoded(rows)))
    }
    /// Affine rows and source-space vignette in validated linear sRGB/P3 working
    /// primaries. Wide output preserves extended RGB until final presentation.
    pub fn linear(paint: LinearImagePaint) -> Self {
        Self(Kind::Linear(paint))
    }
    /// Encoded affine rows, or identity when linear adjustments are selected.
    pub fn encoded_rows(self) -> [[f32; 4]; 3] {
        match self.0 {
            Kind::Encoded(rows) => rows,
            _ => IDENTITY,
        }
    }
    /// Validated linear working-space adjustments, when selected.
    pub fn linear_paint(self) -> Option<LinearImagePaint> {
        match self.0 {
            Kind::Linear(paint) => Some(paint),
            _ => None,
        }
    }
    /// Compositor flags: working P3, adjustment kind (0 identity, 1 encoded, 2 linear).
    pub fn shader_flags(self) -> [f32; 2] {
        match self.0 {
            Kind::Linear(paint) => [if paint.is_p3() { 1. } else { 0. }, 2.],
            Kind::Encoded(rows) => [0., if rows == IDENTITY { 0. } else { 1. }],
        }
    }
    /// Renderer validation against the displayed source and accepted output domain.
    /// Encoded transforms never silently narrow a differently tagged source.
    pub fn validate(self, image: &ImageDescriptor, wide_output: bool) -> Result<(), ContentError> {
        if self.shader_flags()[1] > 0. {
            if image.interpretation().color == ColorEncoding::Coverage {
                return Err(invalid("color adjustments require a color image"));
            }
            if matches!(self.0, Kind::Encoded(_))
                && image.interpretation().color != ColorEncoding::Srgb
            {
                return Err(invalid(
                    "encoded image transforms require encoded sRGB source pixels",
                ));
            }
        }
        if wide_output && let Kind::Linear(paint) = self.0 {
            if paint
                .rows()
                .iter()
                .any(|row| row.iter().map(|v| f64::from(v.abs())).sum::<f64>() > 4.)
            {
                return Err(invalid("wide SDR adjustment row magnitude exceeds 4"));
            }
        }
        Ok(())
    }
}
fn invalid(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::InvalidDescriptor, message)
}
