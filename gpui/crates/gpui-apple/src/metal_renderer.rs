use crate::metal_atlas::MetalAtlas;
use anyhow::{Context as _, Result, ensure};
use block2::RcBlock;
use core_graphics::geometry::CGSize;
use gpui::{
    AtlasTextureId, Background, Bounds, ContentMask, DevicePixels, PaintSurface, Path, Point,
    PrimitiveBatch, ScaledPixels, Scene, Size, point, size,
};
#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
use image::RgbaImage;
use objc2::Message;
use objc2::runtime::AnyObject;
use objc2_metal::MTLTexture as _;

use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{
    CAMetalLayer, CommandQueue, MTLGPUFamily, MTLPixelFormat, MTLResourceOptions, NSRange,
    NSUInteger,
};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBufferGetHeight,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferGetWidthOfPlane, kCVPixelFormatType_32BGRA,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, kCVReturnSuccess,
};
use parking_lot::Mutex;

use std::{cell::Cell, ffi::c_void, mem, mem::MaybeUninit, ops::Range, ptr, slice, sync::Arc};

mod backdrop;
#[cfg(target_os = "macos")]
mod wide_sdr;

// Exported to metal
pub(crate) type PointF = gpui::Point<f32>;

#[cfg(not(feature = "runtime_shaders"))]
const SHADERS_METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shaders.metallib"));
#[cfg(feature = "runtime_shaders")]
const SHADERS_SOURCE_FILE: &str = include_str!(concat!(env!("OUT_DIR"), "/stitched_shaders.metal"));
// Use 4x MSAA, all devices support it.
// https://developer.apple.com/documentation/metal/mtldevice/1433355-supportstexturesamplecount
const PATH_SAMPLE_COUNT: u32 = 4;
/// Metal requires the offset a buffer is bound at to be 256-byte aligned.
const INSTANCE_BUFFER_ALIGNMENT: usize = 256;
const MAX_INSTANCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;

pub type Context = Arc<Mutex<InstanceBufferPool>>;
pub type Renderer = MetalRenderer;

pub unsafe fn new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: gpui::Size<f32>,
    transparent: bool,
) -> Renderer {
    MetalRenderer::new(context, transparent)
}

pub struct InstanceBufferPool {
    buffer_size: usize,
    buffers: Vec<metal::Buffer>,
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self {
            buffer_size: 2 * 1024 * 1024,
            buffers: Vec::new(),
        }
    }
}

pub(crate) struct InstanceBuffer {
    metal_buffer: metal::Buffer,
    size: usize,
}

impl InstanceBufferPool {
    pub(crate) fn reset(&mut self, buffer_size: usize) {
        self.buffer_size = buffer_size;
        self.buffers.clear();
    }

    pub(crate) fn acquire(
        &mut self,
        device: &metal::Device,
        unified_memory: bool,
    ) -> InstanceBuffer {
        let buffer = self.buffers.pop().unwrap_or_else(|| {
            let options = if unified_memory {
                MTLResourceOptions::StorageModeShared
                    // Buffers are write only which can benefit from the combined cache
                    // https://developer.apple.com/documentation/metal/mtlresourceoptions/cpucachemodewritecombined
                    | MTLResourceOptions::CPUCacheModeWriteCombined
            } else {
                MTLResourceOptions::StorageModeManaged
            };

            device.new_buffer(self.buffer_size as u64, options)
        });
        InstanceBuffer {
            metal_buffer: buffer,
            size: self.buffer_size,
        }
    }

    pub(crate) fn release(&mut self, buffer: InstanceBuffer) {
        if buffer.size == self.buffer_size {
            self.buffers.push(buffer.metal_buffer)
        }
    }
}

