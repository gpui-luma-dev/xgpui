//! Synthetic immutable BGRA snapshots exported from a dedicated Vulkan device.
//! CPU pixels are staged once per displayed snapshot; rendering imports the real
//! DMA-BUF. This is not a decoder, pooled video surface, or zero-copy producer.
use super::pattern::{self, COLORS, HEIGHT, WIDTH};
use anyhow::{Context as _, Result, ensure};
use ash::vk;
use gpui::*;
use gpui_wgpu::{custom_draw::WgpuCustomContext, wgpu};
use std::{
    any::Any,
    os::fd::{FromRawFd, OwnedFd},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use wgpu::hal::api::Vulkan;
use xgpui_media::{DmaBufFrame, DmaBufLayout, VideoFrame};

type Errors = Arc<Mutex<Option<String>>>;

#[derive(Clone)]
pub(super) struct Source {
    producer: Arc<Mutex<Option<Arc<Producer>>>>,
    error: Errors,
}
impl Source {
    pub(super) fn new(error: Errors) -> Self {
        Self {
            producer: Arc::new(Mutex::new(None)),
            error,
        }
    }
    fn snapshot(&self, index: u64) -> Arc<Snapshot> {
        Arc::new(Snapshot {
            source: self.clone(),
            pixels: pattern::pixels(index),
            imported: Mutex::new(None),
            initializations: AtomicUsize::new(0),
        })
    }
    pub(super) fn video_frame(&self, index: u64) -> Result<VideoFrame> {
        self.snapshot(index)
            .video_frame(Duration::from_micros(index.saturating_mul(33_333)))
    }
    fn producer(&self, context: &WgpuCustomContext) -> Result<Arc<Producer>> {
        ensure!(
            DmaBufFrame::supported_by(context),
            "this renderer needs Vulkan DMA-BUF import capability"
        );
        let mut cached = self
            .producer
            .lock()
            .map_err(|_| anyhow::anyhow!("video producer cache poisoned"))?;
        if cached
            .as_ref()
            .is_none_or(|p| !Arc::ptr_eq(&p.host, &context.device))
        {
            *cached = Some(Arc::new(Producer::new(context.device.clone())?));
        }
        Ok(cached.as_ref().unwrap().clone())
    }
    fn report(&self, error: &anyhow::Error) {
        if let Ok(mut current) = self.error.lock() {
            if current.is_none() {
                eprintln!("Linux video surface diagnostic: {error:#}");
            }
            *current = Some(error.to_string());
        }
    }
}
struct Imported {
    host: Arc<wgpu::Device>,
    queue_key: usize,
    frame: DmaBufFrame,
}
struct Snapshot {
    source: Source,
    pixels: Vec<u8>,
    imported: Mutex<Option<Imported>>,
    initializations: AtomicUsize,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxVideoSnapshot")
            .field("bytes", &self.pixels.len())
            .field(
                "initializations",
                &self.initializations.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}
fn extent() -> Size<DevicePixels> {
    size(DevicePixels(WIDTH as i32), DevicePixels(HEIGHT as i32))
}
impl Snapshot {
    fn video_frame(self: &Arc<Self>, timestamp: Duration) -> Result<VideoFrame> {
        VideoFrame::new(extent(), timestamp, self.clone())
    }
}
impl CustomContent for Snapshot {
    fn image_source_size(&self) -> Option<Size<DevicePixels>> {
        Some(extent())
    }
    fn prepare(&self, backend: &dyn Any, size: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let result = (|| {
            let context = backend
                .downcast_ref::<WgpuCustomContext>()
                .context("Linux video surfaces need the wgpu Vulkan renderer")?;
            let mut cache = self
                .imported
                .lock()
                .map_err(|_| anyhow::anyhow!("video import cache poisoned"))?;
            if cache.as_ref().is_none_or(|c| {
                !Arc::ptr_eq(&c.host, &context.device) || c.queue_key != context.cache_key()
            }) {
                let producer = self.source.producer(context)?;
                let frame = export_frame(producer, &self.pixels)?;
                *cache = Some(Imported {
                    host: context.device.clone(),
                    queue_key: context.cache_key(),
                    frame,
                });
                self.initializations.fetch_add(1, Ordering::Relaxed);
            }
            cache.as_ref().unwrap().frame.prepare(context, size)
        })();
        if let Err(error) = &result {
            self.source.report(error);
        }
        result
    }
}

// Opt-in native validation also counts errors from the raw producer device.
// Merely setting a layer without observing its callbacks would not make a
// passing pixel test evidence of correct Vulkan object and barrier usage.
struct Validation {
    _host: Arc<wgpu::Device>,
    api: ash::ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
    errors: Box<AtomicUsize>,
}
unsafe extern "system" fn validation_message(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        // SAFETY: the boxed counter outlives this messenger; Vulkan supplies data.
        unsafe {
            let errors = &*user.cast::<AtomicUsize>();
            errors.fetch_add(1, Ordering::SeqCst);
            if let Some(data) = data.as_ref()
                && !data.p_message.is_null()
            {
                use std::io::Write as _;
                let _ = writeln!(
                    std::io::stderr(),
                    "Linux video Vulkan validation: {}",
                    std::ffi::CStr::from_ptr(data.p_message).to_string_lossy()
                );
            }
        }
    }
    vk::FALSE
}
impl Validation {
    fn new(host: Arc<wgpu::Device>) -> Result<Option<Self>> {
        if !std::env::var("VK_INSTANCE_LAYERS")
            .unwrap_or_default()
            .split(':')
            .any(|layer| layer == "VK_LAYER_KHRONOS_validation")
        {
            return Ok(None);
        }
        // SAFETY: host retains the instance; only debug-utils entry points are read.
        let hal = unsafe { host.as_hal::<Vulkan>() }.context("Vulkan validation needs Vulkan")?;
        ensure!(
            hal.shared_instance()
                .extensions()
                .contains(&ash::ext::debug_utils::NAME),
            "enable WGPU_DEBUG=1 for video validation callbacks"
        );
        let api = ash::ext::debug_utils::Instance::new(
            hal.shared_instance().entry(),
            hal.shared_instance().raw_instance(),
        );
        let mut errors = Box::new(AtomicUsize::new(0));
        let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR)
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(validation_message))
            .user_data((&mut *errors as *mut AtomicUsize).cast());
        let messenger = unsafe { api.create_debug_utils_messenger(&info, None) }?;
        drop(hal);
        Ok(Some(Self {
            _host: host,
            api,
            messenger,
            errors,
        }))
    }
    fn check(&self) -> Result<()> {
        ensure!(
            self.errors.load(Ordering::SeqCst) == 0,
            "Linux video Vulkan validation reported errors; see terminal diagnostics"
        );
        Ok(())
    }
}
impl Drop for Validation {
    fn drop(&mut self) {
        unsafe { self.api.destroy_debug_utils_messenger(self.messenger, None) };
    }
}

