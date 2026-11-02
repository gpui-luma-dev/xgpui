//! Dedicated, immutable single-plane linear BGRA DMA-BUF snapshots.
use super::*;
use anyhow::Context as _;
use std::os::fd::OwnedFd;

/// Explicit byte layout for a DRM_FORMAT_ARGB8888 buffer (BGRA in little endian).
/// Tiled modifiers, multi-plane/YUV buffers and premultiplied alpha are excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaBufLayout {
    size: Size<DevicePixels>,
    stride: u64,
    offset: u64,
}
impl DmaBufLayout {
    pub fn linear_bgra(
        size: Size<DevicePixels>,
        stride: u64,
        offset: u64,
        modifier: u64,
    ) -> Result<Self> {
        ensure!(
            cfg!(target_endian = "little"),
            "DMA-BUF BGRA requires little-endian storage"
        );
        ensure!(
            modifier == 0,
            "DMA-BUF snapshot requires explicit DRM_FORMAT_MOD_LINEAR"
        );
        ensure!(
            size.width.0 > 0
                && size.height.0 > 0
                && size.width.0 <= 16384
                && size.height.0 <= 16384,
            "video dimensions must be 1..16384"
        );
        ensure!(
            stride >= size.width.0 as u64 * 4
                && stride.is_multiple_of(4)
                && offset.is_multiple_of(4),
            "DMA-BUF stride/offset must describe aligned BGRA rows"
        );
        let end = stride
            .checked_mul(size.height.0 as u64)
            .and_then(|v| offset.checked_add(v))
            .context("DMA-BUF layout overflow")?;
        ensure!(end <= 256 * 1024 * 1024, "DMA-BUF layout exceeds 256 MiB");
        Ok(Self {
            size,
            stride,
            offset,
        })
    }
    pub fn size(&self) -> Size<DevicePixels> {
        self.size
    }
    pub fn stride(&self) -> u64 {
        self.stride
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
}
struct Inner {
    fd: OwnedFd,
    layout: DmaBufLayout,
    imported: Mutex<Option<Imported>>,
}
#[cfg(target_os = "linux")]
struct Imported {
    texture: wgpu::Texture,
    device: std::sync::Weak<wgpu::Device>,
    queue: std::sync::Weak<wgpu::Queue>,
}
#[cfg(not(target_os = "linux"))]
struct Imported;
/// A dedicated immutable DMA-BUF allocation, bound to one portable host queue.
#[derive(Clone)]
pub struct DmaBufFrame(Arc<Inner>);
impl std::fmt::Debug for DmaBufFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DmaBufFrame")
            .field("layout", &self.0.layout)
            .field("fd", &self.0.fd)
            .finish_non_exhaustive()
    }
}
impl DmaBufFrame {
    /// Take ownership of a synchronized immutable DRM_FORMAT_ARGB8888 allocation.
    /// This initial adapter supports Vulkan exporters released to EXTERNAL in
    /// GENERAL layout; foreign-family GBM/EGL exports are not supported.
    ///
    /// # Safety
    /// `fd` must be a valid compatible DMA-BUF with exactly this layout, fully
    /// initialized opaque/straight-alpha pixels, and enough backing storage.
    /// The exporter must use the same underlying physical device and compatible
    /// driver, with compatible SAMPLED and TRANSFER_SRC image usage. Producer writes and its EXTERNAL/GENERAL ownership release must already
    /// be complete (including any producer fence). Preserve immutable contents
    /// for the allocation's full native lifetime, including imported GPU refs.
    /// No pool-recycle or release-fence signal is provided. Do not publish mutable
    /// decoder buffers or substitute an ordinary file/memfd for a DMA-BUF.
    pub unsafe fn from_bgra(fd: OwnedFd, layout: DmaBufLayout) -> Self {
        Self(Arc::new(Inner {
            fd,
            layout,
            imported: Mutex::new(None),
        }))
    }
    /// Basic host capability only; the driver still validates the specific buffer.
    pub fn supported_by(context: &WgpuCustomContext) -> bool {
        #[cfg(target_os = "linux")]
        {
            context
                .device
                .features()
                .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF)
                && unsafe { context.device.as_hal::<wgpu::hal::api::Vulkan>() }.is_some()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = context;
            false
        }
    }
    pub fn size(&self) -> Size<DevicePixels> {
        self.0.layout.size
    }
    pub fn import_count(&self) -> usize {
        usize::from(self.0.imported.lock().unwrap().is_some())
    }
}
impl DmaBufFrame {
    fn descriptor(
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
    pub fn external_image(
        &self,
    ) -> std::result::Result<gpui::content::ExternalImage, gpui::content::ContentError> {
        Ok(gpui::content::ExternalImage::ready(
            self.descriptor()?,
            Arc::new(self.clone()),
            || {},
        ))
    }
    pub(crate) fn validate_external(
        &self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        descriptor: &gpui::content::ImageDescriptor,
    ) -> std::result::Result<(), gpui::content::ContentError> {
        use gpui::content::{ContentError, ContentErrorKind};
        if &self.descriptor()? != descriptor {
            return Err(ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "DMA-BUF descriptor differs from native layout",
            ));
        }
        #[cfg(target_os = "linux")]
        {
            if !device
                .features()
                .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF)
                || unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }.is_none()
            {
                return Err(ContentError::new(
                    ContentErrorKind::UnsupportedCapability,
                    "DMA-BUF requires enabled Vulkan external memory support",
                ));
            }
            if let Some(imported) = self.0.imported.lock().unwrap().as_ref() {
                if !std::sync::Weak::ptr_eq(&Arc::downgrade(device), &imported.device)
                    || !std::sync::Weak::ptr_eq(&Arc::downgrade(queue), &imported.queue)
                {
                    return Err(ContentError::new(
                        ContentErrorKind::UnsupportedCapability,
                        "DMA-BUF snapshot is bound to another device/queue",
                    ));
                }
            }
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (device, queue);
            Err(ContentError::new(
                ContentErrorKind::UnsupportedCapability,
                "DMA-BUF requires Linux Vulkan",
            ))
        }
    }
    pub(crate) fn import_external(
        &self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        descriptor: &gpui::content::ImageDescriptor,
        lease: &gpui::content::ExternalImageLease,
    ) -> std::result::Result<wgpu::Texture, gpui::content::ContentError> {
        self.validate_external(device, queue, descriptor)?;
        #[cfg(target_os = "linux")]
        {
            let mut cached = self.0.imported.lock().unwrap();
            if let Some(imported) = cached.as_ref() {
                // Recheck while holding the cache lock: concurrent executors may race.
                if !std::sync::Weak::ptr_eq(&Arc::downgrade(device), &imported.device)
                    || !std::sync::Weak::ptr_eq(&Arc::downgrade(queue), &imported.queue)
                {
                    return Err(gpui::content::ContentError::new(
                        gpui::content::ContentErrorKind::UnsupportedCapability,
                        "DMA-BUF snapshot is bound to another device/queue",
                    ));
                }
                return Ok(imported.texture.clone());
            }
            let texture =
                import(device, queue, &self.0.fd, self.0.layout, lease.clone()).map_err(|e| {
                    gpui::content::ContentError::new(
                        gpui::content::ContentErrorKind::ExecutionFailure,
                        e.to_string(),
                    )
                })?;
            *cached = Some(Imported {
                texture: texture.clone(),
                device: Arc::downgrade(device),
                queue: Arc::downgrade(queue),
            });
            Ok(texture)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = lease;
            unreachable!("validation rejects non-Linux imports")
        }
    }
}

