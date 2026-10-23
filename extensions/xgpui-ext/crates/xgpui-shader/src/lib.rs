//! Experimental independent GPU content as normal styled GPUI elements.
use gpui::{
    CustomContent, IntoElement, ParentElement, Style, StyleRefinement, Styled, canvas, div,
};
use refineable::Refineable;
use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
mod mesh;
#[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
pub use mesh::{ControlPoint, MeshGradient, MeshGrid, MeshStats};

#[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
mod solid;
#[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
pub use solid::SolidColor;

#[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
mod gradient;
#[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
pub use gradient::{Gradient, GradientKind, GradientStop};

/// A styled container hosting independently prepared GPU content.
/// Parent opacity/overflow use GPUI's normal div behavior; local corners are
/// explicitly applied by the custom compositor.
pub struct Shader {
    content: Arc<dyn CustomContent>,
    style: StyleRefinement,
}

/// Place a reusable producer in the element tree.
pub fn shader(content: Arc<dyn CustomContent>) -> Shader {
    Shader {
        content,
        style: StyleRefinement::default(),
    }
}

impl Styled for Shader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for Shader {
    type Element = gpui::Div;
    fn into_element(self) -> Self::Element {
        let mut resolved = Style::default();
        resolved.refine(&self.style);
        let content = self.content;
        let canvas = canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                let radii = resolved.corner_radii.to_pixels(window.rem_size());
                window.paint_custom(bounds, radii, content);
            },
        )
        .size_full();
        let mut container = div().child(canvas);
        *container.style() = self.style;
        container
    }
}

#[cfg(feature = "wgpu")]
mod wgpu;

/// Experimental portable fragment paints. Version one is deliberately restricted.
#[cfg(feature = "portable-shaders")]
pub mod experimental;

#[cfg(target_os = "windows")]
mod dx11;

#[cfg(any(target_os = "windows", feature = "wgpu"))]
mod effect_params;
#[cfg(any(target_os = "windows", test))]
mod hlsl;