pub struct MetalRenderer {
    #[cfg(target_os = "macos")]
    wide_sdr: Option<wide_sdr::Compositor>,
    custom_compositor: Option<crate::custom_draw::CustomCompositor>,
    shader_pipelines: std::collections::HashMap<u64, metal::RenderPipelineState>,
    backdrops: Option<backdrop::Backdrops>,
    device: metal::Device,
    layer: Option<metal::MetalLayer>,
    is_apple_gpu: bool,
    is_unified_memory: bool,
    presents_with_transaction: bool,
    /// For headless rendering, tracks whether output should be opaque
    opaque: bool,
    command_queue: CommandQueue,
    paths_rasterization_pipeline_state: metal::RenderPipelineState,
    path_sprites_pipeline_state: metal::RenderPipelineState,
    shadows_pipeline_state: metal::RenderPipelineState,
    quads_pipeline_state: metal::RenderPipelineState,
    underlines_pipeline_state: metal::RenderPipelineState,
    monochrome_sprites_pipeline_state: metal::RenderPipelineState,
    polychrome_sprites_pipeline_state: metal::RenderPipelineState,
    surfaces_pipeline_state: metal::RenderPipelineState,
    bgra_surfaces_pipeline_state: metal::RenderPipelineState,
    unit_vertices: metal::Buffer,
    #[allow(clippy::arc_with_non_send_sync)]
    instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    sprite_atlas: Arc<MetalAtlas>,
    core_video_texture_cache: CFRetained<CVMetalTextureCache>,
    path_intermediate_texture: Option<metal::Texture>,
    path_intermediate_msaa_texture: Option<metal::Texture>,
    path_sample_count: u32,
    /// Offscreen render target reused across `render_scene` calls when
    /// rendering headlessly without reading pixels back.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    headless_render_target: Option<metal::Texture>,
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    /// Creates a new MetalRenderer with a CAMetalLayer for window-based rendering.
    pub fn new(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>, transparent: bool) -> Self {
        let device = Self::create_device();
        let layer = metal::MetalLayer::new();
        Self::configure_layer(&layer, &device, transparent);
        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    /// Negotiate color presentation for this renderer only. Existing UI colors
    /// retain sRGB meaning. Unsupported requests preserve its configuration.
    pub fn set_color_mode(
        &mut self,
        mode: gpui::WindowColorMode,
    ) -> Result<gpui::WindowColorStatus> {
        match mode {
            gpui::WindowColorMode::Default => {
                #[cfg(target_os = "macos")]
                {
                    if self.wide_sdr.is_some() {
                        if let Some(layer) = &self.layer {
                            layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                            let native: &objc2_quartz_core::CAMetalLayer =
                                unsafe { &*layer.as_ptr().cast() };
                            native.setColorspace(None);
                            native.setWantsExtendedDynamicRangeContent(false);
                        }
                        self.wide_sdr = None;
                    }
                }
                Ok(self.color_status())
            }
            #[cfg(target_os = "macos")]
            gpui::WindowColorMode::WideGamutSdr => {
                anyhow::ensure!(self.opaque, "wide-gamut SDR requires an opaque window");
                if self.wide_sdr.is_none() {
                    let compositor = wide_sdr::Compositor::new(&self.device)?;
                    if let Some(layer) = &self.layer {
                        wide_sdr::configure_layer(layer)?;
                    }
                    self.wide_sdr = Some(compositor);
                }
                Ok(self.color_status())
            }
            _ => anyhow::bail!("unsupported color presentation mode"),
        }
    }

    /// Inspect the negotiated renderer configuration. Physical display evidence
    /// is supplied by the windowing backend; headless renderers report unknown.
    pub fn color_status(&self) -> gpui::WindowColorStatus {
        #[cfg(target_os = "macos")]
        {
            let mode = if self.wide_sdr.is_some() {
                gpui::WindowColorMode::WideGamutSdr
            } else {
                gpui::WindowColorMode::Default
            };
            gpui::WindowColorStatus::new(mode, mode, true, None, None)
        }
        #[cfg(not(target_os = "macos"))]
        gpui::WindowColorStatus::default()
    }

    /// Creates a renderer for a CAMetalLayer owned by a platform view, such as
    /// the backing layer UIKit creates for a view whose `layerClass` is
    /// `CAMetalLayer`. The renderer retains the layer.
    pub fn from_layer(
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
        layer: &objc2_quartz_core::CAMetalLayer,
        transparent: bool,
    ) -> Self {
        let device = Self::create_device();
        // Both types bind the same Objective-C class, so this only changes which
        // Rust wrapper views the live layer. `to_owned` retains it.
        let layer = unsafe {
            metal::MetalLayerRef::from_ptr(ptr::from_ref(layer).cast_mut().cast::<CAMetalLayer>())
        }
        .to_owned();
        Self::configure_layer(&layer, &device, transparent);
        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    fn configure_layer(layer: &metal::MetalLayerRef, device: &metal::DeviceRef, transparent: bool) {
        layer.set_device(device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        // Support direct-to-display rendering if the window is not transparent
        // https://developer.apple.com/documentation/metal/managing-your-game-window-for-metal-in-macos
        layer.set_opaque(!transparent);
        layer.set_maximum_drawable_count(3);
        // Allow texture reading for visual tests and UI automation screenshots
        // (captures without ScreenCaptureKit). Only debug builds pay the
        // presentation cost, even when `test-support` is compiled in.
        #[cfg(all(feature = "test-support", debug_assertions))]
        layer.set_framebuffer_only(false);
        // metal-rs doesn't bind these setters, so view the same object through
        // objc2's typed CAMetalLayer binding.
        let objc2_layer: &objc2_quartz_core::CAMetalLayer = unsafe { &*layer.as_ptr().cast() };
        objc2_layer.setAllowsNextDrawableTimeout(false);
        objc2_layer.setNeedsDisplayOnBoundsChange(true);
        // UIKit sizes a view's backing layer itself; only AppKit-hosted
        // layers need to track their superlayer's bounds.
        #[cfg(target_os = "macos")]
        objc2_layer.setAutoresizingMask(
            objc2_quartz_core::CAAutoresizingMask::LayerWidthSizable
                | objc2_quartz_core::CAAutoresizingMask::LayerHeightSizable,
        );
    }

    /// Creates a new headless MetalRenderer for offscreen rendering without a window.
    ///
    /// This renderer can render scenes to images without requiring a CAMetalLayer,
    /// window, or AppKit. Use `render_scene_to_image()` to render scenes.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn new_headless(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Self {
        let device = Self::create_device();
        Self::new_internal(device, None, true, instance_buffer_pool)
    }

    #[cfg(target_os = "macos")]
    fn create_device() -> metal::Device {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        if let Some(d) = metal::Device::all()
            .into_iter()
            .min_by_key(|d| (d.is_removable(), !d.is_low_power()))
        {
            d
        } else {
            // For some reason `all()` can return an empty list, see https://github.com/zed-industries/zed/issues/37689
            // In that case, we fall back to the system default device.
            log::error!(
                "Unable to enumerate Metal devices; attempting to use system default device"
            );
            Self::system_default_device()
        }
    }

    #[cfg(target_os = "ios")]
    fn create_device() -> metal::Device {
        Self::system_default_device()
    }

    fn system_default_device() -> metal::Device {
        metal::Device::system_default().unwrap_or_else(|| {
            log::error!("unable to access a compatible graphics device");
            std::process::exit(1);
        })
    }

    fn new_internal(
        device: metal::Device,
        layer: Option<metal::MetalLayer>,
        opaque: bool,
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    ) -> Self {
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .new_library_with_source(&SHADERS_SOURCE_FILE, &metal::CompileOptions::new())
            .expect("error building metal library");
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .new_library_with_data(SHADERS_METALLIB)
            .expect("error building metal library");

        fn to_float2_bits(point: PointF) -> u64 {
            let mut output = point.y.to_bits() as u64;
            output <<= 32;
            output |= point.x.to_bits() as u64;
            output
        }

        // Shared memory can be used only if CPU and GPU share the same memory space.
        // https://developer.apple.com/documentation/metal/setting-resource-storage-modes
        // iOS does not support managed resources. Its simulator may report a
        // non-unified host GPU even though resources must still use shared storage.
        let is_unified_memory = cfg!(target_os = "ios") || device.has_unified_memory();
        // Apple GPU families support memoryless textures, which can significantly reduce
        // memory usage by keeping render targets in on-chip tile memory instead of
        // allocating backing store in system memory.
        // https://developer.apple.com/documentation/metal/mtlgpufamily
        let is_apple_gpu = device.supports_family(MTLGPUFamily::Apple1);

        let unit_vertices = [
            to_float2_bits(point(0., 0.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(1., 1.)),
        ];
        let unit_vertices = device.new_buffer_with_data(
            unit_vertices.as_ptr() as *const c_void,
            mem::size_of_val(&unit_vertices) as u64,
            if is_unified_memory {
                MTLResourceOptions::StorageModeShared
                    | MTLResourceOptions::CPUCacheModeWriteCombined
            } else {
                MTLResourceOptions::StorageModeManaged
            },
        );

        let paths_rasterization_pipeline_state = build_path_rasterization_pipeline_state(
            &device,
            &library,
            "paths_rasterization",
            "path_rasterization_vertex",
            "path_rasterization_fragment",
            MTLPixelFormat::BGRA8Unorm,
            PATH_SAMPLE_COUNT,
        );
        let path_sprites_pipeline_state = build_path_sprite_pipeline_state(
            &device,
            &library,
            "path_sprites",
            "path_sprite_vertex",
            "path_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let shadows_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "shadows",
            "shadow_vertex",
            "shadow_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let quads_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "quads",
            "quad_vertex",
            "quad_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let underlines_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "underlines",
            "underline_vertex",
            "underline_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let monochrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "monochrome_sprites",
            "monochrome_sprite_vertex",
            "monochrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let polychrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "polychrome_sprites",
            "polychrome_sprite_vertex",
            "polychrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "surfaces",
            "surface_vertex",
            "surface_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );

        let bgra_surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "bgra_surfaces",
            "surface_vertex",
            "surface_bgra_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let command_queue = device.new_command_queue();
        let supports_shared_storage = cfg!(target_os = "ios") || is_apple_gpu;
        let sprite_atlas = Arc::new(MetalAtlas::new(
            crate::metal_compat::device(&device).retain(),
            supports_shared_storage,
        ));
        let core_video_texture_cache = unsafe {
            let mut cache = ptr::null_mut();
            let status = CVMetalTextureCache::create(
                None,
                None,
                crate::metal_compat::device(&device),
                None,
                ptr::NonNull::from(&mut cache),
            );
            assert_eq!(
                status, kCVReturnSuccess,
                "failed to create CoreVideo Metal texture cache"
            );
            CFRetained::from_raw(ptr::NonNull::new(cache).expect("CoreVideo returned a null cache"))
        };

        Self {
            #[cfg(target_os = "macos")]
            wide_sdr: None,
            custom_compositor: None,
            shader_pipelines: Default::default(),
            backdrops: None,
            device,
            layer,
            presents_with_transaction: false,
            is_apple_gpu,
            is_unified_memory,
            opaque,
            command_queue,
            paths_rasterization_pipeline_state,
            path_sprites_pipeline_state,
            shadows_pipeline_state,
            quads_pipeline_state,
            underlines_pipeline_state,
            monochrome_sprites_pipeline_state,
            polychrome_sprites_pipeline_state,
            surfaces_pipeline_state,
            bgra_surfaces_pipeline_state,
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
            core_video_texture_cache,
            path_intermediate_texture: None,
            path_intermediate_msaa_texture: None,
            path_sample_count: PATH_SAMPLE_COUNT,
            #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
            headless_render_target: None,
        }
    }

    pub fn layer(&self) -> Option<&metal::MetalLayerRef> {
        self.layer.as_ref().map(|l| l.as_ref())
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        self.layer
            .as_ref()
            .map(|l| l.as_ptr())
            .unwrap_or(ptr::null_mut())
    }

    pub fn sprite_atlas(&self) -> &Arc<MetalAtlas> {
        &self.sprite_atlas
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        if let Some(layer) = &self.layer {
            layer.set_presents_with_transaction(presents_with_transaction);
        }
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        if let Some(layer) = &self.layer {
            layer.set_drawable_size(CGSize::new(size.width.0 as f64, size.height.0 as f64));
        }
        self.update_path_intermediate_textures(size);
    }

    fn update_path_intermediate_textures(&mut self, size: Size<DevicePixels>) {
        // We are uncertain when this happens, but sometimes size can be 0 here. Most likely before
        // the layout pass on window creation. Zero-sized texture creation causes SIGABRT.
        // https://github.com/zed-industries/zed/issues/36229
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            return;
        }

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);
        texture_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.path_intermediate_texture = Some(self.device.new_texture(&texture_descriptor));

        if self.path_sample_count > 1 {
            // https://developer.apple.com/documentation/metal/choosing-a-resource-storage-mode-for-apple-gpus
            // Rendering MSAA textures are done in a single pass, so we can use memory-less storage on Apple Silicon
            let storage_mode = if self.is_apple_gpu {
                metal::MTLStorageMode::Memoryless
            } else {
                metal::MTLStorageMode::Private
            };

            let msaa_descriptor = texture_descriptor;
            msaa_descriptor.set_texture_type(metal::MTLTextureType::D2Multisample);
            msaa_descriptor.set_storage_mode(storage_mode);
            msaa_descriptor.set_sample_count(self.path_sample_count as _);
            self.path_intermediate_msaa_texture = Some(self.device.new_texture(&msaa_descriptor));
        } else {
            self.path_intermediate_msaa_texture = None;
        }
    }

    /// Update layer opacity. Transparent requests during wide-SDR mode are
    /// ignored with a warning; disable the mode first to preserve valid state.
    pub fn update_transparency(&mut self, transparent: bool) {
        if transparent && self.color_status().requested() == gpui::WindowColorMode::WideGamutSdr {
            log::warn!("disable wide-gamut SDR before enabling transparency");
            return;
        }
        self.opaque = !transparent;
        if let Some(layer) = &self.layer {
            layer.set_opaque(!transparent);
        }
    }

    pub fn destroy(&self) {
        // nothing to do
    }

    pub fn draw(&mut self, scene: &Scene) {
        let layer = match &self.layer {
            Some(l) => l.clone(),
            None => {
                log::error!(
                    "draw() called on headless renderer - use render_scene_to_image() instead"
                );
                return;
            }
        };
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = if let Some(drawable) = layer.next_drawable() {
            drawable
        } else {
            log::error!(
                "failed to retrieve next drawable, drawable size: {:?}",
                viewport_size
            );
            return;
        };

        let command_buffer = match self.render_frame(scene, drawable.texture(), viewport_size) {
            Ok(command_buffer) => command_buffer,
            Err(error) => {
                log::error!("failed to render: {error:#}");
                return;
            }
        };

        if self.presents_with_transaction {
            command_buffer.commit();
            command_buffer.wait_until_scheduled();
            drawable.present();
        } else {
            command_buffer.present_drawable(drawable);
            command_buffer.commit();
        }
    }

    fn render_frame(
        &mut self,
        scene: &Scene,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        if scene.backdrops.is_empty() && scene.layer_boundaries.is_empty() {
            if let Some(effects) = &mut self.backdrops {
                effects.clear();
            }
        }

        #[cfg(target_os = "macos")]
        if let Some(lab) = &mut self.wide_sdr {
            wide_sdr::validate_viewport(viewport_size)?;
            anyhow::ensure!(self.opaque, "wide SDR requires an opaque top-level window");
            lab.wide.clear();
        }
        #[cfg(target_os = "macos")]
        anyhow::ensure!(self.wide_sdr.is_some() || scene.shader_draws.iter().all(|d|
            d.program.output_encoding() == gpui::ShaderOutputEncoding::EncodedSrgb),
            "linear P3 shaders require WideGamutSdr");
        #[cfg(not(target_os = "macos"))]
        anyhow::ensure!(
            scene
                .shader_draws
                .iter()
                .all(|d| d.program.output_encoding() == gpui::ShaderOutputEncoding::EncodedSrgb),
            "linear P3 shaders require a native wide-SDR compositor"
        );
        let custom_outputs = if scene.custom_draws.is_empty() {
            Vec::new()
        } else {
            if self.custom_compositor.is_none() {
                self.custom_compositor =
                    Some(crate::custom_draw::CustomCompositor::new(&self.device)?);
            }
            let context = crate::custom_draw::MetalCustomContext {
                device: self.device.clone(),
                queue: self.command_queue.clone(),
            };
            let mut outputs = Vec::new();
            let preparation_frame = gpui::PreparationFrame::default();
            for draw in &scene.custom_draws {
                let extent = crate::custom_draw::CustomCompositor::extent(draw)?;
                // Direct scene records use 1x logical geometry. Window records
                // retain their original logical size/scale in a private adapter.
                let request = gpui::PreparationRequest::new(
                    draw.bounds.size.map(|dimension| gpui::px(dimension.0)),
                    extent,
                    1.,
                )?;
                let raw = draw
                    .content
                    .prepare_in_frame(&context, &request, &preparation_frame)?;
                let native_source = raw.is::<Background>();
                let raw = if native_source {
                    let background = *raw.downcast::<Background>().unwrap();
                    let target = crate::custom_draw::texture(&self.device, extent)?;
                    let bounds = Bounds::new(
                        point(ScaledPixels(0.), ScaledPixels(0.)),
                        size(
                            ScaledPixels(extent.width.0 as f32),
                            ScaledPixels(extent.height.0 as f32),
                        ),
                    );
                    let mut native = Scene::default();
                    native.insert_primitive(gpui::Quad {
                        bounds,
                        content_mask: ContentMask { bounds },
                        background,
                        ..Default::default()
                    });
                    native.finish();
                    self.render_paint_mask(&native, &target, extent, viewport_size)?;
                    Box::new(crate::custom_draw::MetalCustomOutput {
                        texture: target,
                        commands: Vec::new(),
                    }) as Box<dyn std::any::Any>
                } else {
                    raw
                };
                let output = if raw.is::<crate::custom_draw::MetalWideSdrOutput>() {
                    #[cfg(target_os = "macos")]
                    {
                        anyhow::ensure!(
                            self.wide_sdr.is_some(),
                            "wide output requires WideGamutSdr presentation"
                        );
                        let output = *raw
                            .downcast::<crate::custom_draw::MetalWideSdrOutput>()
                            .unwrap();
                        output.validate_context(&context)?;
                        let output = crate::custom_draw::CustomCompositor::validate_format(
                            &context,
                            draw,
                            Box::new(output.into_output()),
                            MTLPixelFormat::RGBA16Float,
                        )?;
                        self.wide_sdr.as_mut().unwrap().wide.insert(outputs.len());
                        output.into()
                    }
                    #[cfg(not(target_os = "macos"))]
                    anyhow::bail!("wide-gamut SDR presentation is unsupported")
                } else if raw.is::<crate::custom_draw::MetalNv12Output>() {
                    crate::custom_draw::CustomCompositor::validate_nv12(&context, raw)?
                } else {
                    let output =
                        crate::custom_draw::CustomCompositor::validate(&context, draw, raw)?;
                    if draw.content.image_color_mode() == gpui::ImageColorMode::LinearP3Wide {
                        anyhow::ensure!(
                            self.wide_sdr.is_some(),
                            "wide image requires WideGamutSdr presentation"
                        );
                        self.wide_sdr.as_mut().unwrap().wide.insert(outputs.len());
                    }
                    output.into()
                };
                let coverage = if draw.coverage.is_some() {
                    let key = crate::custom_draw::CoverageKey::new(draw)?;
                    if let Some(texture) = self.custom_compositor.as_ref().unwrap().masks.get(&key)
                    {
                        Some(texture.clone())
                    } else {
                        let target = crate::custom_draw::texture(&self.device, extent)?;
                        let mask_scene = crate::custom_draw::coverage_scene(draw, extent)?;
                        self.render_paint_mask(&mask_scene, &target, extent, viewport_size)?;
                        let bytes =
                            extent.width.0 as usize * extent.height.0 as usize * 4 + key.byte_len();
                        let compositor = self.custom_compositor.as_mut().unwrap();
                        if compositor.mask_bytes + bytes > 64 * 1024 * 1024 {
                            compositor.masks.clear();
                            compositor.mask_bytes = 0;
                        }
                        if bytes <= 64 * 1024 * 1024 {
                            compositor.mask_bytes += bytes;
                            compositor.masks.insert(&key, target.clone());
                        }
                        Some(target)
                    }
                } else {
                    None
                };
                outputs.push((output, coverage, native_source));
            }
            crate::custom_draw::submit_custom_commands(
                &context,
                outputs.iter().map(|(output, _, _)| &output.output),
            )?;
            outputs
        };
        // Coverage scenes contain no shaders and must not evict main-scene pipelines.
        if !scene.shader_draws.is_empty() {
            self.shader_pipelines
                .retain(|id, _| scene.shader_draws.iter().any(|d| d.program.id() == *id));
        }
        for draw in &scene.shader_draws {
            draw.validate_masks()?;
            if !self.shader_pipelines.contains_key(&draw.program.id()) {
                self.shader_pipelines.insert(
                    draw.program.id(),
                    create_shader_pipeline(&self.device, &draw.program)?,
                );
            }
        }
        let mut writer = InstanceBufferWriter::new(
            &self.device,
            &self.instance_buffer_pool,
            self.is_unified_memory,
        );
        let instance_bindings = write_instances(scene, &mut writer).with_context(|| {
            format!(
                "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces",
                scene.paths.len(),
                scene.shadows.len(),
                scene.quads.len(),
                scene.underlines.len(),
                scene.monochrome_sprites.len(),
                scene.polychrome_sprites.len(),
                scene.surfaces.len(),
            )
        })?;
        let command_buffer = self.draw_primitives_to_texture(
            scene,
            &instance_bindings,
            &mut writer,
            texture,
            viewport_size,
            &custom_outputs,
        )?;

        // External producers may own CoreVideo/IOSurface pool slots. Keep their
        // complete snapshots (not only Metal views) alive through sampling.
        let content_lifetimes = Cell::new(Some(
            scene
                .custom_draws
                .iter()
                .map(|draw| draw.content.clone())
                .collect::<Vec<_>>(),
        ));
        let surface_lifetimes = Cell::new(Some(
            scene
                .surfaces
                .iter()
                .map(|surface| surface.image_buffer.clone())
                .collect::<Vec<_>>(),
        ));
        let instance_buffer_pool = self.instance_buffer_pool.clone();
        let instance_buffer = Cell::new(Some(writer.finish()));
        let block = RcBlock::new(move |_: ptr::NonNull<AnyObject>| {
            drop(content_lifetimes.take());
            drop(surface_lifetimes.take());
            if let Some(instance_buffer) = instance_buffer.take() {
                instance_buffer_pool.lock().release(instance_buffer);
            }
        });
        // SAFETY: Both pointee types are opaque views of the same Objective-C block pointer ABI.
        unsafe {
            command_buffer.add_completed_handler(&*RcBlock::as_ptr(&block).cast());
        }

        Ok(command_buffer)
    }

    /// Renders the scene to a texture and returns the pixel data as an RGBA image.
    /// This does not present the frame to screen - useful for visual testing
    /// where we want to capture what would be rendered without displaying it.
    ///
    /// Note: This requires a layer-backed renderer. For headless rendering,
    /// use `render_scene_to_image()` instead.
    #[cfg(any(test, feature = "test-support"))]
    pub fn render_to_image(&mut self, scene: &Scene) -> Result<RgbaImage> {
        let layer = self
            .layer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("render_to_image requires a layer-backed renderer"))?;
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        #[cfg(target_os = "macos")]
        if self.wide_sdr.is_some() {
            // Existing image API returns an explicit clipped sRGB preview, not
            // four-byte reads from a Float16 drawable.
            return self.render_scene_to_image(scene, viewport_size);
        }
        let drawable = layer
            .next_drawable()
            .ok_or_else(|| anyhow::anyhow!("Failed to get drawable for render_to_image"))?;

        let command_buffer = self.render_frame(scene, drawable.texture(), viewport_size)?;

        // Commit and wait for completion without presenting
        command_buffer.commit();
        command_buffer.wait_until_completed();

        read_texture_to_image(drawable.texture())
    }

    /// Renders a scene to an image without requiring a window or CAMetalLayer.
    ///
    /// This is the primary method for headless rendering. It creates an offscreen
    /// texture, renders the scene to it, and returns the pixel data as an RGBA image.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<RgbaImage> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene_to_image: {:?}", size);
        }

