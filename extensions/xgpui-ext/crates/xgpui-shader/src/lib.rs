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