struct Producer {
    // Keeps the shared Vulkan instance alive. This is NOT the device we submit on.
    host: Arc<wgpu::Device>,
    instance: ash::Instance,
    device: ash::Device,
    properties: vk::PhysicalDeviceMemoryProperties,
    family: u32,
    queue: vk::Queue,
    // Vulkan queues require external synchronization, including between windows.
    submit: Mutex<()>,
    validation: Option<Validation>,
}
impl Producer {
    fn new(host: Arc<wgpu::Device>) -> Result<Self> {
        let validation = Validation::new(host.clone())?;
        // SAFETY: wgpu owns the host; the guard lives through all raw handle reads.
        // All submissions below use a NEW logical device and its own queue.
        let hal = unsafe { host.as_hal::<Vulkan>() }.context("Vulkan renderer required")?;
        let instance = hal.shared_instance().raw_instance().clone();
        let physical = hal.raw_physical_device();
        let family = hal.queue_family_index();
        let extensions = [
            ash::khr::external_memory_fd::NAME,
            ash::ext::external_memory_dma_buf::NAME,
            ash::ext::image_drm_format_modifier::NAME,
        ];
        // SAFETY: physical belongs to the live host instance.
        let available = unsafe { instance.enumerate_device_extension_properties(physical) }?;
        for name in extensions {
            ensure!(
                available
                    .iter()
                    .any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name)),
                "video producer requires {}",
                name.to_string_lossy()
            );
        }
        let info = unsafe { instance.get_physical_device_properties(physical) };
        ensure!(
            info.api_version >= vk::API_VERSION_1_2,
            "video producer requires Vulkan 1.2 or newer"
        );
        let mut external_info = vk::PhysicalDeviceExternalImageFormatInfo::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let mut modifier_info = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::default()
            .drm_format_modifier(0)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let query = vk::PhysicalDeviceImageFormatInfo2::default()
            .format(vk::Format::B8G8R8A8_UNORM)
            .ty(vk::ImageType::TYPE_2D)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
            .push_next(&mut external_info)
            .push_next(&mut modifier_info);
        let mut external_properties = vk::ExternalImageFormatProperties::default();
        let mut image_properties =
            vk::ImageFormatProperties2::default().push_next(&mut external_properties);
        unsafe {
            instance.get_physical_device_image_format_properties2(
                physical,
                &query,
                &mut image_properties,
            )
        }
        .context("current GPU cannot export linear BGRA video DMA-BUFs")?;
        let max_extent = image_properties.image_format_properties.max_extent;
        ensure!(
            external_properties
                .external_memory_properties
                .external_memory_features
                .contains(
                    vk::ExternalMemoryFeatureFlags::IMPORTABLE
                        | vk::ExternalMemoryFeatureFlags::EXPORTABLE
                ),
            "linear BGRA video needs DMA-BUF export and import on the same GPU"
        );
        ensure!(
            max_extent.width >= WIDTH && max_extent.height >= HEIGHT,
            "DMA-BUF extent cannot hold the synthetic source"
        );
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities)];
        let names = extensions.map(|n| n.as_ptr());
        let create = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&names);
        let properties = unsafe { instance.get_physical_device_memory_properties(physical) };
        // SAFETY: extension availability, queue family and Vulkan core version were checked.
        let device = unsafe { instance.create_device(physical, &create, None) }
            .context("create isolated video producer device")?;
        let queue = unsafe { device.get_device_queue(family, 0) };
        drop(hal);
        Ok(Self {
            host,
            instance,
            device,
            properties,
            family,
            queue,
            submit: Mutex::new(()),
            validation,
        })
    }
}
impl Drop for Producer {
    fn drop(&mut self) {
        // Each export owns this device through Arc and retires its fence before
        // releasing that reference. No queue operation can remain at this point.
        unsafe {
            self.device.destroy_device(None);
        }
    }
}

