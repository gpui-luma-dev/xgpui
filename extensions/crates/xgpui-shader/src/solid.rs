use anyhow::{Result, bail, ensure};
use gpui::{CustomContent, DevicePixels, Size};
#[cfg(target_os = "macos")]
use gpui_apple::custom_draw::MetalCustomContext;
#[cfg(target_os = "macos")]
use gpui_apple::metal::*;
use std::{
    any::Any,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Immutable straight-alpha encoded-sRGB color with a per-device/extent cache.
/// Clearing an offscreen attachment is deliberately the smallest GPU producer:
/// it proves integration without mixing shader math into the first milestone.
pub struct SolidColor {
    rgba: [f32; 4],
    #[cfg(target_os = "macos")]
    cache: crate::metal_cache::OutputCache,
    #[cfg(feature = "wgpu")]
    wgpu: crate::wgpu::PaintCache,
    #[cfg(target_os = "windows")]
    pub(super) dx11: crate::dx11::PaintCache,
    preparations: AtomicUsize,
}
impl std::fmt::Debug for SolidColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolidColor")
            .field("rgba", &self.rgba)
            .finish_non_exhaustive()
    }
}
impl SolidColor {
    /// Create a snapshot from finite straight-alpha encoded-sRGB channels in 0..=1.
    pub fn new(rgba: [f32; 4]) -> Result<Self> {
        ensure!(
            rgba.iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "solid color channels must be finite and in 0..=1"
        );
        Ok(Self {
            rgba,
            #[cfg(target_os = "macos")]
            cache: crate::metal_cache::OutputCache::default(),
            #[cfg(feature = "wgpu")]
            wgpu: crate::wgpu::PaintCache::default(),
            #[cfg(target_os = "windows")]
            dx11: crate::dx11::PaintCache::default(),
            preparations: AtomicUsize::new(0),
        })
    }
    /// Number of texture generations (cache hits do not increment this).
    pub fn preparations(&self) -> usize {
        self.preparations.load(Ordering::Relaxed)
    }
}
#[cfg(target_os = "macos")]
impl SolidColor {
    fn prepare_metal(&self, backend: &dyn Any, size: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let Some(context) = backend.downcast_ref::<MetalCustomContext>() else {
            bail!("SolidColor requires the Metal backend");
        };
        let (output, generated) = self.cache.prepare(context, size, || {
            let descriptor = TextureDescriptor::new();
            descriptor.setTextureType(MTLTextureType::Type2D);
            descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            unsafe { descriptor.setWidth(size.width.0 as usize) };
            unsafe { descriptor.setHeight(size.height.0 as usize) };
            descriptor.setStorageMode(MTLStorageMode::Private);
            descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
            let texture = context
                .device
                .newTextureWithDescriptor(&descriptor)
                .expect("Metal texture allocation failed");
            let pass = RenderPassDescriptor::new();
            let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            attachment.setTexture(Some(&texture));
            attachment.setLoadAction(MTLLoadAction::Clear);
            attachment.setStoreAction(MTLStoreAction::Store);
            let c = self.rgba;
            attachment.setClearColor(MTLClearColor {
                red: c[0] as f64,
                green: c[1] as f64,
                blue: c[2] as f64,
                alpha: c[3] as f64,
            });
            let command = context
                .queue
                .commandBuffer()
                .expect("Metal command buffer allocation failed")
                .to_owned();
            command
                .renderCommandEncoderWithDescriptor(&pass)
                .expect("Metal render encoder allocation failed")
                .endEncoding();
            Ok((texture, command))
        })?;
        if generated {
            self.preparations.fetch_add(1, Ordering::Relaxed);
        }
        Ok(Box::new(output))
    }
}

#[cfg(target_os = "macos")]
impl SolidColor {
    /// Device name and total bytes in live cached BGRA8 textures, if prepared.
    /// This excludes driver allocations, in-flight old outputs, and GPUI targets.
    pub fn cached_texture_info(&self) -> Option<(String, u64)> {
        self.cache.texture_info()
    }
}

impl CustomContent for SolidColor {
    fn prepare(&self, backend: &dyn Any, size: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        #[cfg(target_os = "macos")]
        if backend.is::<MetalCustomContext>() {
            return self.prepare_metal(backend, size);
        }
        #[cfg(target_os = "windows")]
        if let Some(context) =
            backend.downcast_ref::<gpui_windows::custom_draw::Dx11CustomContext>()
        {
            let (output, generated) = self.dx11.clear(context, size, self.rgba)?;
            if generated {
                self.preparations.fetch_add(1, Ordering::Relaxed);
            }
            return Ok(Box::new(output));
        }
        #[cfg(feature = "wgpu")]
        if let Some(context) = backend.downcast_ref::<gpui_wgpu::custom_draw::WgpuCustomContext>() {
            let (output, generated) = self.wgpu.clear(context, size, self.rgba)?;
            if generated {
                self.preparations.fetch_add(1, Ordering::Relaxed);
            }
            return Ok(Box::new(output));
        }
        bail!("SolidColor does not support this backend")
    }
}

#[cfg(target_os = "windows")]
impl SolidColor {
    pub fn cached_texture_info(&self) -> Option<(String, u64)> {
        self.dx11.texture_info()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_channels_and_unsupported_backend_are_errors() {
        assert!(SolidColor::new([f32::NAN, 0., 0., 1.]).is_err());
        assert!(SolidColor::new([1.1, 0., 0., 1.]).is_err());
        let solid = SolidColor::new([1., 0., 0., 1.]).unwrap();
        assert!(
            solid
                .prepare(&(), gpui::size(DevicePixels(16), DevicePixels(16)))
                .is_err()
        );
    }
}