        #[cfg(target_os = "macos")]
        if self.wide_sdr.is_some() {
            wide_sdr::validate_viewport(size)?;
        }

        // Headless callers do not have a Cocoa event-loop pool to release
        // autoreleased command buffers and render-pass descriptors.
        objc2::rc::autoreleasepool(|_| {
            // Update path intermediate textures for this size
            self.update_path_intermediate_textures(size);

            // Create an offscreen texture as render target
            let texture_descriptor = metal::TextureDescriptor::new();
            texture_descriptor.set_width(size.width.0 as u64);
            texture_descriptor.set_height(size.height.0 as u64);
            texture_descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            texture_descriptor.set_usage(
                metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
            );
            // Like the atlas, only Apple GPUs can create shared textures on macOS;
            // Intel Macs cannot, even with unified memory. iOS has no managed storage.
            let uses_shared_storage = cfg!(target_os = "ios") || self.is_apple_gpu;
            texture_descriptor.set_storage_mode(if uses_shared_storage {
                metal::MTLStorageMode::Shared
            } else {
                metal::MTLStorageMode::Managed
            });
            let target_texture = self.device.new_texture(&texture_descriptor);

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // Managed textures require an explicit blit synchronize before the CPU
            // can read back the rendered data. Without this, get_bytes returns
            // stale zeros.
            if !uses_shared_storage {
                let blit = command_buffer.new_blit_command_encoder();
                blit.synchronize_resource(&target_texture);
                blit.end_encoding();
            }

            // Commit and wait for completion
            command_buffer.commit();
            command_buffer.wait_until_completed();

            read_texture_to_image(&target_texture)
        })
    }

    /// Renders a scene to a reused offscreen texture without reading pixels
    /// back or blocking on GPU completion.
    ///
    /// This mirrors the CPU cost of presenting a frame to a window (scene
    /// encoding, instance buffer writes, command submission) and is used by
    /// headless benchmark rendering, where the produced pixels are never
    /// inspected.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> Result<()> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene: {:?}", size);
        }

        #[cfg(target_os = "macos")]
        if self.wide_sdr.is_some() {
            wide_sdr::validate_viewport(size)?;
        }
        objc2::rc::autoreleasepool(|_| {
            self.update_path_intermediate_textures(size);

            let needs_new_target = self.headless_render_target.as_ref().is_none_or(|texture| {
                texture.width() != size.width.0 as u64 || texture.height() != size.height.0 as u64
            });
            if needs_new_target {
                let texture_descriptor = metal::TextureDescriptor::new();
                texture_descriptor.set_width(size.width.0 as u64);
                texture_descriptor.set_height(size.height.0 as u64);
                texture_descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                texture_descriptor.set_usage(
                    metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
                );
                texture_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
                self.headless_render_target = Some(self.device.new_texture(&texture_descriptor));
            }
            let target_texture = self
                .headless_render_target
                .clone()
                .expect("just ensured the render target exists");

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // Commit without waiting, mirroring presentation to a real window where
            // the CPU doesn't block on the GPU.
            command_buffer.commit();
            Ok(())
        })
    }

    // Mask scenes contain native primitives only. Restore presentation state even on error.
    fn render_paint_mask(
        &mut self,
        scene: &Scene,
        target: &metal::TextureRef,
        extent: Size<DevicePixels>,
        viewport: Size<DevicePixels>,
    ) -> Result<()> {
        let opaque = self.opaque;
        self.opaque = false;
        self.update_path_intermediate_textures(extent);
        #[cfg(target_os = "macos")]
        let lab = self.wide_sdr.take();
        let effects = self.backdrops.take();
        let result = self.render_frame(scene, target, extent);
        self.backdrops = effects;
        #[cfg(target_os = "macos")]
        {
            self.wide_sdr = lab;
        }
        self.opaque = opaque;
        self.update_path_intermediate_textures(viewport);
        result?.commit();
        Ok(())
    }

    fn draw_primitives_to_texture(
        &mut self,
        scene: &Scene,
        instance_bindings: &InstanceBindings,
        writer: &mut InstanceBufferWriter,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
        custom_outputs: &[(
            crate::custom_draw::PreparedMetalOutput,
            Option<metal::Texture>,
            bool,
        )],
    ) -> Result<metal::CommandBuffer> {
        #[cfg(target_os = "macos")]
        if let Some(mut lab) = self.wide_sdr.take() {
            if !scene.backdrops.is_empty()
                || !scene.layer_boundaries.is_empty()
                || scene.shader_draws.iter().any(|d| d.path_mask.is_some())
            {
                self.wide_sdr = Some(lab);
                anyhow::bail!("captured effects require SDR presentation");
            }
            let result = lab.render(
                self,
                scene,
                instance_bindings,
                writer,
                texture,
                viewport_size,
                custom_outputs,
            );
            self.wide_sdr = Some(lab);
            return result;
        }
        if let Some(program) = scene
            .backdrops
            .first()
            .map(|d| &d.program)
            .or_else(|| scene.layer_boundaries.first().map(|b| &b.capture.program))
        {
            anyhow::ensure!(
                texture.pixel_format() == metal::MTLPixelFormat::BGRA8Unorm,
                "backdrop blur requires BGRA8 SDR"
            );
            if self.backdrops.is_none() {
                self.backdrops = Some(backdrop::Backdrops::new(&self.device, program)?);
            }
            self.backdrops.as_mut().unwrap().prepare(
                &self.device,
                scene,
                viewport_size,
                &[self.opaque as u64],
            )?;
        }
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.opaque { 1. } else { 0. };

        let mut command_encoder = new_command_encoder_for_texture(
            command_buffer,
            texture,
            viewport_size,
            Some(metal::MTLClearColor::new(0., 0., 0., alpha)),
        );

        let mut executed_backdrops = Vec::new();
        let mut executed_groups = Vec::new();
        let mut skip_group_end = None;
        for batch in scene.batches() {
            if let Some(end) = skip_group_end {
                if !matches!(&batch,PrimitiveBatch::LayerBoundary(index) if *index==end) {
                    continue;
                }
                skip_group_end = None;
            }
            match batch {
                PrimitiveBatch::LayerBoundary(index) => {
                    command_encoder.end_encoding();
                    let effects = self.backdrops.as_mut().unwrap();
                    let (slot, begin) = effects.groups.boundary(index);
                    if begin {
                        effects.groups.start(slot);
                    }
                    let hit = effects.groups.hit(slot);
                    if begin {
                        executed_groups.push(slot);
                    }
                    if begin || !hit {
                        effects.groups.copy(command_buffer, texture, slot, begin);
                    }
                    if !begin {
                        effects.group_filter(command_buffer, slot);
                    }
                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        texture,
                        viewport_size,
                        None,
                    );
                    if begin && hit {
                        skip_group_end = Some(effects.groups.end(slot));
                    } else {
                        effects.group_draw(command_encoder, slot, begin);
                    }
                }

                PrimitiveBatch::Shaders { program_id, range } => {
                    if scene.shader_draws[range.start].path_mask.is_some() {
                        let paths: Vec<_> = scene.shader_draws[range.clone()]
                            .iter()
                            .map(|draw| draw.path_mask.as_ref().unwrap().as_ref().clone())
                            .collect();
                        command_encoder.end_encoding();
                        let did_draw = self.draw_paths_to_intermediate(
                            &paths,
                            writer,
                            viewport_size,
                            command_buffer,
                        )?;
                        command_encoder = new_command_encoder_for_texture(
                            command_buffer,
                            texture,
                            viewport_size,
                            None,
                        );
                        if !did_draw {
                            continue;
                        }
                    }
                    self.draw_shaders(
                        program_id,
                        range.clone(),
                        instance_bindings,
                        viewport_size,
                        scene.shader_draws[range.start].glyph_mask,
                        scene.shader_draws[range.start].path_mask.is_some(),
                        command_encoder,
                    );
                }
                PrimitiveBatch::Backdrop(index) => {
                    command_encoder.end_encoding();
                    let effect = self.backdrops.as_mut().unwrap();
                    executed_backdrops.push(index);
                    effect.filter(command_buffer, texture, index);
                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        texture,
                        viewport_size,
                        None,
                    );
                    effect.composite(command_encoder, index);
                }
                PrimitiveBatch::Custom(range) => {
                    for index in range {
                        self.custom_compositor.as_ref().unwrap().draw(
                            &scene.custom_draws[index],
                            &custom_outputs[index].0,
                            custom_outputs[index].1.as_deref(),
                            custom_outputs[index].2,
                            viewport_size,
                            command_encoder,
                        );
                    }
                }
                PrimitiveBatch::Shadows(range) => {
                    self.draw_shadows(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Quads(range) => {
                    self.draw_quads(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range];
                    command_encoder.end_encoding();

                    let did_draw = self.draw_paths_to_intermediate(
                        paths,
                        writer,
                        viewport_size,
                        command_buffer,
                    )?;

                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        texture,
                        viewport_size,
                        None,
                    );

                    if did_draw {
                        if let Err(error) = self.draw_paths_from_intermediate(
                            paths,
                            writer,
                            viewport_size,
                            command_encoder,
                        ) {
                            command_encoder.end_encoding();
                            return Err(error);
                        }
                    }
                }
                PrimitiveBatch::Underlines(range) => {
                    self.draw_underlines(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => self
                    .draw_monochrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::PolychromeSprites { texture_id, range } => self
                    .draw_polychrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::Surfaces(range) => {
                    if let Err(error) = self.draw_surfaces(
                        &scene.surfaces[range.clone()],
                        range.start,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                        command_buffer,
                    ) {
                        command_encoder.end_encoding();
                        return Err(error);
                    }
                }
                PrimitiveBatch::SubpixelSprites { .. } => unreachable!(),
            }
        }

        command_encoder.end_encoding();
        if let Some(effects) = &mut self.backdrops {
            effects.groups.commit(&executed_groups);
            effects.cache.commit(&executed_backdrops);
        }

        Ok(command_buffer.to_owned())
    }

    fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertex_instance_bindings = writer.write(&vertices)?;

        let render_pass_descriptor = metal::RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .unwrap();
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));

        if let Some(msaa_texture) = &self.path_intermediate_msaa_texture {
            color_attachment.set_texture(Some(msaa_texture));
            color_attachment.set_resolve_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.set_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
        }

        let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        command_encoder.set_render_pipeline_state(&self.paths_rasterization_pipeline_state);
        command_encoder.set_vertex_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            PathRasterizationInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.draw_primitives(
            metal::MTLPrimitiveType::Triangle,
            0,
            vertices.len() as u64,
        );

        command_encoder.end_encoding();
        Ok(true)
    }

    fn draw_shadows(
        &self,
        shadows: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if shadows.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.shadows_pipeline_state);
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            ShadowInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            shadows.len() as u64,
            shadows.start as u64,
        );
    }

    fn draw_shaders(
        &self,
        program_id: u64,
        range: Range<usize>,
        bindings: &InstanceBindings,
        viewport: Size<DevicePixels>,
        glyph_mask: Option<gpui::AtlasTile>,
        path_mask: bool,
        encoder: &metal::RenderCommandEncoderRef,
    ) {
        if let Some(tile) = glyph_mask {
            let Some(texture) = self.sprite_atlas.metal_texture(tile.texture_id) else {
                return;
            };
            encoder.set_fragment_texture(0, Some(crate::metal_compat::legacy_texture(&texture)));
        } else if path_mask {
            encoder.set_fragment_texture(0, self.path_intermediate_texture.as_deref());
        } else {
            encoder.set_fragment_texture(0, None);
        }
        encoder.set_render_pipeline_state(&self.shader_pipelines[&program_id]);
        encoder.set_vertex_buffer(
            0,
            Some(&bindings.shaders.buffer),
            (bindings.shaders.offset + range.start * 176) as u64,
        );
        // Metal glyph atlases are A8Unorm; portable/D3D atlases are R8.
        // The host wrapper selects alpha when pad=2 without changing the ABI.
        let globals = [
            viewport.width.0 as f32,
            viewport.height.0 as f32,
            0.,
            if glyph_mask.is_some() {
                f32::from_bits(2)
            } else {
                0.
            },
        ];
        encoder.set_vertex_bytes(1, 16, globals.as_ptr().cast());
        encoder.set_fragment_bytes(1, 16, globals.as_ptr().cast());
        encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            range.len() as u64,
        );
    }

    fn draw_quads(
        &self,
        quads: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if quads.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.quads_pipeline_state);
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            QuadInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            quads.len() as u64,
            quads.start as u64,
        );
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> Result<()> {
        let Some(first_path) = paths.first() else {
            return Ok(());
        };
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        command_encoder.set_render_pipeline_state(&self.path_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(intermediate_texture),
        );

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if paths.last().unwrap().order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        let sprite_instance_bindings = writer.write(&sprites)?;
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&sprite_instance_bindings.buffer),
            sprite_instance_bindings.offset as u64,
        );

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        Ok(())
    }

    fn draw_underlines(
        &self,
        underlines: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if underlines.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.underlines_pipeline_state);
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            UnderlineInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            underlines.len() as u64,
            underlines.start as u64,
        );
    }

    fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.monochrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(crate::metal_compat::legacy_texture(&texture)),
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.polychrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(crate::metal_compat::legacy_texture(&texture)),
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        first_surface: usize,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
        command_buffer: &metal::CommandBufferRef,
    ) -> Result<()> {
        if surfaces.is_empty() {
            return Ok(());
        }

        command_encoder.set_render_pipeline_state(&self.surfaces_pipeline_state);
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Surfaces as u64,
            Some(&instance_bindings.surfaces.buffer),
            instance_bindings.surfaces.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let mut views = Vec::new();
        for (index, surface) in surfaces.iter().enumerate() {
            let image_buffer = &surface.image_buffer;
            let texture_size = size(
                DevicePixels::from(CVPixelBufferGetWidth(image_buffer) as i32),
                DevicePixels::from(CVPixelBufferGetHeight(image_buffer) as i32),
            );

            let format = CVPixelBufferGetPixelFormatType(image_buffer);
            ensure!(
                format == kCVPixelFormatType_32BGRA
                    || format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                "unsupported CoreVideo surface format: {format}"
            );
            let bgra = format == kCVPixelFormatType_32BGRA;
            command_encoder.set_render_pipeline_state(if bgra {
                &self.bgra_surfaces_pipeline_state
            } else {
                &self.surfaces_pipeline_state
            });

            let plane_texture = |plane, format| unsafe {
                let mut texture = ptr::null_mut();
                let status = CVMetalTextureCache::create_texture_from_image(
                    None,
                    &self.core_video_texture_cache,
                    image_buffer,
                    None,
                    format,
                    if bgra {
                        CVPixelBufferGetWidth(image_buffer)
                    } else {
                        CVPixelBufferGetWidthOfPlane(image_buffer, plane)
                    },
                    if bgra {
                        CVPixelBufferGetHeight(image_buffer)
                    } else {
                        CVPixelBufferGetHeightOfPlane(image_buffer, plane)
                    },
                    plane,
                    ptr::NonNull::from(&mut texture),
                );
                ensure!(
                    status == kCVReturnSuccess,
                    "CoreVideo surface import failed: {status}"
                );
                Ok::<_, anyhow::Error>(CFRetained::<CVMetalTexture>::from_raw(
                    ptr::NonNull::new(texture).context("CoreVideo returned a null texture")?,
                ))
            };
            let y_texture = plane_texture(
                0,
                if bgra {
                    objc2_metal::MTLPixelFormat::BGRA8Unorm
                } else {
                    objc2_metal::MTLPixelFormat::R8Unorm
                },
            )?;
            let cb_cr_texture = if bgra {
                None
            } else {
                Some(plane_texture(1, objc2_metal::MTLPixelFormat::RG8Unorm)?)
            };

            command_encoder.set_vertex_bytes(
                SurfaceInputIndex::TextureSize as u64,
                mem::size_of_val(&texture_size) as u64,
                &texture_size as *const Size<DevicePixels> as *const _,
            );
            let y_metal_texture = CVMetalTextureGetTexture(&y_texture)
                .context("CoreVideo returned no Metal texture")?;
            let cb_cr_metal_texture =
                CVMetalTextureGetTexture(cb_cr_texture.as_ref().unwrap_or(&y_texture))
                    .context("CoreVideo returned no Metal texture")?;
            command_encoder.set_fragment_texture(
                SurfaceInputIndex::YTexture as u64,
                Some(crate::metal_compat::legacy_texture(&y_metal_texture)),
            );
            command_encoder.set_fragment_texture(
                SurfaceInputIndex::CbCrTexture as u64,
                Some(crate::metal_compat::legacy_texture(&cb_cr_metal_texture)),
            );

            views.push(y_texture);
            views.extend(cb_cr_texture);
            command_encoder.draw_primitives_instanced_base_instance(
                metal::MTLPrimitiveType::Triangle,
                0,
                6,
                1,
                (first_surface + index) as u64,
            );
        }
        // A CVMetalTexture's final release returns its backing to its cache.
        // Pending commands can outlive this renderer, so retain the cache too.
        // Tuple elements drop in order: views must be released before the cache,
        // including when an unsubmitted command drops its completion block.
        let resources = Cell::new(Some((views, self.core_video_texture_cache.clone())));
        let keep_alive = RcBlock::new(move |_: ptr::NonNull<AnyObject>| {
            drop(resources.take());
        });
        unsafe {
            command_buffer.add_completed_handler(&*RcBlock::as_ptr(&keep_alive).cast());
        }
        Ok(())
    }
}