struct Exporter {
    producer: Arc<Producer>,
    device: ash::Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    pool: vk::CommandPool,
    fence: vk::Fence,
    submitted: bool,
}
impl Drop for Exporter {
    fn drop(&mut self) {
        // Failure/timeout must not free resources that the GPU still references.
        if self.submitted
            && unsafe {
                self.device
                    .wait_for_fences(&[self.fence], true, 10_000_000_000)
            }
            .is_err()
        {
            eprintln!(
                "DMA-BUF producer GPU did not retire; retaining native objects until process exit"
            );
            // Leak a device reference too: freeing VkDevice would invalidate the
            // objects still in flight after a timeout or device error.
            std::mem::forget(self.producer.clone());
            return;
        }
        unsafe {
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
            self.device.destroy_buffer(self.staging, None);
            self.device.free_memory(self.staging_memory, None);
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_fence(self.fence, None);
        }
    }
}
fn memory_type(
    properties: vk::PhysicalDeviceMemoryProperties,
    bits: u32,
    flags: vk::MemoryPropertyFlags,
) -> Result<u32> {
    (0..properties.memory_type_count)
        .find(|index| {
            bits & (1 << index) != 0
                && properties.memory_types[*index as usize]
                    .property_flags
                    .contains(flags)
        })
        .context("DMA-BUF video source has no compatible memory type")
}

