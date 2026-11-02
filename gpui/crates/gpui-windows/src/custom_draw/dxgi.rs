//! Portable immutable BGRA snapshots with nonblocking keyed NT-handle acquisition.
use super::*;
use std::os::windows::io::{AsRawHandle, BorrowedHandle};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_ABANDONED, WAIT_TIMEOUT},
    Graphics::Dxgi::IDXGIKeyedMutex,
};

#[derive(Debug)]
struct KeyedLease {
    mutex: IDXGIKeyedMutex,
    release_key: u64,
}
impl Drop for KeyedLease {
    fn drop(&mut self) {
        // All portable GPU uses have completed or been discarded, and all
        // snapshot/cache owners have retired. ReleaseSync supplies the handoff.
        if let Err(error) = unsafe { self.mutex.ReleaseSync(self.release_key) } {
            log::error!("DXGI snapshot ownership release failed: {error}");
        }
    }
}

/// A dedicated BGRA8 texture snapshot, imported directly by the portable D3D11 executor.
/// Supports one shader-readable default-usage 2D surface. Keyed NT-handle imports
/// retain exclusive read ownership until all frame/output clones are dropped.
/// Array slices and YUV planes are not supported.
#[derive(Clone, Debug)]
pub struct DxgiFrame {
    texture: ID3D11Texture2D,
    extent: Size<DevicePixels>,
    lease: Option<Arc<KeyedLease>>,
}
impl DxgiFrame {
    /// Retain an opaque or straight-alpha BGRA8 surface without copying pixels.
    ///
    /// # Safety
    /// Producer writes must be complete and visible to the renderer before use.
    /// The texture must remain immutable for its entire native resource lifetime,
    /// including references retained by D3D11 commands after this frame is dropped.
    /// Do not recycle a decoder/pool allocation when the Rust frame is dropped.
    /// Mutable shared surfaces must use `try_from_shared_bgra` and its keyed protocol.
    pub unsafe fn from_bgra(texture: ID3D11Texture2D) -> Result<Self> {
        Self::validate(texture, false)
    }
    fn validate(texture: ID3D11Texture2D, keyed: bool) -> Result<Self> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        ensure!(
            desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM,
            "DXGI frame requires BGRA8 UNORM"
        );
        ensure!(
            desc.Width > 0 && desc.Height > 0 && desc.Width <= 16384 && desc.Height <= 16384,
            "video dimensions must be 1..16384"
        );
        ensure!(
            desc.ArraySize == 1 && desc.MipLevels == 1 && desc.SampleDesc.Count == 1,
            "DXGI frame requires one non-multisampled mip and array slice"
        );
        let required = D3D11_BIND_SHADER_RESOURCE.0 as u32;
        ensure!(
            desc.BindFlags & required == required && desc.Usage == D3D11_USAGE_DEFAULT,
            "DXGI frame requires a default-usage shader-readable texture"
        );
        ensure!(
            keyed || desc.MiscFlags & (D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0 as u32) == 0,
            "keyed-mutex surfaces require an explicit synchronization adapter"
        );
        Ok(Self {
            texture,
            lease: None,
            extent: size(
                DevicePixels(desc.Width as i32),
                DevicePixels(desc.Height as i32),
            ),
        })
    }
    /// Open a shared NT handle on the host's D3D11 device and try to acquire its
    /// ready key without waiting. `None` means busy: retry outside rendering and
    /// keep displaying the previous snapshot. Abandoned ownership is an error.
    /// The handle is borrowed only during open; the returned texture owns its COM
    /// reference. This is not a legacy DXGI shared handle.
    ///
    /// # Safety
    /// `device` must be the renderer's device. All other users must obey the same
    /// keyed-mutex protocol, write opaque/straight-alpha BGRA, and release with
    /// `ready_key` after production. They may reuse pixels only after successfully
    /// acquiring `release_key`. Do not release this lease through raw COM aliases.
    /// Keep one snapshot per shared allocation; cloning shares the same lease.
    pub unsafe fn try_from_shared_bgra(
        device: &ID3D11Device,
        handle: BorrowedHandle<'_>,
        ready_key: u64,
        release_key: u64,
    ) -> Result<Option<Self>> {
        ensure!(
            ready_key != release_key,
            "DXGI producer and consumer keys must differ"
        );
        let device: ID3D11Device1 = device.cast()?;
        let texture: ID3D11Texture2D =
            unsafe { device.OpenSharedResource1(HANDLE(handle.as_raw_handle()))? };
        let mut frame = Self::validate(texture, true)?;
        let mutex: IDXGIKeyedMutex = frame
            .texture
            .cast()
            .context("shared BGRA surface must provide a keyed mutex")?;
        // The generated Result wrapper accepts positive WAIT_* HRESULTs. Check
        // the raw status: only S_OK grants ownership (Microsoft's DXGI contract).
        let status =
            unsafe { (Interface::vtable(&mutex).AcquireSync)(mutex.as_raw(), ready_key, 0) };
        match status.0 {
            0 => {}
            code if code == WAIT_TIMEOUT.0 as i32 => return Ok(None),
            code if code == WAIT_ABANDONED.0 as i32 => {
                anyhow::bail!("DXGI shared surface abandoned; recreate the surface")
            }
            _ => anyhow::bail!("DXGI keyed acquisition failed: {status:?}"),
        }
        frame.lease = Some(Arc::new(KeyedLease { mutex, release_key }));
        Ok(Some(frame))
    }
    pub fn size(&self) -> Size<DevicePixels> {
        self.extent
    }
}
impl DxgiFrame {
    fn external_descriptor(
        &self,
    ) -> std::result::Result<gpui::content::ImageDescriptor, gpui::content::ContentError> {
        use gpui::content::*;
        ImageDescriptor::new(
            ImageExtent::try_from(self.size())?,
            PixelFormat::Bgra8,
            ImageInterpretation {
                color: ColorEncoding::Srgb,
                alpha: AlphaMode::Straight,
            },
            1,
        )
    }
    /// Portable immutable frame. Already-acquired keyed snapshots retain their
    /// read lease through every graph, presentation and GPU completion owner.
    pub fn external_image(
        &self,
    ) -> std::result::Result<gpui::content::ExternalImage, gpui::content::ContentError> {
        Ok(gpui::content::ExternalImage::ready(
            self.external_descriptor()?,
            Arc::new(self.clone()),
            || {},
        ))
    }
    pub(crate) fn import_external(
        &self,
        device: &ID3D11Device,
        descriptor: &gpui::content::ImageDescriptor,
    ) -> std::result::Result<ID3D11Texture2D, gpui::content::ContentError> {
        use gpui::content::*;
        let owner = unsafe { self.texture.GetDevice() }
            .map_err(|e| ContentError::new(ContentErrorKind::ExecutionFailure, e.to_string()))?;
        if owner.as_raw() != device.as_raw() {
            return Err(ContentError::new(
                ContentErrorKind::UnsupportedCapability,
                "DXGI frame belongs to another device; open the shared handle on the consumer device",
            ));
        }
        let expected = self.external_descriptor()?;
        if descriptor != &expected {
            return Err(ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "external descriptor differs from DXGI BGRA storage",
            ));
        }
        Ok(self.texture.clone())
    }
}
