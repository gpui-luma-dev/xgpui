//! Linux transport: opaque-fd Vulkan memory, CPU-observed readiness.
//!
//! The engine is a wgpu device of its own (Vulkan). `WgpuSurfaceTarget` creates
//! one exportable image per slot on the engine device and exports its memory as
//! an opaque fd. The compositor imports each slot once per host device and
//! adopts it through `WgpuCustomContext::adopt_texture`.
//!
//! Each frame the engine transitions the slot to shader-read, releases it to
//! `VK_QUEUE_FAMILY_EXTERNAL`, waits for completion on its own thread, then
//! presents; the host acquires it from `VK_QUEUE_FAMILY_EXTERNAL` in a raw-only
//! preparation encoder before sampling (issue #198, spike 2). A GPU-side wait
//! on an imported timeline semaphore needs host device extensions (#197 K1).
use crate::{EngineSurface, SurfaceDesc, SurfaceFormat, SurfaceSlot, Unsupported};
use anyhow::{Context as _, Result, bail, ensure};
use ash::vk;
use gpui_wgpu::custom_draw::WgpuCustomContext;
use gpui_wgpu::wgpu;
use gpui_wgpu::wgpu::hal::api::Vulkan;
use std::any::Any;
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(test)]
mod tests;

const HANDLE: vk::ExternalMemoryHandleTypeFlags = vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD;

/// Readiness of a presented slot. On Linux the engine observes completion of
/// its writes before presenting, so the compositor never waits.
#[derive(Clone, Debug)]
pub struct SyncPoint(());
impl SyncPoint {
    /// The slot's writes have completed on the engine's GPU queue.
    pub fn completed() -> Self {
        Self(())
    }
}

fn vk_format(format: SurfaceFormat) -> vk::Format {
    match format {
        SurfaceFormat::Bgra8Srgb => vk::Format::B8G8R8A8_UNORM,
        SurfaceFormat::Rgba16FloatLinearP3 => vk::Format::R16G16B16A16_SFLOAT,
    }
}
fn wgpu_format(format: SurfaceFormat) -> wgpu::TextureFormat {
    match format {
        SurfaceFormat::Bgra8Srgb => wgpu::TextureFormat::Bgra8Unorm,
        SurfaceFormat::Rgba16FloatLinearP3 => wgpu::TextureFormat::Rgba16Float,
    }
}
// Engine and host images must be created identically for opaque-fd import.
fn vk_usage(format: SurfaceFormat) -> vk::ImageUsageFlags {
    let usage = vk::ImageUsageFlags::SAMPLED
        | vk::ImageUsageFlags::COLOR_ATTACHMENT
        | vk::ImageUsageFlags::TRANSFER_SRC
        | vk::ImageUsageFlags::TRANSFER_DST;
    match format {
        SurfaceFormat::Bgra8Srgb => usage,
        SurfaceFormat::Rgba16FloatLinearP3 => usage | vk::ImageUsageFlags::STORAGE,
    }
}
fn wgpu_usage(format: SurfaceFormat) -> wgpu::TextureUsages {
    let usage = wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::COPY_SRC
        | wgpu::TextureUsages::COPY_DST;
    match format {
        SurfaceFormat::Bgra8Srgb => usage,
        SurfaceFormat::Rgba16FloatLinearP3 => usage | wgpu::TextureUsages::STORAGE_BINDING,
    }
}
fn hal_usage(format: SurfaceFormat) -> wgpu::TextureUses {
    let usage = wgpu::TextureUses::RESOURCE
        | wgpu::TextureUses::COLOR_TARGET
        | wgpu::TextureUses::COPY_SRC
        | wgpu::TextureUses::COPY_DST;
    match format {
        SurfaceFormat::Bgra8Srgb => usage,
        SurfaceFormat::Rgba16FloatLinearP3 => usage | wgpu::TextureUses::STORAGE_READ_WRITE,
    }
}
fn extent(desc: &SurfaceDesc) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: desc.width,
        height: desc.height,
        depth_or_array_layers: 1,
    }
}
fn hal_desc(desc: &SurfaceDesc) -> wgpu::hal::TextureDescriptor<'static> {
    wgpu::hal::TextureDescriptor {
        label: Some("engine_surface_slot"),
        size: extent(desc),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu_format(desc.format),
        usage: hal_usage(desc.format),
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: vec![],
    }
}
fn wgpu_desc(desc: &SurfaceDesc) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("engine_surface_slot"),
        size: extent(desc),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu_format(desc.format),
        usage: wgpu_usage(desc.format),
        view_formats: &[],
    }
}
fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