fn export_frame(producer: Arc<Producer>, pixels: &[u8]) -> Result<DmaBufFrame> {
    ensure!(
        pixels.len() == (WIDTH * HEIGHT * 4) as usize,
        "invalid video frame byte count"
    );
    let _submission = producer
        .submit
        .lock()
        .map_err(|_| anyhow::anyhow!("video producer queue poisoned"))?;
    let device = producer.device.clone();
    let instance = producer.instance.clone();
    let properties = producer.properties;
    let family = producer.family;
    let queue = producer.queue;
    let mut export = Exporter {
        producer: producer.clone(),
        device,
        image: vk::Image::null(),
        memory: vk::DeviceMemory::null(),
        staging: vk::Buffer::null(),
        staging_memory: vk::DeviceMemory::null(),
        pool: vk::CommandPool::null(),
        fence: vk::Fence::null(),
        submitted: false,
    };
    let dimensions = extent();
    let extent = vk::Extent3D {
        width: WIDTH,
        height: HEIGHT,
        depth: 1,
    };
    let modifiers = [0u64];
    let mut modifier =
        vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
    let mut external = vk::ExternalMemoryImageCreateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let create = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(vk::Format::B8G8R8A8_UNORM)
        .extent(extent)
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
        .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .push_next(&mut external)
        .push_next(&mut modifier);
    export.image = unsafe { export.device.create_image(&create, None) }
        .context("create exportable linear BGRA image")?;
    let requirements = unsafe { export.device.get_image_memory_requirements(export.image) };
    let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(export.image);
    let mut fd_export = vk::ExportMemoryAllocateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type(
            properties,
            requirements.memory_type_bits,
            vk::MemoryPropertyFlags::empty(),
        )?)
        .push_next(&mut dedicated)
        .push_next(&mut fd_export);
    export.memory = unsafe { export.device.allocate_memory(&allocate, None) }
        .context("allocate exportable DMA-BUF memory")?;
    unsafe {
        export
            .device
            .bind_image_memory(export.image, export.memory, 0)
    }?;
    let modifier_api = ash::ext::image_drm_format_modifier::Device::new(&instance, &export.device);
    let mut selected = vk::ImageDrmFormatModifierPropertiesEXT::default();
    unsafe { modifier_api.get_image_drm_format_modifier_properties(export.image, &mut selected) }?;
    ensure!(
        selected.drm_format_modifier == 0,
        "video source selected a non-linear modifier"
    );
    let layout = unsafe {
        export.device.get_image_subresource_layout(
            export.image,
            vk::ImageSubresource {
                aspect_mask: vk::ImageAspectFlags::MEMORY_PLANE_0_EXT,
                mip_level: 0,
                array_layer: 0,
            },
        )
    };
    let layout = DmaBufLayout::linear_bgra(dimensions, layout.row_pitch, layout.offset, 0)?;

    export.staging = unsafe {
        export.device.create_buffer(
            &vk::BufferCreateInfo::default()
                .size(pixels.len() as u64)
                .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                .sharing_mode(vk::SharingMode::EXCLUSIVE),
            None,
        )
    }?;
    let requirements = unsafe { export.device.get_buffer_memory_requirements(export.staging) };
    export.staging_memory = unsafe {
        export.device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(memory_type(
                    properties,
                    requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )?),
            None,
        )
    }?;
    unsafe {
        export
            .device
            .bind_buffer_memory(export.staging, export.staging_memory, 0)?;
        let mapped = export.device.map_memory(
            export.staging_memory,
            0,
            pixels.len() as u64,
            vk::MemoryMapFlags::empty(),
        )?;
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), mapped.cast(), pixels.len());
        export.device.unmap_memory(export.staging_memory);
    }
    export.pool = unsafe {
        export.device.create_command_pool(
            &vk::CommandPoolCreateInfo::default().queue_family_index(family),
            None,
        )
    }?;
    let command = unsafe {
        export.device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default()
                .command_pool(export.pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1),
        )
    }?[0];
    let range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    unsafe {
        export.device.begin_command_buffer(
            command,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;
        let initialize = vk::ImageMemoryBarrier::default()
            .image(export.image)
            .subresource_range(range)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
        export.device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[initialize],
        );
        let copy = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(extent);
        export.device.cmd_copy_buffer_to_image(
            command,
            export.staging,
            export.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[copy],
        );
        // Resolve the producer's layout first, then perform a matching
        // GENERAL -> GENERAL ownership release for the importer's acquire.
        let ready = vk::ImageMemoryBarrier::default()
            .image(export.image)
            .subresource_range(range)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::MEMORY_READ);
        export.device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[ready],
        );
        let release = vk::ImageMemoryBarrier::default()
            .image(export.image)
            .subresource_range(range)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(family)
            .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
            .src_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE);
        export.device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[release],
        );
        export.device.end_command_buffer(command)?;
        export.fence = export.device.create_fence(&Default::default(), None)?;
        let commands = [command];
        export.device.queue_submit(
            queue,
            &[vk::SubmitInfo::default().command_buffers(&commands)],
            export.fence,
        )?;
        export.submitted = true;
        export
            .device
            .wait_for_fences(&[export.fence], true, 10_000_000_000)
            .context("DMA-BUF producer fence timeout/failure")?;
    }
    let fd_api = ash::khr::external_memory_fd::Device::new(&instance, &export.device);
    let raw_fd = unsafe {
        fd_api.get_memory_fd(
            &vk::MemoryGetFdInfoKHR::default()
                .memory(export.memory)
                .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT),
        )
    }?;
    // SAFETY: Vulkan returned a new owned DMA-BUF FD. Layout is queried from its
    // initialized allocation; producer fence and matching release have completed.
    let frame = unsafe { DmaBufFrame::from_bgra(OwnedFd::from_raw_fd(raw_fd), layout) };
    // The owned FD retains the allocation; producer image/staging objects may
    // retire after the completed fence. Never write or recycle this allocation.
    drop(export);
    Ok(frame)
}