fn new_command_encoder_for_texture<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    texture: &'a metal::TextureRef,
    viewport_size: Size<DevicePixels>,
    clear_color: Option<metal::MTLClearColor>,
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(texture));
    color_attachment.set_store_action(metal::MTLStoreAction::Store);
    if let Some(clear_color) = clear_color {
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(clear_color);
    } else {
        color_attachment.set_load_action(metal::MTLLoadAction::Load);
    }

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
fn read_texture_to_image(texture: &metal::TextureRef) -> Result<RgbaImage> {
    let width = texture.width() as u32;
    let height = texture.height() as u32;
    let bytes_per_row = width as usize * 4;
    let mut pixels = vec![0u8; height as usize * bytes_per_row];

    let region = metal::MTLRegion {
        origin: metal::MTLOrigin { x: 0, y: 0, z: 0 },
        size: metal::MTLSize {
            width: width as u64,
            height: height as u64,
            depth: 1,
        },
    };
    texture.get_bytes(
        pixels.as_mut_ptr() as *mut std::ffi::c_void,
        bytes_per_row as u64,
        region,
        0,
    );

    // Convert BGRA to RGBA (swap B and R channels)
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    RgbaImage::from_raw(width, height, pixels).context("failed to create RgbaImage from pixel data")
}

fn build_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_sprite_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

