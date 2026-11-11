//! Linux transport: opaque-fd Vulkan memory and timeline semaphores.
//!
//! The engine is a wgpu device of its own (Vulkan). `WgpuSurfaceTarget` creates
//! one exportable image per slot on the engine device and exports its memory as
//! an opaque fd. The compositor imports each slot once per host device and
//! adopts it through `WgpuCustomContext::adopt_texture`.
//!
//! Each frame the engine transitions the slot to shader-read, releases it to
//! `VK_QUEUE_FAMILY_EXTERNAL` and signals a shared timeline semaphore, then
//! presents without waiting. The compositor imports the semaphore once, stages
//! a GPU-side wait with its preparation batch (#197 K1/K3), and acquires the
//! slot from `VK_QUEUE_FAMILY_EXTERNAL` in a raw-only encoder before sampling
//! (issue #198, spike 2). Without `VK_KHR_external_semaphore_fd` on the engine
//! device the engine waits for completion on its own thread instead; without it
//! on the compositor device, preparation waits on the CPU.
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

/// Readiness of a presented slot: already complete, or a value on the engine's
/// shared timeline that the compositor waits for on the GPU.
#[derive(Clone)]
pub struct SyncPoint(Readiness);
#[derive(Clone)]
enum Readiness {
    Completed,
    Timeline(Arc<Timeline>, u64),
}
impl std::fmt::Debug for SyncPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Readiness::Completed => f.write_str("SyncPoint::Completed"),
            Readiness::Timeline(_, value) => write!(f, "SyncPoint::Timeline({value})"),
        }
    }
}
impl SyncPoint {
    /// The slot's writes have completed on the engine's GPU queue.
    pub fn completed() -> Self {
        Self(Readiness::Completed)
    }
    /// Whether the compositor waits for this point on the GPU.
    pub fn is_gpu_wait(&self) -> bool {
        matches!(self.0, Readiness::Timeline(..))
    }
}

const SEMAPHORE_HANDLE: vk::ExternalSemaphoreHandleTypeFlags =
    vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD;

unsafe fn timeline_semaphore(raw: &ash::Device, exportable: bool) -> Result<vk::Semaphore> {
    let mut kind = vk::SemaphoreTypeCreateInfo::default()
        .semaphore_type(vk::SemaphoreType::TIMELINE)
        .initial_value(0);
    let mut export = vk::ExportSemaphoreCreateInfo::default().handle_types(SEMAPHORE_HANDLE);
    let mut info = vk::SemaphoreCreateInfo::default().push_next(&mut kind);
    if exportable {
        info = info.push_next(&mut export);
    }
    unsafe { raw.create_semaphore(&info, None) }.context("timeline semaphore creation failed")
}

/// The engine's exportable timeline semaphore and the compositor's import.
/// Holding device clones keeps both Vulkan devices alive until it is dropped.
pub(crate) struct Timeline {
    _engine: wgpu::Device,
    raw: ash::Device,
    semaphore: vk::Semaphore,
    fd: OwnedFd,
    host: Mutex<Option<HostSemaphore>>,
}
struct HostSemaphore {
    key: usize,
    _device: Arc<wgpu::Device>,
    raw: ash::Device,
    semaphore: vk::Semaphore,
}
impl Timeline {
    fn reached(&self, value: u64) -> bool {
        unsafe { self.raw.get_semaphore_counter_value(self.semaphore) }
            .is_ok_and(|current| current >= value)
    }
    /// The compositor's import, or `None` when its device cannot import.
    fn host_semaphore(&self, context: &WgpuCustomContext) -> Result<Option<vk::Semaphore>> {
        let key = context.cache_key();
        let mut host = self.host.lock().unwrap();
        if let Some(import) = host.as_ref().filter(|import| import.key == key) {
            return Ok(Some(import.semaphore));
        }
        let hal_device = unsafe { context.device.as_hal::<Vulkan>() }
            .ok_or_else(|| Unsupported("engine surfaces require the Vulkan compositor".into()))?;
        if !hal_device
            .enabled_device_extensions()
            .contains(&ash::khr::external_semaphore_fd::NAME)
        {
            return Ok(None);
        }
        let raw = hal_device.raw_device().clone();
        let instance = hal_device.shared_instance().raw_instance().clone();
        drop(hal_device);
        let semaphore = unsafe { timeline_semaphore(&raw, false)? };
        let fd = self.fd.try_clone()?.into_raw_fd();
        let imported = unsafe {
            ash::khr::external_semaphore_fd::Device::new(&instance, &raw).import_semaphore_fd(
                &vk::ImportSemaphoreFdInfoKHR::default()
                    .semaphore(semaphore)
                    .handle_type(SEMAPHORE_HANDLE)
                    .fd(fd),
            )
        };
        if let Err(error) = imported {
            unsafe {
                // A failed import leaves fd ownership with the caller.
                drop(OwnedFd::from_raw_fd(fd));
                raw.destroy_semaphore(semaphore, None);
            }
            bail!("compositor timeline semaphore import failed: {error:?}");
        }
        if let Some(previous) = host.take() {
            unsafe { previous.raw.destroy_semaphore(previous.semaphore, None) };
        }
        *host = Some(HostSemaphore {
            key,
            _device: context.device.clone(),
            raw,
            semaphore,
        });
        Ok(Some(semaphore))
    }
    /// Fallback for a compositor that cannot import the semaphore.
    fn cpu_wait(&self, value: u64) -> Result<()> {
        let semaphores = [self.semaphore];
        let values = [value];
        unsafe {
            self.raw.wait_semaphores(
                &vk::SemaphoreWaitInfo::default()
                    .semaphores(&semaphores)
                    .values(&values),
                2_000_000_000,
            )
        }
        .context("engine timeline wait failed")
    }
}
impl Drop for Timeline {
    fn drop(&mut self) {
        // Frames holding this timeline are released only after the host GPU
        // work that waited on it completed; the engine target waits for its
        // queue before dropping its reference.
        unsafe {
            if let Some(host) = self.host.lock().unwrap().take() {
                host.raw.destroy_semaphore(host.semaphore, None);
            }
            self.raw.destroy_semaphore(self.semaphore, None);
        }
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
    sync: &SyncPoint,
    backend: &dyn Any,
) -> Result<Box<dyn Any>> {
    let context = backend
        .downcast_ref::<WgpuCustomContext>()
        .ok_or_else(|| Unsupported("engine surfaces require the wgpu compositor".into()))?;
    let (texture, raw, family, image) = resources.host_import(context, desc)?;
    if let Readiness::Timeline(timeline, value) = &sync.0
        && !timeline.reached(*value)
    {
        match timeline.host_semaphore(context)? {
            // SAFETY: imported on the host device from the engine's timeline,
            // kept alive by this frame, and the engine submitted the signal
            // before presenting.
            Some(semaphore) => unsafe { context.wait_vulkan_semaphore(semaphore, *value)? },
            None => timeline.cpu_wait(*value)?,
        }
    }
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
            // ALL_COMMANDS in the first scope chains the batch's semaphore
            // wait to this barrier, and its second scope to the composition.
            raw.cmd_pipeline_barrier(
                encoder.raw_handle(),
                vk::PipelineStageFlags::ALL_COMMANDS,
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
    timeline: Option<Arc<Timeline>>,
    next: u64,
}

impl Drop for WgpuSurfaceTarget {
    fn drop(&mut self) {
        // Let pending signals of the shared timeline complete before it can be
        // destroyed.
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(5)),
        });
    }
}

