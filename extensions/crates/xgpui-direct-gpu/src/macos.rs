//! macOS transport: IOSurface slots, `MTLSharedEvent` sync points, zero copy.
//!
//! The engine and the compositor each create their own `MTLTexture` over a
//! slot's IOSurface. The compositor waits GPU-side for the sync point inside a
//! host command buffer that also touches the slot texture, so Metal orders the
//! sampling composition after the wait (issue #198, spike 1).
use crate::ring::{Lease, Ring, Writing};
use crate::{AcquireError, SurfaceDesc, SurfaceFormat, SurfaceStats, Transport, Unsupported};
use anyhow::{Context as _, Result, ensure};
use gpui::{Bounds, Corners, CustomContent, CustomGeometry, DevicePixels, Pixels, Size, Window};
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
use std::sync::{Arc, Mutex};

#[cfg(feature = "wgpu")]
mod wgpu_target;
#[cfg(feature = "wgpu")]
pub use wgpu_target::WgpuSurfaceTarget;

#[cfg(test)]
mod tests;

/// A GPU timeline point: the engine signals `event` to at least `value` after
/// the slot's writes complete.
#[derive(Clone, Debug)]
pub struct SyncPoint {
    event: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    value: u64,
}
// SAFETY: MTLSharedEvent is a thread-safe Metal object; only retain/release,
// signaledValue reads and GPU-side waits are performed.
unsafe impl Send for SyncPoint {}
unsafe impl Sync for SyncPoint {}
impl SyncPoint {
    /// The engine must already have submitted work that signals `event` to
    /// `value`. The event must belong to the compositor's GPU.
    pub fn new(event: Retained<ProtocolObject<dyn MTLSharedEvent>>, value: u64) -> Self {
        Self { event, value }
    }
    pub fn value(&self) -> u64 {
        self.value
    }
    fn reached(&self) -> bool {
        self.event.signaledValue() >= self.value
    }
}

pub(crate) fn metal_pixel_format(format: SurfaceFormat) -> MTLPixelFormat {
    match format {
        SurfaceFormat::Bgra8Srgb => MTLPixelFormat::BGRA8Unorm,
        SurfaceFormat::Rgba16FloatLinearP3 => MTLPixelFormat::RGBA16Float,
    }
}

/// One slot's shareable memory plus the compositor's cached view of it.
#[derive(Debug)]
pub(crate) struct SlotResources {
    surface: CFRetained<IOSurfaceRef>,
    host: Mutex<Option<(u64, Retained<ProtocolObject<dyn MTLTexture>>)>>,
}
// SAFETY: IOSurface is a thread-safe kernel object and Metal textures may be
// retained and released on any thread; mutation is limited to the mutex-guarded
// cache entry.
unsafe impl Send for SlotResources {}
unsafe impl Sync for SlotResources {}

impl SlotResources {
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

struct Shared {
    desc: SurfaceDesc,
    ring: Ring,
    slots: Vec<Arc<SlotResources>>,
    latest: Mutex<Option<Arc<SurfaceFrame>>>,
    on_present: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// A ring of shareable slots presented into xgpui (see the crate docs).
#[derive(Clone)]
pub struct EngineSurface(Arc<Shared>);

impl std::fmt::Debug for EngineSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineSurface")
            .field("desc", &self.0.desc)
            .field("stats", &self.stats())
            .finish()
    }
}

/// A slot acquired for writing. Drop it to give the slot back unpublished.
pub struct SurfaceSlot {
    writing: Writing,
    resources: Arc<SlotResources>,
    desc: SurfaceDesc,
}

impl SurfaceSlot {
    /// Ring index, stable for the surface's lifetime (useful as a cache key).
    pub fn index(&self) -> usize {
        self.writing.index()
    }
    /// The slot's shareable memory, for the engine to wrap in its own texture.
    pub fn iosurface(&self) -> &IOSurfaceRef {
        &self.resources.surface
    }
    pub fn desc(&self) -> SurfaceDesc {
        self.desc
    }
}

impl EngineSurface {
    /// Allocate the ring. Fails on invalid descriptions or allocation failure.
    pub fn new(desc: SurfaceDesc) -> Result<Self> {
        desc.validate()?;
        let slots = (0..desc.slots)
            .map(|_| SlotResources::new(&desc).map(Arc::new))
            .collect::<Result<Vec<_>>>()
            .map_err(|error| Unsupported(format!("{error:#}")))?;
        Ok(Self(Arc::new(Shared {
            desc,
            ring: Ring::new(desc.slots),
            slots,
            latest: Mutex::new(None),
            on_present: Mutex::new(None),
        })))
    }