#[derive(Clone)]
struct InstanceBinding {
    buffer: metal::Buffer,
    offset: usize,
}

struct InstanceBindings {
    shaders: InstanceBinding,
    quads: InstanceBinding,
    shadows: InstanceBinding,
    underlines: InstanceBinding,
    monochrome_sprites: InstanceBinding,
    polychrome_sprites: InstanceBinding,
    surfaces: InstanceBinding,
}

fn write_instances(scene: &Scene, writer: &mut InstanceBufferWriter) -> Result<InstanceBindings> {
    Ok(InstanceBindings {
        shaders: writer.write_iter(scene.shader_draws.iter().map(|d| d.instance_data()))?,
        quads: writer.write(&scene.quads)?,
        shadows: writer.write(&scene.shadows)?,
        underlines: writer.write(&scene.underlines)?,
        monochrome_sprites: writer.write(&scene.monochrome_sprites)?,
        polychrome_sprites: writer.write(&scene.polychrome_sprites)?,
        surfaces: writer.write_iter(scene.surfaces.iter().map(|surface| SurfaceBounds {
            bounds: surface.bounds,
            content_mask: surface.content_mask,
        }))?,
    })
}

struct InstanceBufferWriter {
    device: metal::Device,
    pool: Arc<Mutex<InstanceBufferPool>>,
    unified_memory: bool,
    filled: Vec<(InstanceBuffer, usize)>,
    current: InstanceBuffer,
    offset: usize,
}

