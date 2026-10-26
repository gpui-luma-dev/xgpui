//! Explicit Linux-only native test. No media file, decoder or GBM dependency.
use super::*;
use ash::vk;
use gpui::{
    Bounds, ContentMask, Corners, CustomContent, PlatformHeadlessRenderer, ScaledPixels, Scene,
    point,
};
use std::io::Write as _;
use std::os::fd::FromRawFd;
use wgpu::hal::api::Vulkan;

// Count native Vulkan validation errors; pixel checks alone cannot establish
// correctness of raw external-memory barriers or object lifetimes.
struct Validation {
    api: ash::ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
    errors: Box<std::sync::atomic::AtomicUsize>,
}
unsafe extern "system" fn validation_message(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        // SAFETY: the boxed counter lives until DestroyDebugUtilsMessengerEXT
        // has stopped callbacks. Vulkan supplies callback data for this call.
        unsafe {
            let errors = &*user.cast::<std::sync::atomic::AtomicUsize>();
            errors.fetch_add(1, Ordering::SeqCst);
            if let Some(data) = data.as_ref()
                && !data.p_message.is_null()
            {
                let _ = writeln!(
                    std::io::stderr(),
                    "DMA-BUF Vulkan validation: {}",
                    std::ffi::CStr::from_ptr(data.p_message).to_string_lossy()
                );
            }
        }
    }
    vk::FALSE
}
impl Validation {
    fn new(host: &crate::WgpuContext) -> Result<Self> {
        ensure!(
            std::env::var("VK_INSTANCE_LAYERS")
                .unwrap_or_default()
                .split(':')
                .any(|layer| layer == "VK_LAYER_KHRONOS_validation"),
            "native DMA-BUF validation requires the dma-buf-native-test recipe and Khronos validation layer"
        );
        let hal =
            unsafe { host.device.as_hal::<Vulkan>() }.context("native fixture requires Vulkan")?;
        ensure!(
            hal.shared_instance()
                .extensions()
                .contains(&ash::ext::debug_utils::NAME),
            "Vulkan debug utils unavailable; enable WGPU_DEBUG=1 and WGPU_VALIDATION=1"
        );
        let api = ash::ext::debug_utils::Instance::new(
            hal.shared_instance().entry(),
            hal.shared_instance().raw_instance(),
        );
        let mut errors = Box::new(std::sync::atomic::AtomicUsize::new(0));
        let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR)
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(validation_message))
            .user_data((&mut *errors as *mut std::sync::atomic::AtomicUsize).cast());
        let messenger = unsafe { api.create_debug_utils_messenger(&info, None) }?;
        Ok(Self {
            api,
            messenger,
            errors,
        })
    }
    fn check(&self) -> Result<()> {
        ensure!(
            self.errors.load(Ordering::SeqCst) == 0,
            "native DMA-BUF Vulkan validation reported errors; see log"
        );
        Ok(())
    }
}
impl Drop for Validation {
    fn drop(&mut self) {
        unsafe { self.api.destroy_debug_utils_messenger(self.messenger, None) };
    }
}

// These resources belong to a standalone generated exporter. The importer gets
// only an owned DMA-BUF FD, never a wgpu texture or a CPU upload of these pixels.
struct Exporter {
    _host: Arc<wgpu::Device>,
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
                "DMA-BUF fixture GPU did not retire; retaining native objects until process exit"
            );
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
        .context("DMA-BUF fixture has no compatible memory type")
}

