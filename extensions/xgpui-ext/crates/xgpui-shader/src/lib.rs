//! Experimental independent GPU content as normal styled GPUI elements.
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

mod paint;
pub use paint::{Paint, WindowPaintExt};
mod surface;
pub use surface::{Painted, PaintedElement, Shader, painted, shader};

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

#[cfg(target_os = "macos")]
mod metal_cache;

/// Experimental typed shaders drawn directly in the host render pass.
#[cfg(feature = "portable-shaders")]
pub mod instanced;
#[cfg(feature = "portable-shaders")]
pub use instanced::WindowShaderExt;

/// Main-pass solid, gradient and procedurally modulated brushes for extension surfaces.
#[cfg(feature = "portable-shaders")]
pub mod brush;
#[cfg(feature = "portable-shaders")]
pub use brush::{BlendMode, Brush, BrushMeshPoint, BrushPoint, BrushStop, Noise, NoiseKind};

#[cfg(feature = "portable-shaders")]
mod backdrop;
#[cfg(feature = "portable-shaders")]
pub use backdrop::BackdropBlur;

#[cfg(feature = "portable-shaders")]
mod layer_effect;
#[cfg(feature = "portable-shaders")]
pub use layer_effect::LayerEffect;