    pub fn desc(&self) -> SurfaceDesc {
        self.0.desc
    }

    pub fn transport(&self) -> Transport {
        Transport::ZeroCopy
    }

    /// A writable slot, or `WouldBlock` when every slot is in use.
    pub fn acquire(&self) -> Result<SurfaceSlot, AcquireError> {
        let writing = self.0.ring.acquire().ok_or(AcquireError::WouldBlock)?;
        let resources = self.0.slots[writing.index()].clone();
        Ok(SurfaceSlot {
            writing,
            resources,
            desc: self.0.desc,
        })
    }

    /// Publish `slot` as the latest frame, readable once `sync` is reached.
    /// Call only after submitting the engine work that signals `sync`.
    pub fn present(&self, slot: SurfaceSlot, sync: SyncPoint) -> Result<()> {
        ensure!(
            self.0.ring.owns(&slot.writing),
            "slot belongs to another surface"
        );
        let lease = self.0.ring.publish(slot.writing);
        let frame = Arc::new(SurfaceFrame {
            desc: self.0.desc,
            resources: slot.resources,
            sync,
            _lease: lease,
        });
        // Drop the previous frame outside the lock: its lease may free a slot.
        let previous = self.0.latest.lock().unwrap().replace(frame);
        drop(previous);
        let callback = self.0.on_present.lock().unwrap().clone();
        if let Some(callback) = callback {
            callback();
        }
        Ok(())
    }

    /// Immutable snapshot of the latest presented frame, for scene placement.
    pub fn content(&self) -> Option<Arc<dyn CustomContent>> {
        self.0
            .latest
            .lock()
            .unwrap()
            .clone()
            .map(|frame| frame as Arc<dyn CustomContent>)
    }

    /// Paint the latest frame into `bounds`, scaled from the slot size.
    /// The host clips the frame to the rounded shape, so it matches rounded
    /// containers (GPUI's `overflow_hidden` clips to a rectangle only).
    pub fn paint(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
    ) {
        if let Some(content) = self.content() {
            window.paint_custom(
                CustomGeometry::RoundedRect {
                    bounds,
                    corner_radii,
                },
                content,
            );
        }
    }

    /// Called after every present, on the presenting thread. Typically wakes
    /// the UI to redraw.
    pub fn on_present(&self, callback: impl Fn() + Send + Sync + 'static) {
        *self.0.on_present.lock().unwrap() = Some(Arc::new(callback));
    }

    pub fn stats(&self) -> SurfaceStats {
        self.0.ring.stats()
    }
}

/// One presented frame. Holding it keeps its slot published.
struct SurfaceFrame {
    desc: SurfaceDesc,
    resources: Arc<SlotResources>,
    sync: SyncPoint,
    _lease: Arc<Lease>,
}
impl std::fmt::Debug for SurfaceFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceFrame")
            .field("desc", &self.desc)
            .field("sync", &self.sync.value)
            .finish()
    }
}

impl CustomContent for SurfaceFrame {
    fn image_source_size(&self) -> Option<Size<DevicePixels>> {
        Some(self.desc.extent())
    }
    fn image_color_mode(&self) -> gpui::ImageColorMode {
        self.desc.format.color_mode()
    }
    fn prepare(&self, backend: &dyn Any, _: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let context = backend
            .downcast_ref::<MetalCustomContext>()
            .ok_or_else(|| Unsupported("engine surfaces require the Metal compositor".into()))?;
        // `MTLSharedEvent.device` can be nil, so GPU identity is not checked
        // here; `SyncPoint::new` documents the same-GPU requirement and xgpui
        // validates the slot texture against the compositor's device.
        let device = context.objc2_device();
        let texture = self.resources.host_texture(device, &self.desc)?;
        let commands = if self.sync.reached() {
            // Engine work has completed; nothing to wait for.
            Vec::new()
        } else {
            let event = self.sync.event.clone();
            let value = self.sync.value;
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
        Ok(match self.desc.format {
            SurfaceFormat::Bgra8Srgb => Box::new(MetalCustomOutput { texture, commands }),
            SurfaceFormat::Rgba16FloatLinearP3 => Box::new(
                MetalWideSdrOutput::from_objc2_linear_p3_premultiplied(context, texture, commands)?,
            ),
        })
    }
}
