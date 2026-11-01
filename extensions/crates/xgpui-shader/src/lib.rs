//! Experimental independent GPU content as normal styled GPUI elements.
mod mesh;
pub use mesh::{ControlPoint, MeshGradient, MeshGrid, MeshStats};

mod solid;
pub use solid::SolidColor;

mod gradient;
pub use gradient::{Gradient, GradientKind, GradientStop};

mod paint;
pub use paint::{Paint, WindowPaintExt};
mod surface;
pub use surface::{Painted, PaintedElement, painted};


/// Experimental shader paints with reflected parameters and declarative paint integration.
#[cfg(feature = "portable-shaders")]
pub mod shader_paint;




/// Experimental typed shaders drawn directly in the host render pass.
#[cfg(feature = "portable-shaders")]
pub mod instanced;
#[cfg(feature = "portable-shaders")]
pub use instanced::WindowShaderExt;

/// Main-pass solid, gradient and procedurally modulated brushes for extension surfaces.
#[cfg(feature = "portable-shaders")]
pub mod brush;
#[cfg(feature = "portable-shaders")]
pub use brush::{
    BlendMode, Brush, BrushMeshPoint, BrushPoint, BrushStop, DirectPath, MAX_DIRECT_PATH_EDGES,
    MAX_DIRECT_PATH_EXTENT, MAX_DIRECT_PATH_TRIANGLES, Noise, NoiseKind,
};

#[cfg(feature = "portable-shaders")]
mod backdrop;
#[cfg(feature = "portable-shaders")]
pub use backdrop::BackdropBlur;

#[cfg(feature = "portable-shaders")]
mod layer_effect;
#[cfg(feature = "portable-shaders")]
pub use gpui::LayerBlurQuality;
#[cfg(feature = "portable-shaders")]
pub use layer_effect::LayerEffect;

/// Selectable kinetic typography with optional brush fills and glyph-only shadows.
pub mod typography;
