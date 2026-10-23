use anyhow::{Result, bail, ensure};
use gpui::{CustomContent, DevicePixels, Size};
#[cfg(target_os = "macos")]
use gpui_apple::custom_draw::{MetalCustomContext, MetalCustomOutput};
#[cfg(target_os = "macos")]
use metal::*;
use std::{
    any::Any,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(target_os = "macos")]
use std::sync::Mutex;

/// Immutable straight-alpha encoded-sRGB color with a per-device/extent cache.
/// Clearing an offscreen attachment is deliberately the smallest GPU producer:
/// it proves integration without mixing shader math into the first milestone.
pub struct SolidColor {
    rgba: [f32; 4],
    #[cfg(target_os = "macos")]
    cache: Mutex<Option<CachedOutput>>,
    #[cfg(feature = "wgpu")]
    wgpu: crate::wgpu::PaintCache,
    preparations: AtomicUsize,
}
#[cfg(target_os = "macos")]
struct CachedOutput {
    key: u64,
    extent: Size<DevicePixels>,
    texture: Texture,
    command: CommandBuffer,
    // Keep cache identity alive even after command completion.
    _queue: CommandQueue,
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
            cache: Mutex::new(None),
            #[cfg(feature = "wgpu")]
            wgpu: crate::wgpu::PaintCache::default(),
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
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| anyhow::anyhow!("solid cache poisoned"))?;
        let key = context.cache_key();
        if let Some(cached) = cache.as_ref()
            && cached.key == key
            && cached.extent == size
        {
            let commands = match cached.command.status() {
                MTLCommandBufferStatus::NotEnqueued => vec![cached.command.clone()],
                MTLCommandBufferStatus::Error => {
                    bail!("cached solid preparation failed on the GPU")
                }
                MTLCommandBufferStatus::Enqueued => {
                    bail!("producer command was enqueued outside the host")
                }
                _ => vec![],
            };
            return Ok(Box::new(MetalCustomOutput {
                texture: cached.texture.clone(),
                commands,
            }));
        }
        ensure!(size.width.0 > 0 && size.height.0 > 0, "empty solid texture");
        let descriptor = TextureDescriptor::new();
        descriptor.set_texture_type(MTLTextureType::D2);
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        descriptor.set_width(size.width.0 as u64);
        descriptor.set_height(size.height.0 as u64);
        descriptor.set_storage_mode(MTLStorageMode::Private);
        descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        let texture = context.device.new_texture(&descriptor);
        let pass = RenderPassDescriptor::new();
        let attachment = pass.color_attachments().object_at(0).unwrap();
        attachment.set_texture(Some(&texture));
        attachment.set_load_action(MTLLoadAction::Clear);
        attachment.set_store_action(MTLStoreAction::Store);
        let c = self.rgba;
        attachment.set_clear_color(MTLClearColor::new(
            c[0] as f64,
            c[1] as f64,
            c[2] as f64,
            c[3] as f64,
        ));
        let command = context.queue.new_command_buffer().to_owned();
        command.new_render_command_encoder(pass).end_encoding();
        // Host submits before using cached output on its next frame.
        *cache = Some(CachedOutput {
            key,
            extent: size,
            texture: texture.clone(),
            command: command.clone(),
            _queue: context.queue.clone(),
        });
        self.preparations.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(MetalCustomOutput {
            texture,
            commands: vec![command],
        }))
    }
}

impl SolidColor {
    /// Device name and bytes in the current cached BGRA8 texture, if prepared.
    /// This excludes driver allocations, in-flight old outputs, and GPUI targets.
    pub fn cached_texture_info(&self) -> Option<(String, u64)> {
        let cache = self.cache.lock().ok()?;
        let texture = &cache.as_ref()?.texture;
        Some((
            texture.device().name().to_string(),
            texture.width() * texture.height() * 4,
        ))
    }
}

impl CustomContent for SolidColor {
    fn prepare(&self, backend: &dyn Any, size: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        #[cfg(target_os = "macos")]
        if backend.is::<MetalCustomContext>() {
            return self.prepare_metal(backend, size);
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