fn generate(host: &crate::WgpuContext) -> Result<(Exporter, DmaBufFrame)> {
    ensure!(
        host.adapter.get_info().backend == wgpu::Backend::Vulkan,
        "native DMA-BUF test requires Vulkan"
    );
    ensure!(
        host.device
            .features()
            .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF),
        "adapter lacks DMA-BUF/modifier import capability; this is not a passing native test"
    );
    // This ignored test runs alone (--test-threads=1); raw queue submission must
    // not overlap wgpu queue calls. The exporter finishes before GPUI rendering.
    let hal = unsafe { host.device.as_hal::<Vulkan>() }.context("missing native Vulkan device")?;
    let device = hal.raw_device().clone();
    let instance = hal.shared_instance().raw_instance().clone();
    let properties =
        unsafe { instance.get_physical_device_memory_properties(hal.raw_physical_device()) };
    let family = hal.queue_family_index();
    let queue = hal.raw_queue();
    // Import support alone does not imply this exact image can be exported.
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
            hal.raw_physical_device(),
            &query,
            &mut image_properties,
        )
    }
    .context("linear BGRA DMA-BUF export/import is unsupported on this adapter")?;
    ensure!(
        external_properties
            .external_memory_properties
            .external_memory_features
            .contains(
                vk::ExternalMemoryFeatureFlags::IMPORTABLE
                    | vk::ExternalMemoryFeatureFlags::EXPORTABLE
            ),
        "linear BGRA fixture requires both DMA-BUF export and import support"
    );
    let mut export = Exporter {
        _host: host.device.clone(),
        device,
        image: vk::Image::null(),
        memory: vk::DeviceMemory::null(),
        staging: vk::Buffer::null(),
        staging_memory: vk::DeviceMemory::null(),
        pool: vk::CommandPool::null(),
        fence: vk::Fence::null(),
        submitted: false,
    };
    let dimensions = size(DevicePixels(64), DevicePixels(32));
    let extent = vk::Extent3D {
        width: 64,
        height: 32,
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
        "fixture selected a non-linear modifier"
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

    let mut pixels = vec![0u8; 64 * 32 * 4];
    for y in 0..32 {
        for x in 0..64 {
            let bgra = match (x < 32, y < 16) {
                (true, true) => [0, 0, 255, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [255, 0, 0, 255],
                (false, false) => [255; 4],
            };
            pixels[(y * 64 + x) * 4..(y * 64 + x + 1) * 4].copy_from_slice(&bgra);
        }
    }
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
    Ok((export, frame))
}

fn scene(frame: &DmaBufFrame) -> Scene {
    let mut scene = Scene::default();
    for (x, opacity, clip_width) in [(8., 1., 64.), (88., 0.5, 48.)] {
        let bounds = Bounds::new(
            point(ScaledPixels(x), ScaledPixels(8.)),
            size(ScaledPixels(64.), ScaledPixels(32.)),
        );
        scene.insert_primitive(gpui::CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask {
                bounds: Bounds::new(
                    bounds.origin,
                    size(ScaledPixels(clip_width), bounds.size.height),
                ),
            },
            corner_radii: Corners::all(ScaledPixels(8.)),
            opacity,
            content: Arc::new(frame.clone()),
            coverage: None,
        });
    }
    scene.finish();
    scene
}
fn pixel(image: &image::RgbaImage, x: u32, y: u32, rgb: [u8; 3]) {
    let actual = image.get_pixel(x, y).0;
    for channel in 0..3 {
        assert!(
            actual[channel].abs_diff(rgb[channel]) <= 2,
            "pixel ({x},{y}): {actual:?}, expected {rgb:?}"
        );
    }
}

#[test]
#[ignore = "requires Linux Vulkan DMA-BUF export/import; run the dma-buf-native-test recipe"]
fn dma_buf_native_generated_snapshot() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    eprintln!(
        "DMA-BUF native fixture adapter: {:?}",
        host.adapter.get_info()
    );
    let validation = Validation::new(&host)?;
    let (exporter, frame) = generate(&host)?;
    drop(exporter); // The exported FD, then Vulkan imports, retain the actual payload.
    assert_eq!(frame.import_count(), 0);
    {
        let abandoned = WgpuCustomContext::new(host.device.clone(), &host.queue);
        let output = frame.prepare(&abandoned, size(DevicePixels(64), DevicePixels(32)))?;
        drop(output);
        // No acquire command is submitted; the next preparation must import anew.
    }
    let mut renderer = crate::WgpuHeadlessRenderer::new()?;
    let scene = scene(&frame);
    let image =
        renderer.render_scene_to_image(&scene, size(DevicePixels(160), DevicePixels(64)))?;
    pixel(&image, 24, 16, [255, 0, 0]);
    pixel(&image, 56, 16, [0, 255, 0]);
    pixel(&image, 24, 32, [0, 0, 255]);
    pixel(&image, 56, 32, [255; 3]);
    pixel(&image, 8, 8, [0; 3]);
    pixel(&image, 104, 16, [128, 0, 0]);
    pixel(&image, 144, 20, [0; 3]);
    assert_eq!(
        frame.import_count(),
        1,
        "placements must share one accepted import"
    );
    let png = std::env::temp_dir().join("media-lab-dma-buf.png");
    image.save(&png)?;
    let larger =
        renderer.render_scene_to_image(&scene, size(DevicePixels(320), DevicePixels(128)))?;
    pixel(&larger, 24, 16, [255, 0, 0]);
    assert_eq!(
        frame.import_count(),
        1,
        "resize must reuse the source texture"
    );
    let mut foreign = WgpuCustomContext::new(host.device.clone(), &host.queue);
    foreign.key = foreign.key.wrapping_add(1);
    assert!(frame.prepare(&foreign, frame.size()).is_err());
    assert!(frame.prepare(&(), frame.size()).is_err());
    renderer.render_scene(&scene, size(DevicePixels(160), DevicePixels(64)))?;
    drop(scene);
    drop(frame); // Retire all producer snapshots while sampling is queued.
    host.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(10)),
    })?;
    renderer.render_scene_to_image(&Scene::default(), size(DevicePixels(16), DevicePixels(16)))?;

    // Preparation may submit its raw ownership barrier before a later render
    // failure. Its completion guard must protect the texture with no scene left.
    let (exporter, transient) = generate(&host)?;
    drop(exporter);
    let preparation = WgpuCustomContext::new(host.device.clone(), &host.queue);
    let output = transient.prepare(&preparation, transient.size())?;
    preparation.submit()?;
    drop(output);
    drop(preparation);
    drop(transient);
    host.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(10)),
    })?;
    renderer.render_scene_to_image(&Scene::default(), size(DevicePixels(16), DevicePixels(16)))?;
    drop(renderer);
    host.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(10)),
    })?;
    validation.check()?;
    eprintln!(
        "Saved {}: generated DMA-BUF import, pixels, clipping, corners, opacity, resize/cache and teardown checks passed",
        png.display()
    );
    Ok(())
}