impl InstanceBufferWriter {
    fn new(
        device: &metal::Device,
        pool: &Arc<Mutex<InstanceBufferPool>>,
        unified_memory: bool,
    ) -> Self {
        let current = pool.lock().acquire(device, unified_memory);
        Self {
            device: device.clone(),
            pool: pool.clone(),
            unified_memory,
            filled: Vec::new(),
            current,
            offset: 0,
        }
    }

    fn allocate<T>(&mut self, count: usize) -> Result<(InstanceBinding, &mut [MaybeUninit<T>])> {
        let size = mem::size_of::<T>() * count;
        let mut offset = self.offset.next_multiple_of(INSTANCE_BUFFER_ALIGNMENT);
        if offset + size > self.current.size {
            self.grow(size)?;
            offset = 0;
        }
        self.offset = offset + size;

        let binding = InstanceBinding {
            buffer: self.current.metal_buffer.clone(),
            offset,
        };
        // Safety: the reservation lies within a buffer this frame owns
        // exclusively, and never overlaps one handed out earlier.
        let values = unsafe {
            let start = (self.current.metal_buffer.contents() as *mut u8).add(offset);
            slice::from_raw_parts_mut(start.cast::<MaybeUninit<T>>(), count)
        };
        Ok((binding, values))
    }