unsafe fn create_image(raw: &ash::Device, desc: &SurfaceDesc) -> Result<vk::Image> {
    let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(HANDLE);
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(vk_format(desc.format))
        .extent(vk::Extent3D {
            width: desc.width,
            height: desc.height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk_usage(desc.format))
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .push_next(&mut external);
    unsafe { raw.create_image(&info, None) }.context("external image creation failed")
}

/// One slot's exported memory plus the compositor's import of it.
pub(crate) struct SlotResources {
    fd: OwnedFd,
    allocation_size: u64,
    memory_type: u32,
    host: Mutex<Option<HostImport>>,
}
struct HostImport {
    key: usize,
    texture: wgpu::Texture,
    raw: ash::Device,
    family: u32,
    image: vk::Image,
}
impl std::fmt::Debug for SlotResources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlotResources")
            .field("allocation_size", &self.allocation_size)
            .field("memory_type", &self.memory_type)
            .finish_non_exhaustive()
    }
}

impl SlotResources {
    /// The compositor's texture over this slot, imported once per host queue.
    fn host_import(
        &self,
        context: &WgpuCustomContext,
        desc: &SurfaceDesc,
    ) -> Result<(wgpu::Texture, ash::Device, u32, vk::Image)> {
        let key = context.cache_key();
        let mut host = self.host.lock().unwrap();
        if let Some(import) = host.as_ref().filter(|import| import.key == key) {
            return Ok((
                import.texture.clone(),
                import.raw.clone(),
                import.family,
                import.image,
            ));
        }
        ensure!(
            context
                .device
                .features()
                .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_FD),
            Unsupported(
                "the compositor device lacks VULKAN_EXTERNAL_MEMORY_FD \
                 (gpui-wgpu feature `external-memory`)"
                    .into()
            )
        );
        let hal_device = unsafe { context.device.as_hal::<Vulkan>() }
            .ok_or_else(|| Unsupported("engine surfaces require the Vulkan compositor".into()))?;
        let raw = hal_device.raw_device().clone();
        let family = hal_device.queue_family_index();
        let hal_texture = unsafe {
            let image = create_image(&raw, desc)?;
            let requirements = raw.get_image_memory_requirements(image);
            if requirements.memory_type_bits & (1 << self.memory_type) == 0
                || requirements.size > self.allocation_size
            {
                raw.destroy_image(image, None);
                bail!(Unsupported(
                    "the compositor cannot import the engine's memory (different GPU or driver?)"
                        .into()
                ));
            }
            let fd = self.fd.try_clone()?.into_raw_fd();
            let mut import = vk::ImportMemoryFdInfoKHR::default()
                .handle_type(HANDLE)
                .fd(fd);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let memory = match raw.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(self.allocation_size)
                    .memory_type_index(self.memory_type)
                    .push_next(&mut import)
                    .push_next(&mut dedicated),
                None,
            ) {
                Ok(memory) => memory,
                Err(error) => {
                    // A failed import leaves fd ownership with the caller.
                    drop(OwnedFd::from_raw_fd(fd));
                    raw.destroy_image(image, None);
                    bail!("compositor memory import failed: {error:?}");
                }
            };
            if let Err(error) = raw.bind_image_memory(image, memory, 0) {
                raw.free_memory(memory, None);
                raw.destroy_image(image, None);
                bail!("compositor image bind failed: {error:?}");
            }
            hal_device.texture_from_raw(
                image,
                &hal_desc(desc),
                None,
                wgpu::hal::vulkan::TextureMemory::Dedicated(memory),
            )
        };
        let image = unsafe { hal_texture.raw_handle() };
        drop(hal_device);
        // The engine releases each frame in SHADER_READ_ONLY_OPTIMAL; the host
        // acquires without a layout change, so wgpu tracks it as RESOURCE.
        let texture = unsafe {
            context.device.create_texture_from_hal::<Vulkan>(
                hal_texture,
                &wgpu_desc(desc),
                wgpu::TextureUses::RESOURCE,
            )
        };
        *host = Some(HostImport {
            key,
            texture: texture.clone(),
            raw: raw.clone(),
            family,
            image,
        });
        Ok((texture, raw, family, image))
    }
}

