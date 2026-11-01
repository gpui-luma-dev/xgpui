//! Portable wide SDR content. Window presentation remains a host concern.
//! Colors are explicitly linear Display P3/D65; native GPUI colors remain sRGB.
/// Shared CPU color math, also available independently as `xgpui-color`.
pub use xgpui_color as color;

#[cfg(feature = "instanced")]
pub mod instanced;

use gpui::content::{
    AlphaMode, ColorEncoding, ColorValue, Content, ContentBuilder, ContentError, ContentRequest,
    ContentSource, ImageInterpretation, ImageRef,
};

/// Immutable bounded-SDR color with straight linear Display P3/D65 input.
/// The portable graph stores premultiplied linear P3 in RGBA16Float. The host
/// owns allocation, execution, caching and completion; the producer has no native
/// device or queue. Presentation requires a wide-capable window/executor.
#[derive(Clone, Debug)]
pub struct WideColor {
    rgba: [f32; 4],
    value: ColorValue,
    content: Content,
}
impl WideColor {
    /// Validate straight linear P3 RGB/alpha in 0..=1, then associate once in
    /// linear space. Does not decode sRGB or change the window's color mode.
    pub fn linear_p3(rgba: [f32; 4]) -> Result<Self, ContentError> {
        // Validate the unassociated channels before multiplying: transparent
        // invalid RGB must not disappear into an otherwise valid zero payload.
        ColorValue::linear_p3(rgba)?;
        let [r, g, b, a] = rgba;
        let value = ColorValue::new(
            [r * a, g * a, b * a, a],
            ImageInterpretation {
                color: ColorEncoding::LinearP3,
                alpha: AlphaMode::Premultiplied,
            },
        )?;
        let content = Content::solid(value.clone());
        Ok(Self {
            rgba,
            value,
            content,
        })
    }
    /// Original straight linear P3 channels, also used by the instanced painter.
    pub fn rgba(&self) -> [f32; 4] {
        self.rgba
    }
    /// Retain this immutable portable source identity across draws and clones.
    /// Use with `ContentDraw`/`Window::paint_content` or a portable paint adapter.
    pub fn content(&self) -> Content {
        self.content.clone()
    }
}
impl ContentSource for WideColor {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> Result<ImageRef, ContentError> {
        cx.solid(request.extent(), self.value.clone())
    }
}
impl From<WideColor> for Content {
    fn from(value: WideColor) -> Self {
        value.content
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{content::*, px, size};
    #[test]
    fn rejects_invalid_straight_channels_including_transparent_colors() {
        for alpha in [0., 0.5, 1.] {
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
                for i in 0..4 {
                    let mut rgba = [0.5, 0.5, 0.5, alpha];
                    rgba[i] = bad;
                    assert_eq!(
                        WideColor::linear_p3(rgba).unwrap_err().kind,
                        ContentErrorKind::InvalidDescriptor
                    );
                }
            }
        }
    }
    #[test]
    fn describes_associated_fp16_p3_and_shares_equivalent_outputs() {
        for alpha in [0., 0.5, 1.] {
            let color = WideColor::linear_p3([1., 0.5, 0.25, alpha]).unwrap();
            let request =
                ContentRequest::new(size(px(3.), px(5.)), ImageExtent::new(6, 10).unwrap(), 2.)
                    .unwrap();
            let roots = [color.content(), Content::new(color.clone())].map(|content| ContentRoot {
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
            let node = &graph.nodes()[0];
            let ResourceDescriptor::Image(descriptor) = &node.outputs()[0] else {
                panic!("image output")
            };
            assert_eq!(descriptor.format(), PixelFormat::Rgba16Float);
            assert_eq!(descriptor.extent(), request.extent());
            assert_eq!(
                descriptor.interpretation(),
                ImageInterpretation {
                    color: ColorEncoding::LinearP3,
                    alpha: AlphaMode::Premultiplied
                }
            );
            let ContentOperation::Solid(value) = node.operation() else {
                panic!("portable solid")
            };
            assert_eq!(value.rgba(), [alpha, alpha * 0.5, alpha * 0.25, alpha]);
        }
    }
    #[test]
    fn clones_retain_content_identity_and_changed_colors_get_new_snapshots() {
        let color = WideColor::linear_p3([1., 0., 0., 0.5]).unwrap();
        assert_eq!(color.clone().content().id(), color.content().id());
        let consumed: Content = color.clone().into();
        assert_eq!(consumed.id(), color.content().id());
        assert_ne!(
            WideColor::linear_p3([0., 1., 0., 0.5])
                .unwrap()
                .content()
                .id(),
            color.content().id()
        );
        assert_eq!(color.rgba(), [1., 0., 0., 0.5]);
    }
    #[test]
    fn executor_limits_replace_native_producer_extent_rules() {
        let color = WideColor::linear_p3([1.; 4]).unwrap();
        let root = ContentRoot {
            content: color.content(),
            request: ContentRequest::new(
                size(px(20.), px(20.)),
                ImageExtent::new(20, 20).unwrap(),
                1.,
            )
            .unwrap(),
        };
        let caps = ContentCapabilities {
            max_image_dimension: 16,
            ..Default::default()
        };
        assert_eq!(
            ContentGraph::describe(&[root], caps, GraphLimits::default())
                .unwrap_err()
                .kind,
            ContentErrorKind::UnsupportedCapability
        );
    }
}
