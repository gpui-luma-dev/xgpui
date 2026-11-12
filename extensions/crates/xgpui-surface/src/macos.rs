//! macOS transport: IOSurface slots, `MTLSharedEvent` sync points, zero copy.
//!
//! The engine and the compositor each create their own `MTLTexture` over a
//! slot's IOSurface. The compositor waits GPU-side for the sync point inside a
//! host command buffer that also touches the slot texture, so Metal orders the
//! sampling composition after the wait (issue #198, spike 1).
use crate::{EngineSurface, SurfaceDesc, SurfaceFormat, SurfaceSlot, Unsupported};
use anyhow::{Context as _, Result};
use gpui_apple::custom_draw::{MetalCustomContext, MetalCustomOutput, MetalWideSdrOutput};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight, kIOSurfacePixelFormat,
    kIOSurfaceWidth,
};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandEncoder, MTLDevice, MTLLoadAction, MTLPixelFormat,
    MTLRenderPassDescriptor, MTLSharedEvent, MTLStoreAction, MTLTexture, MTLTextureDescriptor,
    MTLTextureUsage,
};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(feature = "wgpu")]
mod wgpu_target;
#[cfg(feature = "wgpu")]
pub use wgpu_target::WgpuSurfaceTarget;

#[cfg(test)]
mod tests;

/// A GPU timeline point: the engine signals `event` to at least `value` after
/// the slot's writes complete.
#[derive(Clone)]
pub struct SyncPoint {
    event: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    value: u64,
    /// The signal is known to be submitted, so the compositor may wait for it
    /// on the GPU.
    submitted: bool,
    /// Caches "reached" so recompositing never queries the event again.
    seen: Arc<AtomicBool>,
}
// SAFETY: MTLSharedEvent is a thread-safe Metal object; only retain/release,
// signaledValue reads, CPU waits and GPU-side waits are performed.
unsafe impl Send for SyncPoint {}
unsafe impl Sync for SyncPoint {}
impl std::fmt::Debug for SyncPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SyncPoint({}{})",
            self.value,
            if self.submitted { ", submitted" } else { "" }
        )
    }
}
impl SyncPoint {
    /// A point the compositor observes without waiting: the frame is shown
    /// once `event` is seen at `value` or above, and the last ready frame
    /// stays on screen until then. Safe for any engine, including one that
    /// never signals. The event must belong to the compositor's GPU.
    pub fn new(event: Retained<ProtocolObject<dyn MTLSharedEvent>>, value: u64) -> Self {
        Self {
            event,
            value,
            submitted: false,
            seen: Arc::new(AtomicBool::new(false)),
        }
    }
    /// A point the compositor waits for on the GPU, so the frame is shown
    /// without waiting for the engine on the CPU.
    ///
    /// # Safety
    ///
    /// The engine must already have committed work that signals `event` to
    /// `value`, and that work must finish. Otherwise the compositor's queue
    /// waits forever and composition stalls.
    pub unsafe fn submitted(
        event: Retained<ProtocolObject<dyn MTLSharedEvent>>,
        value: u64,
    ) -> Self {
        Self {
            submitted: true,
            ..Self::new(event, value)
        }
    }
    pub fn value(&self) -> u64 {
        self.value
    }
    /// Whether the compositor waits for this point on the GPU.
    pub fn is_gpu_wait(&self) -> bool {
        self.submitted
    }
    pub(crate) fn reached(&self) -> bool {
        if self.seen.load(Ordering::Acquire) {
            return true;
        }
        let reached = self.event.signaledValue() >= self.value;
        if reached {
            self.seen.store(true, Ordering::Release);
        }
        reached
    }
    /// Whether a frame with this point can be composited now.
    pub(crate) fn ready(&self) -> bool {
        self.submitted || self.reached()
    }
    /// Block up to `timeout` for the point (waiter thread only).
    pub(crate) fn wait_reached(&self, timeout: Duration) -> bool {
        self.reached()
            || (self
                .event
                .waitUntilSignaledValue_timeoutMS(self.value, timeout.as_millis() as u64)
                && self.reached())
    }
}

pub(crate) fn metal_pixel_format(format: SurfaceFormat) -> MTLPixelFormat {
    match format {
        SurfaceFormat::Bgra8Srgb => MTLPixelFormat::BGRA8Unorm,
        SurfaceFormat::Rgba16FloatLinearP3 => MTLPixelFormat::RGBA16Float,
    }
}

/// The compositor's texture over a slot, keyed by the GPU's registry ID.
type HostTexture = (u64, Retained<ProtocolObject<dyn MTLTexture>>);

/// One slot's shareable memory plus the compositor's cached view of it.
#[derive(Debug)]
pub(crate) struct SlotResources {
    surface: CFRetained<IOSurfaceRef>,
    host: Mutex<Option<HostTexture>>,
}
// SAFETY: IOSurface is a thread-safe kernel object and Metal textures may be
// retained and released on any thread; mutation is limited to the mutex-guarded
// cache entry.
unsafe impl Send for SlotResources {}
unsafe impl Sync for SlotResources {}

impl SlotResources {
    /// Nothing to release here: only D3D12 devices are per-adapter singletons
    /// that a surviving object keeps lost (see the Windows transport).
    pub(crate) fn release_engine(&self) {}