    fn write<T>(&mut self, values: &[T]) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        unsafe {
            ptr::copy_nonoverlapping(
                values.as_ptr(),
                destination.as_mut_ptr().cast::<T>(),
                values.len(),
            );
        }
        Ok(binding)
    }

    fn write_iter<T>(
        &mut self,
        values: impl ExactSizeIterator<Item = T>,
    ) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        for (slot, value) in destination.iter_mut().zip(values) {
            slot.write(value);
        }
        Ok(binding)
    }

    fn grow(&mut self, required: usize) -> Result<()> {
        let mut pool = self.pool.lock();
        let buffer_size = (pool.buffer_size * 2)
            .max(required.next_power_of_two())
            .min(MAX_INSTANCE_BUFFER_SIZE);
        anyhow::ensure!(
            buffer_size >= required,
            "instance buffer needs {required} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE}"
        );
        anyhow::ensure!(
            buffer_size > self.current.size,
            "frame instance data exceeds the {MAX_INSTANCE_BUFFER_SIZE}-byte maximum"
        );
        if buffer_size != pool.buffer_size {
            log::info!("increased instance buffer size to {buffer_size}");
            pool.reset(buffer_size);
        }
        let buffer = pool.acquire(&self.device, self.unified_memory);
        drop(pool);

        let filled = mem::replace(&mut self.current, buffer);
        self.filled.push((filled, self.offset));
        self.offset = 0;
        Ok(())
    }

    fn finish(self) -> InstanceBuffer {
        let Self {
            unified_memory,
            filled,
            current,
            offset,
            ..
        } = self;

        if !unified_memory {
            for (buffer, written) in &filled {
                if *written == 0 {
                    continue;
                }
                buffer.metal_buffer.did_modify_range(NSRange {
                    location: 0,
                    length: *written as NSUInteger,
                });
            }
            if offset > 0 {
                current.metal_buffer.did_modify_range(NSRange {
                    location: 0,
                    length: offset as NSUInteger,
                });
            }
        }

        // Metal retains encoded resources until the command buffer completes.
        // Only the final, largest buffer is worth keeping in the pool.
        drop(filled);
        current
    }
}