pub(crate) fn prepare_frame(
    desc: &SurfaceDesc,
    resources: &SlotResources,
    _sync: &SyncPoint,
    backend: &dyn Any,
) -> Result<Box<dyn Any>> {
    let context = backend
        .downcast_ref::<WgpuCustomContext>()
        .ok_or_else(|| Unsupported("engine surfaces require the wgpu compositor".into()))?;
    let (texture, raw, family, image) = resources.host_import(context, desc)?;
    // Raw commands get the producer's own encoder: wgpu forbids mixing raw and
    // wgpu commands in one encoder, and this producer records nothing else.
    context.record(|encoder| unsafe {
        encoder.as_hal_mut::<Vulkan, _, _>(|encoder| -> Result<()> {
            let encoder = encoder.context("missing Vulkan preparation encoder")?;
            let acquire = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                .dst_queue_family_index(family)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(image)
                .subresource_range(color_range());
            raw.cmd_pipeline_barrier(
                encoder.raw_handle(),
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[acquire],
            );
            Ok(())
        })
    })?;
    // SAFETY: `texture` belongs to the host device, its contents completed
    // before present, the acquire above makes them visible, and the slot's
    // lease keeps it unmodified while this frame can be sampled.
    Ok(Box::new(unsafe { context.adopt_texture(texture) }?))
}

/// A wgpu engine (Vulkan) on its own device rendering into surface slots.
///
/// The engine device must enable [`WgpuSurfaceTarget::required_features`] and
/// run on the compositor's GPU. `present` waits for the engine's GPU work on
/// the calling thread, so call it from the engine thread, not the UI thread.
pub struct WgpuSurfaceTarget {
    surface: EngineSurface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    raw: ash::Device,
    family: u32,
    textures: Vec<wgpu::Texture>,
    images: Vec<vk::Image>,
}

impl WgpuSurfaceTarget {
    /// Features the engine's wgpu device must enable.
    pub fn required_features() -> wgpu::Features {
        wgpu::Features::VULKAN_EXTERNAL_MEMORY_FD
    }

