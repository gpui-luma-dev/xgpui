//! Platform-neutral surface: ring ownership, presentation and frame snapshots.
//! Slot memory, sync points and host preparation come from `crate::platform`.
use crate::platform::{self, SlotResources, SyncPoint};
use crate::ring::{Lease, Ring, Writing};
use crate::{AcquireError, SurfaceDesc, SurfaceStats, Transport};
use anyhow::{Result, ensure};
use gpui::{Bounds, Corners, CustomContent, CustomGeometry, DevicePixels, Pixels, Size, Window};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct Shared {
    desc: SurfaceDesc,
    ring: Ring,
    slots: Vec<Arc<SlotResources>>,
    latest: Mutex<Option<Arc<SurfaceFrame>>>,
    on_present: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    invalid: AtomicBool,
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
    pub(crate) resources: Arc<SlotResources>,
    desc: SurfaceDesc,
}

impl SurfaceSlot {
    /// Ring index, stable for the surface's lifetime (useful as a cache key).
    pub fn index(&self) -> usize {
        self.writing.index()
    }
    pub fn desc(&self) -> SurfaceDesc {
        self.desc
    }
}

impl EngineSurface {
    pub(crate) fn from_slots(desc: SurfaceDesc, slots: Vec<Arc<SlotResources>>) -> Result<Self> {
        desc.validate()?;
        ensure!(slots.len() == desc.slots, "slot count mismatch");
        Ok(Self(Arc::new(Shared {
            desc,
            ring: Ring::new(desc.slots),
            slots,
            latest: Mutex::new(None),
            on_present: Mutex::new(None),
            invalid: AtomicBool::new(false),
        })))
    }

    pub fn desc(&self) -> SurfaceDesc {
        self.0.desc
    }

    pub fn transport(&self) -> Transport {
        Transport::ZeroCopy
    }

    /// A writable slot, or `WouldBlock` when every slot is in use.
    ///
    /// When the ring is full, first lets the compositor retire frames whose GPU
    /// sampling already finished (no later frame needs to be drawn, #197 K4).
    pub fn acquire(&self) -> Result<SurfaceSlot, AcquireError> {
        if !self.is_valid() {
            return Err(AcquireError::Invalidated);
        }
        let writing = match self.0.ring.acquire(false) {
            Some(writing) => writing,
            None => {
                // No locks are held here: retiring frames frees slots.
                platform::retire_completed(&self.0.slots);
                self.0.ring.acquire(true).ok_or(AcquireError::WouldBlock)?
            }
        };
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
        ensure!(self.is_valid(), AcquireError::Invalidated);
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

    /// Immutable snapshot of the latest presented frame, for scene placement
    /// in a window whose effective color mode is wide-gamut SDR (macOS and
    /// Windows). Use [`Self::preview_content`] for other windows, or
    /// [`Self::paint`], which chooses for the window.
    pub fn content(&self) -> Option<Arc<dyn CustomContent>> {
        self.content_for(true)
    }

    /// Like [`Self::content`], for a window that is not wide-gamut: RGBA16F
    /// slots are composited as an sRGB preview. BGRA8 slots, and every slot
    /// on Linux, composite the same either way.
    pub fn preview_content(&self) -> Option<Arc<dyn CustomContent>> {
        self.content_for(false)
    }

    fn content_for(&self, wide: bool) -> Option<Arc<dyn CustomContent>> {
        let frame = self.0.latest.lock().unwrap().clone()?;
        Some(Arc::new(FrameView { frame, wide }))
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
        let wide = window.color_status().effective() == gpui::WindowColorMode::WideGamutSdr;
        if let Some(content) = self.content_for(wide) {
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

    /// Mark the surface unusable, typically from the engine's device-lost
    /// callback (#197 K6). Later `acquire`/`present` calls fail with
    /// `AcquireError::Invalidated`. The last presented frame keeps displaying:
    /// the compositor holds its own reference to that frame's shared memory,
    /// so it outlives the engine device. Recover by creating a new engine
    /// device and surface.
    pub fn invalidate(&self) {
        self.0.invalid.store(true, Ordering::Release);
    }

    /// `false` after `invalidate`.
    pub fn is_valid(&self) -> bool {
        !self.0.invalid.load(Ordering::Acquire)
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
            .field("sync", &self.sync)
            .finish()
    }
}

/// A frame placed for a wide or non-wide window. Holding it keeps the
/// frame's slot published.
#[derive(Debug)]
struct FrameView {
    frame: Arc<SurfaceFrame>,
    wide: bool,
}

impl CustomContent for FrameView {
    fn image_source_size(&self) -> Option<Size<DevicePixels>> {
        Some(self.frame.desc.extent())
    }
    fn image_color_mode(&self) -> gpui::ImageColorMode {
        self.frame.desc.format.color_mode(self.wide)
    }
    fn prepare(&self, backend: &dyn Any, _: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let frame = &self.frame;
        platform::prepare_frame(
            &frame.desc,
            &frame.resources,
            &frame.sync,
            self.image_color_mode(),
            backend,
        )
    }
}