#[cfg(target_os = "linux")]
fn import(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    fd: &OwnedFd,
    layout: DmaBufLayout,
    lease: gpui::content::ExternalImageLease,
) -> Result<wgpu::Texture> {
    use ash::vk;
    use wgpu::hal::api::Vulkan;
    let size = wgpu::Extent3d {
        width: layout.size.width.0 as u32,
        height: layout.size.height.0 as u32,
        depth_or_array_layers: 1,
    };
    ensure!(
        size.width <= device.limits().max_texture_dimension_2d
            && size.height <= device.limits().max_texture_dimension_2d,
        "DMA-BUF dimensions exceed host device limits"
    );
    let hal_desc = wgpu::hal::TextureDescriptor {
        label: Some("dma_buf_snapshot"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUses::RESOURCE | wgpu::TextureUses::COPY_SRC,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: vec![],
    };
    let (hal_texture, raw_device, queue_family) = {
        // SAFETY: only the host device imports this immutable allocation. HAL
        // consumes a duplicate FD on all paths; the frame retains the original.
        let device = unsafe { device.as_hal::<Vulkan>() }
            .context("DMA-BUF import requires Vulkan, not GLES")?;
        let texture = unsafe {
            device.texture_from_dmabuf_fd(
                fd.try_clone()?,
                &hal_desc,
                0,
                layout.stride,
                layout.offset,
            )
        }
        .map_err(|error| anyhow::anyhow!("DMA-BUF native import failed: {error:?}"))?;
        (
            texture,
            device.raw_device().clone(),
            device.queue_family_index(),
        )
    };
    let desc = wgpu::TextureDescriptor {
        label: Some("dma_buf_snapshot"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    // GENERAL is represented by STORAGE_READ_ONLY in wgpu's state tracker.
    // The ownership-only barrier below preserves GENERAL; wgpu then performs
    // its normal GENERAL -> shader-read transition at the first sampling use.
    // SAFETY: initialized import from this device, matching descriptor and layout.
    let texture = unsafe {
        device.create_texture_from_hal::<Vulkan>(
            hal_texture,
            &desc,
            wgpu::TextureUses::STORAGE_READ_ONLY,
        )
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("DMA-BUF ownership acquire"),
    });
    (|| -> Result<()> {
        let native_texture =
            unsafe { texture.as_hal::<Vulkan>() }.context("missing Vulkan DMA-BUF texture")?;
        // SAFETY: producer has completed its matching EXTERNAL release in GENERAL.
        // The worker submits this acquire before graph work on the same queue. No texture layout is changed here.
        unsafe {
            encoder.as_hal_mut::<Vulkan, _, _>(|encoder| -> Result<()> {
                let encoder = encoder.context("missing Vulkan preparation encoder")?;
                let barrier = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::GENERAL)
                    .new_layout(vk::ImageLayout::GENERAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                    .dst_queue_family_index(queue_family)
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                    .image(native_texture.raw_handle())
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    });
                raw_device.cmd_pipeline_barrier(
                    encoder.raw_handle(),
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
                Ok(())
            })
        }
    })()?;
    // Raw barriers do not register tracked usage. Retain the allocation through
    // this submission even if a later graph operation fails before sampling.
    queue.submit([encoder.finish()]);
    let retained = texture.clone();
    queue.on_submitted_work_done(move || drop((retained, lease)));
    Ok(texture)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bgra_layout_accepts_padding_and_offset() -> Result<()> {
        let size = size(DevicePixels(320), DevicePixels(180));
        assert_eq!(DmaBufLayout::linear_bgra(size, 1344, 4096, 0)?.size(), size);
        Ok(())
    }
    #[test]
    fn bgra_layout_rejects_unsupported_and_unbounded_payloads() {
        let normal = size(DevicePixels(320), DevicePixels(180));
        for (dimensions, stride, offset, modifier) in [
            (size(DevicePixels(0), DevicePixels(180)), 1280, 0, 0),
            (size(DevicePixels(16385), DevicePixels(1)), 65540, 0, 0),
            (normal, 1276, 0, 0),
            (normal, 1281, 0, 0),
            (normal, 1280, 1, 0),
            (normal, 1280, 0, u64::MAX),
            (normal, u64::MAX - 3, 0, 0),
            (normal, 1280, u64::MAX - 3, 0),
            (normal, 1280, 256 * 1024 * 1024, 0),
        ] {
            assert!(DmaBufLayout::linear_bgra(dimensions, stride, offset, modifier).is_err());
        }
    }
}

#[cfg(all(feature = "test-support", target_os = "linux"))]
#[doc(hidden)]
#[path = "dma_buf/native_tests.rs"]
pub mod native_validation;