    /// Allocate exportable slots on the engine device and create the surface.
    pub fn new(desc: SurfaceDesc, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self> {
        desc.validate()?;
        ensure!(
            device.features().contains(Self::required_features()),
            Unsupported("the engine device must enable VULKAN_EXTERNAL_MEMORY_FD".into())
        );
        let hal_device = unsafe { device.as_hal::<Vulkan>() }
            .ok_or_else(|| Unsupported("wgpu engine surfaces require the Vulkan backend".into()))?;
        let raw = hal_device.raw_device().clone();
        let family = hal_device.queue_family_index();
        let instance = hal_device.shared_instance().raw_instance().clone();
        let physical = hal_device.raw_physical_device();
        let exporter = ash::khr::external_memory_fd::Device::new(&instance, &raw);
        let properties = unsafe { instance.get_physical_device_memory_properties(physical) };
        let mut slots = Vec::new();
        let mut hal_textures = Vec::new();
        let mut images = Vec::new();
        for _ in 0..desc.slots {
            unsafe {
                let image = create_image(&raw, &desc)?;
                let requirements = raw.get_image_memory_requirements(image);
                let Some(memory_type) = (0..properties.memory_type_count).find(|&index| {
                    requirements.memory_type_bits & (1 << index) != 0
                        && properties.memory_types[index as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                }) else {
                    raw.destroy_image(image, None);
                    bail!("no device-local memory type for engine surface slots");
                };
                let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(HANDLE);
                let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
                let memory = match raw.allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(requirements.size)
                        .memory_type_index(memory_type)
                        .push_next(&mut export)
                        .push_next(&mut dedicated),
                    None,
                ) {
                    Ok(memory) => memory,
                    Err(error) => {
                        raw.destroy_image(image, None);
                        bail!("exportable slot memory allocation failed: {error:?}");
                    }
                };
                raw.bind_image_memory(image, memory, 0)
                    .context("slot image bind failed")?;
                let fd = exporter
                    .get_memory_fd(
                        &vk::MemoryGetFdInfoKHR::default()
                            .memory(memory)
                            .handle_type(HANDLE),
                    )
                    .context("slot memory fd export failed")?;
                hal_textures.push(hal_device.texture_from_raw(
                    image,
                    &hal_desc(&desc),
                    None,
                    wgpu::hal::vulkan::TextureMemory::Dedicated(memory),
                ));
                images.push(image);
                slots.push(Arc::new(SlotResources {
                    fd: OwnedFd::from_raw_fd(fd),
                    allocation_size: requirements.size,
                    memory_type,
                    host: Mutex::new(None),
                }));
            }
        }
        drop(hal_device);
        let textures = hal_textures
            .into_iter()
            .map(|hal| unsafe {
                device.create_texture_from_hal::<Vulkan>(
                    hal,
                    &wgpu_desc(&desc),
                    wgpu::TextureUses::UNINITIALIZED,
                )
            })
            .collect();
        Ok(Self {
            surface: EngineSurface::from_slots(desc, slots)?,
            device: device.clone(),
            queue: queue.clone(),
            raw,
            family,
            textures,
            images,
        })
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        wgpu_format(self.surface.desc().format)
    }

    /// The engine's texture over `slot`.
    pub fn texture(&mut self, slot: &SurfaceSlot) -> Result<wgpu::Texture> {
        Ok(self.textures[slot.index()].clone())
    }

    /// Submit `commands`, release the slot to the compositor, wait for the
    /// engine's GPU work to complete, then publish the slot.
    pub fn present(
        &mut self,
        slot: SurfaceSlot,
        commands: impl IntoIterator<Item = wgpu::CommandBuffer>,
    ) -> Result<()> {
        let texture = &self.textures[slot.index()];
        let image = self.images[slot.index()];
        let mut transition = self.device.create_command_encoder(&Default::default());
        transition.transition_resources(
            std::iter::empty(),
            std::iter::once(wgpu::TextureTransition {
                texture,
                selector: None,
                state: wgpu::TextureUses::RESOURCE,
            }),
        );
        let mut release = self.device.create_command_encoder(&Default::default());
        let raw = &self.raw;
        let family = self.family;
        unsafe {
            release.as_hal_mut::<Vulkan, _, _>(|encoder| -> Result<()> {
                let encoder = encoder.context("missing Vulkan engine encoder")?;
                let barrier = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_queue_family_index(family)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::empty())
                    .image(image)
                    .subresource_range(color_range());
                raw.cmd_pipeline_barrier(
                    encoder.raw_handle(),
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
                Ok(())
            })?;
        }
        let index = self.queue.submit(
            commands
                .into_iter()
                .chain([transition.finish(), release.finish()]),
        );
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(Duration::from_secs(5)),
            })
            .context("engine GPU work did not complete")?;
        self.surface.present(slot, SyncPoint::completed())
    }

    pub fn surface(&self) -> &EngineSurface {
        &self.surface
    }
}
