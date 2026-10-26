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
    outputs: Mutex<Vec<WgpuCustomOutput>>,
}
/// A dedicated immutable DMA-BUF allocation. An import is cached for one host queue.
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
    /// driver. Producer writes and its EXTERNAL/GENERAL ownership release must already
    /// be complete (including any producer fence). Preserve immutable contents
    /// for the allocation's full native lifetime, including imported GPU refs.
    /// No pool-recycle or release-fence signal is provided. Do not publish mutable
    /// decoder buffers or substitute an ordinary file/memfd for a DMA-BUF.
    pub unsafe fn from_bgra(fd: OwnedFd, layout: DmaBufLayout) -> Self {
        Self(Arc::new(Inner {
            fd,
            layout,
            outputs: Mutex::new(Vec::new()),
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
        self.0.outputs.lock().unwrap().len()
    }
}
impl gpui::CustomContent for DmaBufFrame {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        let context = backend
            .downcast_ref::<WgpuCustomContext>()
            .context("DMA-BUF frames require wgpu Vulkan")?;
        #[cfg(target_os = "linux")]
        {
            ensure!(
                context
                    .device
                    .features()
                    .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF),
                "DMA-BUF import capability was not enabled on the host device"
            );
            let mut outputs = self.0.outputs.lock().unwrap();
            if let Some(output) = outputs.iter().find(|output| context.can_reuse(output)) {
                return Ok(Box::new(output.clone()));
            }
            // An abandoned preparation never acquired GPU ownership; discard its
            // cache entry and import anew. Retained outputs preserve queue identity.
            outputs.retain(|output| output.recording.state.load(Ordering::Acquire) != ABANDONED);
            ensure!(
                outputs.is_empty(),
                "DMA-BUF snapshot is already bound to another host preparation/queue"
            );
            let output = import(context, &self.0.fd, self.0.layout)?;
            outputs.push(output.clone());
            Ok(Box::new(output))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = context;
            anyhow::bail!("DMA-BUF snapshot import requires Linux Vulkan")
        }
    }
}

#[cfg(target_os = "linux")]
fn import(
    context: &WgpuCustomContext,
    fd: &OwnedFd,
    layout: DmaBufLayout,
) -> Result<WgpuCustomOutput> {
    use ash::vk;
    use wgpu::hal::api::Vulkan;
    let size = wgpu::Extent3d {
        width: layout.size.width.0 as u32,
        height: layout.size.height.0 as u32,
        depth_or_array_layers: 1,
    };
    ensure!(
        size.width <= context.device.limits().max_texture_dimension_2d
            && size.height <= context.device.limits().max_texture_dimension_2d,
        "DMA-BUF dimensions exceed host device limits"
    );
    let hal_desc = wgpu::hal::TextureDescriptor {
        label: Some("dma_buf_snapshot"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUses::RESOURCE,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: vec![],
    };
    let (hal_texture, raw_device, queue_family) = {
        // SAFETY: only the host device imports this immutable allocation. HAL
        // consumes a duplicate FD on all paths; the frame retains the original.
        let device = unsafe { context.device.as_hal::<Vulkan>() }
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    // GENERAL is represented by STORAGE_READ_ONLY in wgpu's state tracker.
    // The ownership-only barrier below preserves GENERAL; wgpu then performs
    // its normal GENERAL -> shader-read transition at the first sampling use.
    // SAFETY: initialized import from this device, matching descriptor and layout.
    let texture = unsafe {
        context.device.create_texture_from_hal::<Vulkan>(
            hal_texture,
            &desc,
            wgpu::TextureUses::STORAGE_READ_ONLY,
        )
    };
    context.record(|encoder| {
        let native_texture =
            unsafe { texture.as_hal::<Vulkan>() }.context("missing Vulkan DMA-BUF texture")?;
        // SAFETY: producer has completed its matching EXTERNAL release in GENERAL.
        // This records on the host-owned preparation encoder without submitting;
        // the host executes it before sampling. No texture layout is changed here.
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
    })?;
    // Raw HAL barriers do not register resource usage with wgpu's tracker.
    // Retain the imported texture until the preparation submission completes,
    // including when the main render fails before any tracked sampling occurs.
    context
        .recording
        .guards
        .lock()
        .unwrap()
        .push(Arc::new(texture.clone()));
    Ok(WgpuCustomOutput {
        texture,
        source_sized: true,
        key: context.key,
        _queue: context.queue.clone(),
        recording: context.recording.clone(),
    })
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

#[cfg(all(test, target_os = "linux"))]
mod native_tests;