#[repr(C)]
enum ShadowInputIndex {
    Vertices = 0,
    Shadows = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum QuadInputIndex {
    Vertices = 0,
    Quads = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum UnderlineInputIndex {
    Vertices = 0,
    Underlines = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum SpriteInputIndex {
    Vertices = 0,
    Sprites = 1,
    ViewportSize = 2,
    AtlasTextureSize = 3,
    AtlasTexture = 4,
}

#[repr(C)]
enum SurfaceInputIndex {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
}

#[repr(C)]
enum PathRasterizationInputIndex {
    Vertices = 0,
    ViewportSize = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PathSprite {
    pub bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SurfaceBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
pub struct MetalHeadlessRenderer {
    renderer: MetalRenderer,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl MetalHeadlessRenderer {
    /// Bytes in retained subtree textures, excluding driver/in-flight references.
    pub fn layer_texture_bytes(&self) -> u64 {
        self.renderer
            .backdrops
            .as_ref()
            .map(|effects| effects.groups.texture_bytes())
            .unwrap_or(0)
    }

    /// Number of retained effect textures; excludes driver and in-flight references.
    pub fn effect_texture_count(&self) -> usize {
        self.renderer
            .backdrops
            .as_ref()
            .map(|effects| effects.texture_count())
            .unwrap_or(0)
    }

    /// Cumulative backdrop filter cache statistics.
    pub fn backdrop_cache_stats(&self) -> gpui::BackdropCacheStats {
        self.renderer
            .backdrops
            .as_ref()
            .map(|e| e.cache.stats)
            .unwrap_or_default()
    }

    /// Cumulative captured-subtree cache statistics.
    pub fn layer_cache_stats(&self) -> gpui::LayerCacheStats {
        self.renderer
            .backdrops
            .as_ref()
            .map(|e| e.groups.stats)
            .unwrap_or_default()
    }

    /// Configure native wide-SDR composition for offscreen validation.
    #[cfg(target_os = "macos")]
    pub fn set_color_mode(
        &mut self,
        mode: gpui::WindowColorMode,
    ) -> Result<gpui::WindowColorStatus> {
        self.renderer.set_color_mode(mode)
    }
    pub fn new() -> Self {
        let instance_buffer_pool = Arc::new(Mutex::new(InstanceBufferPool::default()));
        let renderer = MetalRenderer::new_headless(instance_buffer_pool);
        Self { renderer }
    }
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl gpui::PlatformHeadlessRenderer for MetalHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.renderer.render_scene_to_image(scene, size)
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.renderer.render_scene(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
        self.renderer.sprite_atlas().clone()
    }
}

#[cfg(test)]
mod submission_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_foundation::{CFDictionary, CFType};
    use objc2_core_video::{
        CVPixelBufferCreate, CVPixelBufferGetBaseAddressOfPlane,
        CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVPixelBufferIOSurfacePropertiesKey,
    };

    #[test]
    fn render_core_video_surface() {
        let attributes = CFDictionary::from_slices(
            &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
            &[&*CFDictionary::<CFType, CFType>::empty()],
        );
        let mut image_buffer = ptr::null_mut();
        assert_eq!(
            unsafe {
                CVPixelBufferCreate(
                    None,
                    16,
                    16,
                    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                    Some(attributes.as_opaque()),
                    ptr::NonNull::from(&mut image_buffer),
                )
            },
            kCVReturnSuccess
        );
        let image_buffer =
            unsafe { CFRetained::from_raw(ptr::NonNull::new(image_buffer).unwrap()) };
        unsafe {
            assert_eq!(
                CVPixelBufferLockBaseAddress(&image_buffer, CVPixelBufferLockFlags::empty()),
                kCVReturnSuccess
            );
            for plane in 0..2 {
                let bytes = CVPixelBufferGetBaseAddressOfPlane(&image_buffer, plane).cast::<u8>();
                let length = CVPixelBufferGetBytesPerRowOfPlane(&image_buffer, plane)
                    * CVPixelBufferGetHeightOfPlane(&image_buffer, plane);
                slice::from_raw_parts_mut(bytes, length).fill(128);
            }
            assert_eq!(
                CVPixelBufferUnlockBaseAddress(&image_buffer, CVPixelBufferLockFlags::empty()),
                kCVReturnSuccess
            );
        }

        let bounds = Bounds {
            origin: Point::default(),
            size: size(ScaledPixels(16.0), ScaledPixels(16.0)),
        };
        let mut scene = Scene::default();
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            image_buffer: image_buffer.clone(),
        });
        drop(image_buffer);
        scene.finish();

        let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        for image::Rgba([red, green, blue, alpha]) in image.pixels() {
            assert!(red.abs_diff(128) <= 2);
            assert!(green.abs_diff(128) <= 2);
            assert!(blue.abs_diff(128) <= 2);
            assert_eq!(*alpha, 255);
        }
    }
    #[test]
    fn render_bgra_core_video_surface() {
        let frame = crate::custom_draw::core_video::test_frame(32, 18, 0).unwrap();
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(32.), ScaledPixels(18.)),
        );
        let mut scene = Scene::default();
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            image_buffer: frame.buffer_for_test(),
        });
        drop(frame);
        scene.finish();
        let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(32), DevicePixels(18)))
            .unwrap();
        let p = image.get_pixel(3, 2).0;
        assert!(
            p[0].abs_diff(224) <= 2 && p[1].abs_diff(64) <= 2 && p[2].abs_diff(32) <= 2,
            "BGRA order: {p:?}"
        );
    }

    #[test]
    fn unsupported_core_video_surface_returns_error() {
        let mut buffer = ptr::null_mut();
        let status = unsafe {
            CVPixelBufferCreate(
                None,
                16,
                16,
                objc2_core_video::kCVPixelFormatType_32ARGB,
                None,
                ptr::NonNull::from(&mut buffer),
            )
        };
        assert_eq!(status, kCVReturnSuccess);
        let buffer = unsafe { CFRetained::from_raw(ptr::NonNull::new(buffer).unwrap()) };
        assert!(
            unsafe { crate::custom_draw::core_video::CoreVideoFrame::from_bgra(buffer.clone()) }
                .is_err()
        );
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(16.), ScaledPixels(16.)),
        );
        let mut scene = Scene::default();
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            image_buffer: buffer,
        });
        scene.finish();
        let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
        let error = renderer
            .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported CoreVideo surface format"),
            "{error}"
        );
    }

    #[test]
    fn surface_sampling_retains_cache_after_renderer_destruction() {
        for nv12 in [false, true] {
            for _ in 0..16 {
                objc2::rc::autoreleasepool(|_| {
                    use crate::custom_draw::core_video::{Nv12Matrix, test_frame, test_nv12_frame};
                    let frame = if nv12 {
                        test_nv12_frame(32, 18, 0, Nv12Matrix::Bt601, false).unwrap()
                    } else {
                        test_frame(32, 18, 0).unwrap()
                    };
                    let bounds = Bounds::new(
                        point(ScaledPixels(0.), ScaledPixels(0.)),
                        size(ScaledPixels(32.), ScaledPixels(18.)),
                    );
                    let mut scene = Scene::default();
                    scene.insert_primitive(PaintSurface {
                        order: 0,
                        bounds,
                        content_mask: ContentMask { bounds },
                        image_buffer: frame.buffer_for_test(),
                    });
                    scene.finish();
                    drop(frame);
                    let mut renderer =
                        MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
                    let cache = renderer.core_video_texture_cache.clone();
                    let extent = size(DevicePixels(32), DevicePixels(18));
                    let target = crate::custom_draw::texture(&renderer.device, extent).unwrap();
                    let command = renderer.render_frame(&scene, &target, extent).unwrap();
                    drop(scene);
                    drop(renderer);
                    // This observer is one owner. Pending sampling must retain its own cache,
                    // not rely on the renderer or on CVMetalTexture implicitly retaining it.
                    assert!(
                        cache.retain_count() >= 2,
                        "pending Surface sampling lost its CoreVideo cache owner"
                    );
                    command.commit();
                    command.wait_until_completed();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                    while cache.retain_count() != 1 && std::time::Instant::now() < deadline {
                        std::thread::yield_now();
                    }
                    assert_eq!(
                        cache.retain_count(),
                        1,
                        "completion must release its cache owner even while command remains alive"
                    );
                    let image = read_texture_to_image(&target).unwrap();
                    let pixel = image.get_pixel(3, 2);
                    assert!(
                        pixel[0].abs_diff(224) <= 3
                            && pixel[1].abs_diff(64) <= 3
                            && pixel[2].abs_diff(32) <= 3,
                        "Surface survived renderer teardown: {pixel:?}"
                    );
                });
            }
        }
    }

    #[test]
    fn core_video_snapshot_lives_until_sampling_completion() {
        for variant in 0..5 {
            objc2::rc::autoreleasepool(|_| {
                use crate::custom_draw::core_video::{Nv12Matrix, test_frame, test_nv12_frame};
                let frame = Arc::new(if variant == 0 {
                    test_frame(32, 18, 0).unwrap()
                } else {
                    test_nv12_frame(
                        32,
                        18,
                        0,
                        if variant <= 2 {
                            Nv12Matrix::Bt601
                        } else {
                            Nv12Matrix::Bt709
                        },
                        variant % 2 == 0,
                    )
                    .unwrap()
                });
                let weak = Arc::downgrade(&frame);
                let bounds = Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(0.)),
                    size(ScaledPixels(32.), ScaledPixels(18.)),
                );
                let mut scene = Scene::default();
                scene.insert_primitive(gpui::CustomDraw {
                    order: Default::default(),
                    bounds,
                    content_mask: ContentMask { bounds },
                    corner_radii: Default::default(),
                    opacity: 1.,
                    content: frame.clone(),
                    coverage: None,
                });
                scene.finish();
                let mut renderer =
                    MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
                let extent = size(DevicePixels(32), DevicePixels(18));
                let target = crate::custom_draw::texture(&renderer.device, extent).unwrap();
                let command = renderer.render_frame(&scene, &target, extent).unwrap();
                drop(scene);
                drop(frame);
                assert!(
                    weak.upgrade().is_some(),
                    "unsubmitted sampling command owns its snapshot"
                );
                command.commit();
                command.wait_until_completed();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                while weak.upgrade().is_some() && std::time::Instant::now() < deadline {
                    std::thread::yield_now();
                }
                assert!(
                    weak.upgrade().is_none(),
                    "completion releases producer even while command object remains alive"
                );
            });
        }
    }
}

fn create_shader_pipeline(
    device: &metal::DeviceRef,
    program: &gpui::SceneShaderProgram,
) -> Result<metal::RenderPipelineState> {
    use metal::*;
    let library = device
        .new_library_with_source(&program.metal().0, &CompileOptions::new())
        .map_err(anyhow::Error::msg)?;
    let descriptor = RenderPipelineDescriptor::new();
    let vertex = library
        .get_function(&program.metal().1, None)
        .map_err(anyhow::Error::msg)?;
    let fragment = library
        .get_function(&program.metal().2, None)
        .map_err(anyhow::Error::msg)?;
    descriptor.set_vertex_function(Some(&vertex));
    descriptor.set_fragment_function(Some(&fragment));
    let layout = VertexDescriptor::new();
    for index in 0..11 {
        let attribute = layout.attributes().object_at(index).unwrap();
        attribute.set_format(MTLVertexFormat::Float4);
        attribute.set_offset(index * 16);
        attribute.set_buffer_index(0);
    }
    let buffer = layout.layouts().object_at(0).unwrap();
    buffer.set_stride(176);
    buffer.set_step_function(MTLVertexStepFunction::PerInstance);
    buffer.set_step_rate(1);
    descriptor.set_vertex_descriptor(Some(&layout));
    let color = descriptor.color_attachments().object_at(0).unwrap();
    let wide = program.output_encoding() == gpui::ShaderOutputEncoding::LinearDisplayP3;
    color.set_pixel_format(if wide {
        MTLPixelFormat::RGBA16Float
    } else {
        MTLPixelFormat::BGRA8Unorm
    });
    color.set_blending_enabled(true);
    color.set_source_rgb_blend_factor(if wide {
        MTLBlendFactor::One
    } else {
        MTLBlendFactor::SourceAlpha
    });
    color.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
    color.set_source_alpha_blend_factor(MTLBlendFactor::One);
    color.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
    device
        .new_render_pipeline_state(&descriptor)
        .map_err(anyhow::Error::msg)
}
