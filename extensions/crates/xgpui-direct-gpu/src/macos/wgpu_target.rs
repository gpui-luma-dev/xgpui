//! Adapter for wgpu engines running on Metal with their own device and queue.
use super::{SyncPoint, surface_texture};
use crate::{EngineSurface, SurfaceDesc, SurfaceFormat, SurfaceSlot};
use anyhow::{Context as _, Result};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLDevice, MTLSharedEvent, MTLTextureType, MTLTextureUsage};
use wgpu::hal::api::Metal;

/// Wraps surface slots as `wgpu::Texture`s and presents after submission.
///
/// `present` stages a shared-event signal on the engine queue, submits, then
/// publishes the slot, so the contract "present only after the signal is
/// submitted" holds by construction. Staged signals are queue-wide: do not
/// submit on this queue from other threads concurrently with `present`.
pub struct WgpuSurfaceTarget {
    surface: EngineSurface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    event: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    next: u64,
    textures: Vec<Option<wgpu::Texture>>,
}

// SAFETY: wgpu handles are Send + Sync on native targets; MTLSharedEvent is
// thread-safe for retain/release and is only staged on the engine's queue. The
// target is used from one engine thread at a time (`&mut self` methods).
unsafe impl Send for WgpuSurfaceTarget {}

impl WgpuSurfaceTarget {
    /// Create a surface for `desc` and a target that renders into it.
    pub fn new(desc: SurfaceDesc, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self> {
        let surface = EngineSurface::new(desc)?;
        let event = unsafe { device.as_hal::<Metal>() }
            .context("wgpu engine surfaces require the Metal backend")?
            .raw_device()
            .newSharedEvent()
            .context("MTLSharedEvent allocation failed")?;
        Ok(Self {
            surface,
            device: device.clone(),
            queue: queue.clone(),
            event,
            next: 0,
            textures: vec![None; desc.slots],
        })
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        match self.surface.desc().format {
            SurfaceFormat::Bgra8Srgb => wgpu::TextureFormat::Bgra8Unorm,
            SurfaceFormat::Rgba16FloatLinearP3 => wgpu::TextureFormat::Rgba16Float,
        }
    }

    /// The engine's texture over `slot` (cached per slot).
    pub fn texture(&mut self, slot: &SurfaceSlot) -> Result<wgpu::Texture> {
        if let Some(texture) = &self.textures[slot.index()] {
            return Ok(texture.clone());
        }
        let desc = self.surface.desc();
        let raw_device = unsafe { self.device.as_hal::<Metal>() }
            .context("Metal hal device")?
            .raw_device()
            .clone();
        let raw = surface_texture(
            &raw_device,
            slot.iosurface(),
            &desc,
            MTLTextureUsage::RenderTarget
                | MTLTextureUsage::ShaderRead
                | MTLTextureUsage::ShaderWrite,
        )?;
        let size = wgpu::Extent3d {
            width: desc.width,
            height: desc.height,
            depth_or_array_layers: 1,
        };
        let hal = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                raw,
                self.format(),
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width: desc.width,
                    height: desc.height,
                    depth: 1,
                },
                None,
            )
        };
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        if desc.format == SurfaceFormat::Rgba16FloatLinearP3 {
            usage |= wgpu::TextureUsages::STORAGE_BINDING;
        }
        let texture = unsafe {
            self.device.create_texture_from_hal::<Metal>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some("engine_surface_slot"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format(),
                    usage,
                    view_formats: &[],
                },
                wgpu::TextureUses::UNINITIALIZED,
            )
        };
        self.textures[slot.index()] = Some(texture.clone());
        Ok(texture)
    }

    /// Submit `commands`, signal completion, and publish `slot`.
    pub fn present(
        &mut self,
        slot: SurfaceSlot,
        commands: impl IntoIterator<Item = wgpu::CommandBuffer>,
    ) -> Result<()> {
        self.next += 1;
        unsafe { self.queue.as_hal::<Metal>() }
            .context("Metal hal queue")?
            .add_signal_event(self.event.clone(), self.next);
        self.queue.submit(commands);
        self.surface
            .present(slot, SyncPoint::new(self.event.clone(), self.next))
    }

    pub fn surface(&self) -> &EngineSurface {
        &self.surface
    }
}
