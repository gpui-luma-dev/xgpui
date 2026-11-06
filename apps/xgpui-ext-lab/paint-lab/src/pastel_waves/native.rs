//! Native producer snapshot for pastel waves. Every desktop GPUI renderer hands
//! `prepare` its own recording context: Metal on macOS, D3D11 on Windows and
//! wgpu on Linux. Which producers exist is decided at build time by target;
//! all of them run the same two passes (per-column curves, then one shading
//! pass) from pastel_waves.metal or producer.wgsl.
use anyhow::{Result, bail};
use gpui::{CustomContent, DevicePixels, Size};
use std::any::Any;

/// Label of this build's native producer, as shown by the renderer button.
pub(super) const BACKEND: &str = if cfg!(target_os = "macos") {
    "metal"
} else if cfg!(target_os = "windows") {
    "d3d11"
} else {
    "wgpu"
};

/// Immutable frame snapshot; create one per animation frame.
#[derive(Debug)]
pub(super) struct PastelWavesNative {
    pub phase: f32,
    pub grain: bool,
    pub scale: f32,
    /// Per-ribbon normalized fill y bounds (min, max).
    pub fill_bounds: [f32; 18],
}

/// Matches `Uniforms` in pastel_waves.metal and producer.wgsl.
#[repr(C, align(16))]
pub(super) struct Uniforms {
    frame: [f32; 4],
    extent: [f32; 4],
    bounds: [[f32; 4]; 5],
}
impl Uniforms {
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: plain `f32` arrays with no padding inside the 16-aligned struct.
        unsafe {
            std::slice::from_raw_parts((self as *const Self).cast(), std::mem::size_of::<Self>())
        }
    }
}

impl PastelWavesNative {
    pub(super) fn uniforms(&self, width: usize, height: usize) -> Uniforms {
        let mut bounds = [[0f32; 4]; 5];
        for (index, value) in self.fill_bounds.iter().enumerate() {
            bounds[index / 4][index % 4] = *value;
        }
        Uniforms {
            frame: [self.phase, if self.grain { 1. } else { 0. }, self.scale, 0.],
            extent: [width as f32, height as f32, 0., 0.],
            bounds,
        }
    }
}

impl CustomContent for PastelWavesNative {
    fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        #[cfg(target_os = "macos")]
        if let Some(ctx) = backend.downcast_ref::<gpui_apple::custom_draw::MetalCustomContext>() {
            return super::metal::prepare(self, ctx, extent);
        }
        #[cfg(target_os = "windows")]
        if let Some(ctx) = backend.downcast_ref::<gpui_windows::custom_draw::Dx11CustomContext>() {
            return super::dx11::prepare(self, ctx, extent);
        }
        if let Some(ctx) = backend.downcast_ref::<gpui_wgpu::custom_draw::WgpuCustomContext>() {
            return super::wgpu_producer::prepare(self, ctx, extent);
        }
        bail!("pastel waves has no native producer for this backend")
    }
}

/// The wgpu producer (Linux's, here on wgpu's Metal backend) against the
/// native Metal producer. Run with `just wgpu-test`.
#[cfg(all(test, feature = "wgpu", target_os = "macos"))]
mod tests {
    use super::*;
    use gpui::{
        Bounds, ContentMask, Corners, CustomDraw, PlatformHeadlessRenderer, ScaledPixels, Scene,
        point, size,
    };
    use std::sync::Arc;

    fn render(
        renderer: &mut dyn PlatformHeadlessRenderer,
        phase: f32,
        grain: bool,
    ) -> Result<Vec<u8>> {
        let (w, h) = (640., 400.);
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(w), ScaledPixels(h)),
        );
        let content: Arc<dyn CustomContent> = Arc::new(PastelWavesNative {
            phase,
            grain,
            scale: 2.,
            fill_bounds: super::super::fill_bounds(phase),
        });
        let mut scene = Scene::default();
        scene.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Corners::default(),
            opacity: 1.,
            content,
            coverage: None,
        });
        scene.finish();
        Ok(renderer
            .render_scene_to_image(&scene, size(DevicePixels(w as i32), DevicePixels(h as i32)))?
            .into_raw())
    }

    #[::core::prelude::v1::test]
    fn wgpu_pastel_producer_matches_metal() -> Result<()> {
        let mut metal = gpui_platform::current_headless_renderer()?.expect("native renderer");
        let mut wgpu = gpui_wgpu::WgpuHeadlessRenderer::new()?;
        for (phase, grain) in [(0., true), (0.9, true), (2.2, false), (3.0, true)] {
            let expected = render(metal.as_mut(), phase, grain)?;
            let actual = render(&mut wgpu, phase, grain)?;
            assert_eq!(expected.len(), actual.len());
            // Guard against two matching blank frames: ribbons span a wide range.
            let (lo, hi) = expected
                .iter()
                .fold((255u8, 0u8), |(l, h), &v| (l.min(v), h.max(v)));
            assert!(hi - lo > 100, "phase {phase}: flat frame {lo}..{hi}");
            let (mut worst, mut differing) = (0u8, 0usize);
            for (a, b) in expected.iter().zip(&actual) {
                let d = a.abs_diff(*b);
                worst = worst.max(d);
                differing += usize::from(d > 0);
            }
            // Same algorithm, different shader compilers: rounding only.
            assert!(worst <= 1, "phase {phase}: max channel difference {worst}");
            assert!(
                differing * 1000 < expected.len(),
                "phase {phase}: {differing} channels differ"
            );
        }
        Ok(())
    }
}