impl WgpuSurfaceTarget {
    /// Features the engine's wgpu device must enable.
    pub fn required_features() -> wgpu::Features {
        wgpu::Features::VULKAN_EXTERNAL_MEMORY_FD
    }

    /// Create an engine device with the required features and, when supported,
    /// `VK_KHR_external_semaphore_fd` for GPU-side handoff.
    pub fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("engine"),
            required_features: Self::required_features(),
            ..Default::default()
        };
        let name = ash::khr::external_semaphore_fd::NAME;
        let open = unsafe { adapter.as_hal::<Vulkan>() }.and_then(|hal_adapter| {
            let available = unsafe {
                hal_adapter
                    .shared_instance()
                    .raw_instance()
                    .enumerate_device_extension_properties(hal_adapter.raw_physical_device())
            }
            .ok()?;
            if !available
                .iter()
                .any(|extension| extension.extension_name_as_c_str() == Ok(name))
            {
                return None;
            }
            let callback: Box<wgpu::hal::vulkan::CreateDeviceCallback> = Box::new(move |args| {
                if !args.extensions.contains(&name) {
                    args.extensions.push(name);
                }
            });
            // SAFETY: the callback only adds an extension the adapter supports.
            unsafe {
                hal_adapter.open_with_callback(
                    descriptor.required_features,
                    &descriptor.required_limits,
                    &descriptor.memory_hints,
                    Some(callback),
                )
            }
            .ok()
        });
        if let Some(open) = open {
            // SAFETY: `open` came from this adapter with the descriptor's features.
            if let Ok(pair) = unsafe { adapter.create_device_from_hal(open, &descriptor) } {
                return Ok(pair);
            }
        }
        Ok(gpui::block_on(adapter.request_device(&descriptor))?)
    }

    /// Whether frames are handed over with a GPU-side wait (shared timeline
    /// semaphore) rather than the engine waiting on its own thread.
    pub fn gpu_sync(&self) -> bool {
        self.timeline.is_some()
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
        let timeline = if hal_device
            .enabled_device_extensions()
            .contains(&ash::khr::external_semaphore_fd::NAME)
        {
            let semaphore = unsafe { timeline_semaphore(&raw, true)? };
            let fd = unsafe {
                ash::khr::external_semaphore_fd::Device::new(&instance, &raw).get_semaphore_fd(
                    &vk::SemaphoreGetFdInfoKHR::default()
                        .semaphore(semaphore)
                        .handle_type(SEMAPHORE_HANDLE),
                )
            };
            match fd {
                Ok(fd) => Some(Arc::new(Timeline {
                    _engine: device.clone(),
                    raw: raw.clone(),
                    semaphore,
                    fd: unsafe { OwnedFd::from_raw_fd(fd) },
                    host: Mutex::new(None),
                })),
                Err(_) => {
                    unsafe { raw.destroy_semaphore(semaphore, None) };
                    None
                }
            }
        } else {
            None
        };
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
            timeline,
            next: 0,
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
        let commands = commands
            .into_iter()
            .chain([transition.finish(), release.finish()]);
        if let Some(timeline) = &self.timeline {
            // Signal after this submission; present only once it is submitted.
            self.next += 1;
            unsafe { self.queue.as_hal::<Vulkan>() }
                .context("missing Vulkan engine queue")?
                .add_signal_semaphore(timeline.semaphore, Some(self.next));
            self.queue.submit(commands);
            return self.surface.present(
                slot,
                SyncPoint(Readiness::Timeline(timeline.clone(), self.next)),
            );
        }
        let index = self.queue.submit(commands);
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
