//! Experimental: present output from an independently owned GPU engine into
//! xgpui, like presenting to a swapchain (design: issue #197).
//!
//! The engine keeps its own device, queue and thread. An [`EngineSurface`] owns
//! a fixed ring of shareable slots:
//!
//! 1. `acquire()` a writable slot, or get [`AcquireError::WouldBlock`] when
//!    every slot is still in use (bounded backpressure).
//! 2. Render into the slot on the engine's GPU and submit a signal of a sync
//!    point after that work.
//! 3. `present(slot, sync)` publishes the slot. It is never written again
//!    until every retained scene that sampled it is gone and the compositor's
//!    GPU sampling has completed; then it returns to the ring.
//!
//! The host composites the latest presented slot as ordinary custom content,
//! after a GPU-side wait on the sync point. Nothing is copied through the CPU.
//!
//! Contract: call `present` only after the engine has *submitted* the work that
//! signals the sync point. A sync point whose signal is never submitted can
//! stall composition.
//!
//! Implemented: macOS (IOSurface slots, `MTLSharedEvent` sync, GPU-side wait)
//! and Linux with feature `wgpu` (opaque-fd Vulkan memory from a wgpu engine on
//! its own device; readiness observed by the engine before `present`).
use gpui::{DevicePixels, Size, size};

mod ring;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(all(target_os = "linux", feature = "wgpu"))]
mod linux;
#[cfg(all(target_os = "linux", feature = "wgpu"))]
use linux as platform;
#[cfg(all(target_os = "linux", feature = "wgpu"))]
pub use linux::*;

#[cfg(any(target_os = "macos", all(target_os = "linux", feature = "wgpu")))]
mod surface;
#[cfg(any(target_os = "macos", all(target_os = "linux", feature = "wgpu")))]
pub use surface::{EngineSurface, SurfaceSlot};

/// The wgpu version engine adapters are built against; use it for engine devices.
#[cfg(all(target_os = "macos", feature = "wgpu"))]
pub use ::wgpu;
/// The wgpu version engine adapters are built against; use it for engine devices.
#[cfg(all(target_os = "linux", feature = "wgpu"))]
pub use gpui_wgpu::wgpu;

/// Pixel storage and color interpretation of every slot in a surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceFormat {
    /// BGRA8, straight alpha, sRGB-encoded channels. Works in every window.
    Bgra8Srgb,
    /// RGBA16 float, associated alpha, linear Display P3/D65. On macOS this
    /// requires a window that negotiated `WindowColorMode::WideGamutSdr`; on
    /// Linux it is shown as a mapped sRGB preview (#146).
    Rgba16FloatLinearP3,
}
impl SurfaceFormat {
    /// Bytes per pixel.
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Bgra8Srgb => 4,
            Self::Rgba16FloatLinearP3 => 8,
        }
    }
    /// The color interpretation the host applies when compositing.
    pub fn color_mode(self) -> gpui::ImageColorMode {
        match self {
            Self::Bgra8Srgb => gpui::ImageColorMode::EncodedSrgb,
            #[cfg(target_os = "macos")]
            Self::Rgba16FloatLinearP3 => gpui::ImageColorMode::LinearP3Wide,
            #[cfg(not(target_os = "macos"))]
            Self::Rgba16FloatLinearP3 => gpui::ImageColorMode::LinearP3Preview,
        }
    }
}

/// Fixed properties of a surface's slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDesc {
    pub width: u32,
    pub height: u32,
    pub format: SurfaceFormat,
    /// Ring length: 2..=8. Use at least 3 for continuous animation.
    pub slots: usize,
}
impl SurfaceDesc {
    /// Validate dimensions, ring length and the per-surface memory budget.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=16384).contains(&self.width) && (1..=16384).contains(&self.height),
            "surface dimensions must be 1..=16384"
        );
        anyhow::ensure!(
            (2..=8).contains(&self.slots),
            "surface ring must hold 2..=8 slots"
        );
        let bytes = self.width as u64
            * self.height as u64
            * self.format.bytes_per_pixel() as u64
            * self.slots as u64;
        anyhow::ensure!(bytes <= 512 * 1024 * 1024, "surface ring exceeds 512 MiB");
        Ok(())
    }
    /// Slot extent in physical pixels.
    pub fn extent(&self) -> Size<DevicePixels> {
        size(
            DevicePixels(self.width as i32),
            DevicePixels(self.height as i32),
        )
    }
}

/// Why `acquire` returned no slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcquireError {
    /// Every slot is being written, or still referenced by retained scenes or
    /// in-flight composition. Retry after the next presented frame.
    WouldBlock,
}
impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WouldBlock => f.write_str("every surface slot is in use"),
        }
    }
}
impl std::error::Error for AcquireError {}

/// Returned when the platform or configuration cannot support a surface.
#[derive(Debug)]
pub struct Unsupported(pub String);
impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "engine surface unsupported: {}", self.0)
    }
}
impl std::error::Error for Unsupported {}

/// Counters for diagnostics and pressure tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SurfaceStats {
    /// Slots available to `acquire`.
    pub free: usize,
    /// Slots acquired and not yet presented or dropped.
    pub writing: usize,
    /// Slots presented and still referenced (latest frame, retained scenes or
    /// in-flight composition).
    pub published: usize,
    /// Total successful presents.
    pub presents: u64,
    /// Total `acquire` calls that returned `WouldBlock`.
    pub would_block: u64,
}

/// How engine output reaches the compositor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    /// The compositor samples the engine's memory directly.
    ZeroCopy,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_descriptions() {
        let ok = SurfaceDesc {
            width: 64,
            height: 32,
            format: SurfaceFormat::Rgba16FloatLinearP3,
            slots: 3,
        };
        assert!(ok.validate().is_ok());
        for bad in [
            SurfaceDesc { width: 0, ..ok },
            SurfaceDesc {
                height: 16385,
                ..ok
            },
            SurfaceDesc { slots: 1, ..ok },
            SurfaceDesc { slots: 9, ..ok },
            SurfaceDesc {
                width: 16384,
                height: 16384,
                ..ok
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }
}