pub(super) fn capture(png: &Path) -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    let source_factory = Source::new(Arc::new(Mutex::new(None)));
    let source = source_factory.snapshot(0);
    let video = source.video_frame(Duration::ZERO)?;
    let bounds = Bounds::new(
        point(ScaledPixels(4.), ScaledPixels(4.)),
        size(ScaledPixels(320.), ScaledPixels(180.)),
    );
    let mut scene = Scene::default();
    scene.insert_primitive(CustomDraw {
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::all(ScaledPixels(12.)),
        content: video.content(),
        coverage: None,
        opacity: 1.,
        order: Default::default(),
    });
    scene.finish();
    let output_size = size(DevicePixels(328), DevicePixels(188));
    let pixels = renderer.render_scene_to_image(&scene, output_size)?;
    for (x, expected) in [24, 104, 184, 264].into_iter().zip(COLORS) {
        let pixel = pixels.get_pixel(x, 24).0;
        ensure!(
            (0..3).all(|i| pixel[i].abs_diff(expected[i]) <= 3),
            "DMA-BUF BGRA color at {x}: {pixel:?}"
        );
    }
    let translucent = pixels.get_pixel(264, 124).0;
    ensure!(
        (0..3).all(|i| translucent[i].abs_diff((u16::from(COLORS[3][i]) * 128 / 255) as u8) <= 3),
        "DMA-BUF straight alpha: {translucent:?}"
    );
    ensure!(
        pixels.get_pixel(4, 4).0[..3] == [0, 0, 0],
        "DMA-BUF rounded corners"
    );
    let next = source_factory.snapshot(8);
    scene.custom_draws[0].content = next.video_frame(Duration::from_millis(264))?.content();
    let moving = renderer.render_scene_to_image(&scene, output_size)?;
    let changed = pixels
        .pixels()
        .zip(moving.pixels())
        .filter(|(a, b)| a != b)
        .count();
    ensure!(
        changed > (WIDTH * HEIGHT / 4) as usize,
        "DMA-BUF playback must update the animated region: {changed} pixels changed"
    );
    scene.custom_draws[0].content = video.content();
    drop(next);
    use xgpui_media::pipeline::{ColorMatrix, ImageAdjustments, ImagePrimaries};
    scene.custom_draws[0].content = video
        .adjusted(ImageAdjustments::new(
            ColorMatrix::identity(ImagePrimaries::Srgb).exposure(-1.)?,
            None,
        )?)
        .content();
    let adjusted = renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        (150..=180).contains(&adjusted.get_pixel(24, 24).0[0]),
        "DMA-BUF linear exposure must affect the adjusted preview"
    );
    scene.custom_draws[0].content = video.content();
    scene.custom_draws[0].opacity = 0.5;
    let opacity = renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        opacity.get_pixel(24, 24).0[0].abs_diff(112) <= 3,
        "DMA-BUF inherited opacity"
    );
    scene.custom_draws[0].content_mask.bounds.origin.x = ScaledPixels(20.);
    scene.custom_draws[0].content_mask.bounds.size.width = ScaledPixels(304.);
    let clipped = renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        clipped.get_pixel(10, 50).0[..3] == [0, 0, 0],
        "DMA-BUF content mask"
    );
    let mut duplicate = scene.custom_draws[0].clone();
    duplicate.bounds.size = size(ScaledPixels(160.), ScaledPixels(90.));
    scene.custom_draws.push(duplicate);
    renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        source.initializations.load(Ordering::Relaxed) == 1,
        "both placements and resize must reuse one DMA-BUF texture"
    );

    // Dropping the producer/cache does not invalidate already prepared outputs.
    // Scene content retains this source until the last presentation drops it.
    let weak = Arc::downgrade(&source);
    drop(source);
    drop(video);
    renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        weak.upgrade().is_some(),
        "scene must retain its source after producer drop"
    );
    scene.custom_draws.clear();
    drop(scene);
    drop(renderer);
    ensure!(
        weak.upgrade().is_none(),
        "source must retire after scene/renderer teardown"
    );
    // Retire the raw exporter device with its debug messenger still attached.
    // The factory is the only owner after all synchronous exports completed.
    if let Some(producer) = source_factory.producer.lock().unwrap().take() {
        let mut producer = Arc::try_unwrap(producer)
            .map_err(|_| anyhow::anyhow!("video exporter still referenced after capture"))?;
        producer.host.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })?;
        let validation = producer.validation.take();
        drop(producer);
        if let Some(validation) = validation {
            validation.check()?;
            eprintln!(
                "Linux video: no Vulkan validation errors during export/import, rendering and producer teardown"
            );
        }
    }
    pixels.save(png)?;
    println!(
        "Saved {}: DMA-BUF BGRA colors, alpha, corners, opacity, clipping, placement reuse and teardown passed.",
        png.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgpui_media::VideoStream;

    #[::core::prelude::v1::test]
    fn snapshots_are_lazy_and_survive_pause_burst_and_shutdown() -> Result<()> {
        let errors = Arc::new(Mutex::new(None));
        let source = Source::new(errors.clone());
        let (sender, stream) = VideoStream::channel();
        let first = source.snapshot(0);
        let weak = Arc::downgrade(&first);
        sender.publish(first.video_frame(Duration::ZERO)?)?;
        drop(first);
        let shown = stream.snapshot().context("initial frame")?;
        stream.set_playing(false);
        for index in 1..=120 {
            sender.publish(source.video_frame(index)?)?;
        }
        ensure!(
            stream.snapshot().unwrap().timestamp() == Duration::ZERO,
            "pause retains displayed frame"
        );
        ensure!(
            stream.stats().replaced == 119 && stream.stats().pending,
            "burst keeps one pending frame"
        );
        ensure!(
            source.producer.lock().unwrap().is_none(),
            "unpresented frames must not allocate Vulkan resources"
        );
        stream.set_playing(true);
        ensure!(
            stream.snapshot().unwrap().timestamp() == Duration::from_micros(120 * 33_333),
            "resume uses newest frame"
        );
        ensure!(
            weak.upgrade().is_some(),
            "external snapshot retains old pixels"
        );
        drop(shown);
        ensure!(
            weak.upgrade().is_none(),
            "old pixels retire with the last snapshot"
        );
        drop(sender);
        ensure!(
            stream.stats().closed && stream.snapshot().is_some(),
            "shutdown retains last frame"
        );
        ensure!(
            errors.lock().unwrap().is_none(),
            "CPU lifecycle needs no graphics device"
        );
        Ok(())
    }

    #[::core::prelude::v1::test]
    fn invalid_backend_reports_a_recoverable_source_error() -> Result<()> {
        let error = Arc::new(Mutex::new(None));
        let source = Source::new(error.clone());
        let frame = source.snapshot(0);
        ensure!(
            frame.prepare(&(), extent()).is_err(),
            "non-Vulkan backends must be rejected"
        );
        ensure!(
            error.lock().unwrap().is_some(),
            "UI must be notified of a preparation failure"
        );
        ensure!(
            source.producer.lock().unwrap().is_none(),
            "failure must not initialize a producer"
        );
        let restarted_error = Arc::new(Mutex::new(None));
        let restarted = Source::new(restarted_error.clone());
        ensure!(
            restarted.video_frame(0).is_ok() && restarted_error.lock().unwrap().is_none(),
            "retry starts with independent state"
        );
        Ok(())
    }

    #[::core::prelude::v1::test]
    #[ignore = "requires Linux Vulkan BGRA DMA-BUF export/import; run video-native-test"]
    fn dma_buf_video_capture_pixels_and_lifetimes() -> Result<()> {
        let png =
            std::env::temp_dir().join(format!("media-lab-linux-video-{}.png", std::process::id()));
        capture(&png)
    }
}