    fn new(desc: &SurfaceDesc) -> Result<Self> {
        let keys: [&CFString; 4] = unsafe {
            [
                kIOSurfaceWidth,
                kIOSurfaceHeight,
                kIOSurfaceBytesPerElement,
                kIOSurfacePixelFormat,
            ]
        };
        let pixel_format = match desc.format {
            SurfaceFormat::Bgra8Srgb => *b"BGRA",
            SurfaceFormat::Rgba16FloatLinearP3 => *b"RGhA",
        };
        let values = [
            CFNumber::new_i32(desc.width as i32),
            CFNumber::new_i32(desc.height as i32),
            CFNumber::new_i32(desc.format.bytes_per_pixel() as i32),
            CFNumber::new_i32(i32::from_be_bytes(pixel_format)),
        ];
        let values: Vec<&CFNumber> = values.iter().map(|value| &**value).collect();
        let properties = CFDictionary::from_slices(&keys, &values);
        let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }
            .context("IOSurface allocation failed")?;
        Ok(Self {
            surface,
            host: Mutex::new(None),
        })
    }

    /// The compositor's texture over this slot, cached per GPU.
    fn host_texture(
        &self,
        device: &ProtocolObject<dyn MTLDevice>,
        desc: &SurfaceDesc,
    ) -> Result<Retained<ProtocolObject<dyn MTLTexture>>> {
        let registry = device.registryID();
        let mut cached = self.host.lock().unwrap();
        if let Some((id, texture)) = cached.as_ref()
            && *id == registry
        {
            return Ok(texture.clone());
        }
        let texture = surface_texture(
            device,
            &self.surface,
            desc,
            MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget,
        )?;
        *cached = Some((registry, texture.clone()));
        Ok(texture)
    }
}

/// A Metal texture over an IOSurface with the surface's format.
pub(crate) fn surface_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    surface: &IOSurfaceRef,
    desc: &SurfaceDesc,
    usage: MTLTextureUsage,
) -> Result<Retained<ProtocolObject<dyn MTLTexture>>> {
    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            metal_pixel_format(desc.format),
            desc.width as usize,
            desc.height as usize,
            false,
        )
    };
    descriptor.setUsage(usage);
    device
        .newTextureWithDescriptor_iosurface_plane(&descriptor, surface, 0)
        .context("IOSurface-backed Metal texture creation failed")
}

impl EngineSurface {
    /// Allocate the ring of IOSurface slots.
    pub fn new(desc: SurfaceDesc) -> Result<Self> {
        desc.validate()?;
        let slots = (0..desc.slots)
            .map(|_| SlotResources::new(&desc).map(Arc::new))
            .collect::<Result<Vec<_>>>()
            .map_err(|error| Unsupported(format!("{error:#}")))?;
        Self::from_slots(desc, slots)
    }
}

impl SurfaceSlot {
    /// The slot's shareable memory, for the engine to wrap in its own texture.
    pub fn iosurface(&self) -> &IOSurfaceRef {
        &self.resources.surface
    }
}

/// Metal completion handlers retire frames without further frames.
pub(crate) fn retire_completed(_slots: &[Arc<SlotResources>]) {}

pub(crate) fn prepare_frame(
    desc: &SurfaceDesc,
    resources: &SlotResources,
    sync: &SyncPoint,
    color: gpui::ImageColorMode,
    backend: &dyn Any,
) -> Result<Box<dyn Any>> {
    let context = backend
        .downcast_ref::<MetalCustomContext>()
        .ok_or_else(|| Unsupported("engine surfaces require the Metal compositor".into()))?;
    // `MTLSharedEvent.device` can be nil, so GPU identity is not checked
    // here; `SyncPoint::new` documents the same-GPU requirement and xgpui
    // validates the slot texture against the compositor's device.
    let device = context.objc2_device();
    let texture = resources.host_texture(device, desc)?;
    let commands = if sync.reached() {
        // Engine work has completed; nothing to wait for.
        Vec::new()
    } else {
        // Only frames whose signal is known to be submitted are selected
        // before they are reached (`EngineSurface::content`).
        anyhow::ensure!(sync.submitted, "engine frame selected before it was ready");
        let event = sync.event.clone();
        let value = sync.value;
        let target = texture.clone();
        vec![context.record(move |command| {
            command.encodeWaitForEvent_value(ProtocolObject::from_ref(&*event), value);
            // Touch the slot so Metal orders composition after the wait.
            let pass = MTLRenderPassDescriptor::new();
            let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            attachment.setTexture(Some(&target));
            attachment.setLoadAction(MTLLoadAction::Load);
            attachment.setStoreAction(MTLStoreAction::Store);
            command
                .renderCommandEncoderWithDescriptor(&pass)
                .context("Metal render encoder allocation failed")?
                .endEncoding();
            Ok(())
        })?]
    };
    Ok(match (desc.format, color) {
        (SurfaceFormat::Bgra8Srgb, _) => Box::new(MetalCustomOutput { texture, commands }),
        (SurfaceFormat::Rgba16FloatLinearP3, gpui::ImageColorMode::LinearP3Wide) => Box::new(
            MetalWideSdrOutput::from_objc2_linear_p3_premultiplied(context, texture, commands)?,
        ),
        // SAFETY: the slot is on the host device; `commands` wait for the
        // engine's signal before composition, and the slot's lease keeps it
        // unmodified while this frame can be sampled.
        (SurfaceFormat::Rgba16FloatLinearP3, _) => {
            Box::new(unsafe { context.adopt_texture(texture, commands) }?)
        }
    })
}
