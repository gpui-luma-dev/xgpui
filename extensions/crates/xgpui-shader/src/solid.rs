//! Portable immutable encoded-sRGB solids; the host owns native realization.
use gpui::content::{
    ColorValue, Content, ContentBuilder, ContentError, ContentRequest, ContentSource, ImageRef,
};

/// Immutable straight-alpha encoded-sRGB color. No device, command buffer or
/// output cache belongs to the producer; cloned snapshots retain content identity.
#[derive(Clone, Debug)]
pub struct SolidColor {
    value: ColorValue,
    content: Content,
}
impl SolidColor {
    /// Validate finite straight-alpha encoded-sRGB channels in 0..=1.
    pub fn new(rgba: [f32; 4]) -> Result<Self, ContentError> {
        let value = ColorValue::srgb(rgba)?;
        let content = Content::solid(value.clone());
        Ok(Self { value, content })
    }
    /// Original straight-alpha encoded-sRGB channels (canonical zero).
    pub fn rgba(&self) -> [f32; 4] {
        self.value.rgba()
    }
    /// Retained portable source for ContentDraw or painted().bg_paint(...).
    pub fn content(&self) -> Content {
        self.content.clone()
    }
}
impl ContentSource for SolidColor {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> Result<ImageRef, ContentError> {
        cx.solid(request.extent(), self.value.clone())
    }
}
impl From<SolidColor> for Content {
    fn from(value: SolidColor) -> Self {
        value.content
    }
}
impl From<SolidColor> for crate::Paint {
    fn from(value: SolidColor) -> Self {
        value.content.into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{content::*, px, size};
    #[test]
    fn validates_all_straight_channels_even_when_transparent() {
        for alpha in [0., 0.5, 1.] {
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
                for channel in 0..4 {
                    let mut rgba = [0.25, 0.5, 0.75, alpha];
                    rgba[channel] = bad;
                    assert_eq!(
                        SolidColor::new(rgba).unwrap_err().kind,
                        ContentErrorKind::InvalidDescriptor
                    );
                }
            }
        }
    }
    #[test]
    fn declares_rgba8_srgb_straight_alpha_and_shares_equal_recipes() {
        let solid = SolidColor::new([0.2, 0.4, 0.6, 0.5]).unwrap();
        let request =
            ContentRequest::new(size(px(3.), px(5.)), ImageExtent::new(6, 10).unwrap(), 2.)
                .unwrap();
        let roots = [solid.content(), Content::new(solid.clone())].map(|content| ContentRoot {
            content,
            request: request.clone(),
        });
        let graph = ContentGraph::describe(
            &roots,
            ContentCapabilities::default(),
            GraphLimits::default(),
        )
        .unwrap();
        assert_eq!(graph.nodes().len(), 1);
        assert_eq!(graph.roots()[0], graph.roots()[1]);
        let ResourceDescriptor::Image(d) = &graph.nodes()[0].outputs()[0] else {
            panic!("image")
        };
        assert_eq!(d.format(), PixelFormat::Rgba8);
        assert_eq!(d.extent(), request.extent());
        assert_eq!(
            d.interpretation(),
            ImageInterpretation {
                color: ColorEncoding::Srgb,
                alpha: AlphaMode::Straight
            }
        );
        let ContentOperation::Solid(value) = graph.nodes()[0].operation() else {
            panic!("solid")
        };
        assert_eq!(value.rgba(), [0.2, 0.4, 0.6, 0.5]);
    }
    #[test]
    fn snapshots_keep_identity_and_canonicalize_signed_zero() {
        let solid = SolidColor::new([-0., 0.5, 1., 0.]).unwrap();
        assert_eq!(solid.rgba()[0].to_bits(), 0);
        assert_eq!(solid.content().id(), solid.clone().content().id());
        let consumed: Content = solid.clone().into();
        assert_eq!(consumed.id(), solid.content().id());
        let paint: crate::Paint = solid.clone().into();
        assert!(matches!(paint, crate::Paint::Content(_)));
        assert_ne!(
            solid.content().id(),
            SolidColor::new([1.; 4]).unwrap().content().id()
        );
    }
}
