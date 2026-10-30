//! Metal integration for retained, independently rendered GPU content.
use crate::metal;
use crate::metal::*;
use objc2::{rc::Retained, runtime::ProtocolObject};
pub mod core_video;
mod coverage_cache;
use anyhow::{Result, ensure};
pub(crate) use coverage_cache::{CoverageCache, CoverageKey};
use gpui::{CustomDraw, DevicePixels, Size, size};
use objc2_metal::{MTLCommandBuffer, MTLCommandQueue, MTLDevice, MTLTexture};

/// Borrowed by producers during preparation. Handles belong to the host device.
#[derive(Clone)]
pub struct MetalCustomContext {
    /// Active host device; create all producer resources on this device.
    pub device: Device,
    /// Host queue for recording commands. Only the host commits them.
    pub queue: CommandQueue,
}

impl MetalCustomContext {
    /// Typed objc2 view of the host device, borrowed for this context's lifetime.
    /// Producers may retain it when keeping resources beyond preparation.
    pub fn objc2_device(&self) -> &ProtocolObject<dyn MTLDevice> {
        &self.device
    }

    /// Typed objc2 view of the host queue. Producers record commands on this
    /// queue; only the host may enqueue or commit them.
    pub fn objc2_queue(&self) -> &ProtocolObject<dyn MTLCommandQueue> {
        &self.queue
    }

    /// Queue-scoped cache identity. Cached command buffers retain their queue.
    pub fn cache_key(&self) -> u64 {
        self.queue.as_ptr() as usize as u64
    }
}

/// Producer output: straight-alpha, encoded sRGB-channel BGRA8Unorm texture.
/// Commands must be uncommitted and allocated from the supplied host queue.
/// The host submits them before sampling the texture. Return no commands when
/// reusing immutable cached output. Resources referenced by commands are retained
/// by Metal's normal command-buffer resource retention.
/// Cached output with no commands must already be ready or have been submitted
/// earlier on the host queue. The host does not wait for CPU-observed completion.
pub struct MetalCustomOutput {
    /// Immutable sampleable output matching the requested physical extent.
    pub texture: Texture,
    /// Pending preparation commands, or empty for already-submitted cached output.
    pub commands: Vec<CommandBuffer>,
}

/// Source-sized RGBA8 image. The compositor samples its complete mip chain
/// directly; no display-sized intermediate texture is allocated.
pub struct MetalImageOutput {
    output: MetalCustomOutput,
    queue: CommandQueue,
}
// SAFETY: the image texture is immutable after its recorded upload. Moving
// this owner between the staging worker and host does not encode concurrently;
// the host alone submits commands and cache access is synchronized by callers.
unsafe impl Send for MetalImageOutput {}
impl MetalImageOutput {
    /// Host submission has accepted all upload commands; no completion wait.
    pub fn is_submitted(&self) -> bool {
        self.output.commands.iter().all(|command| {
            matches!(
                command.status(),
                MTLCommandBufferStatus::Committed
                    | MTLCommandBufferStatus::Scheduled
                    | MTLCommandBufferStatus::Completed
            )
        })
    }

    /// Reuse only on the original host queue. Pending recordings are resubmitted
    /// by the host; submitted work remains ordered without a CPU fence wait.
    pub fn reusable_for(&self, context: &MetalCustomContext) -> Result<Self> {
        ensure!(
            self.queue.as_ptr() == context.queue.as_ptr(),
            "image output belongs to another queue"
        );
        ensure!(
            self.output.texture.device().registryID() == context.device.registryID(),
            "image output belongs to another device"
        );
        let mut commands = Vec::new();
        for command in &self.output.commands {
            if wide_command_pending(command.status())? {
                commands.push(command.clone());
            }
        }
        Ok(Self {
            output: MetalCustomOutput {
                texture: self.output.texture.clone(),
                commands,
            },
            queue: self.queue.clone(),
        })
    }
}

/// Queue-bound cache of host-shared upload buffers. Borrowed buffers return
/// only after the upload completion callback releases its exclusive lease.
#[derive(Clone)]
pub struct MetalImageStagingPool {
    queue: CommandQueue,
    buffers: std::sync::Arc<gpui::ImageStagingPool<Buffer>>,
}
impl MetalImageStagingPool {
    pub fn stats(&self) -> gpui::ImageStagingPoolStats {
        self.buffers.stats()
    }
}

impl MetalCustomContext {
    pub fn image_staging_pool(&self, capacity: usize) -> MetalImageStagingPool {
        MetalImageStagingPool {
            queue: self.queue.clone(),
            buffers: gpui::ImageStagingPool::new(capacity),
        }
    }
    /// Worker convenience route with an autorelease pool and no retained cache.
    pub fn stage_image_upload(&self, data: &gpui::ImageTextureData) -> Result<MetalImageOutput> {
        objc2::rc::autoreleasepool(|_| self.create_image_output(data))
    }
    /// Worker-side allocation/refill with completion-safe pool reuse. An optional
    /// admission guard remains alive through GPU completion, not just submission.
    pub fn stage_image_upload_with_pool(
        &self,
        data: &gpui::ImageTextureData,
        pool: &MetalImageStagingPool,
        guard: Option<std::sync::Arc<dyn Send + Sync>>,
    ) -> Result<MetalImageOutput> {
        objc2::rc::autoreleasepool(|_| self.create_pooled_image(data, pool, guard))
    }
    /// Synchronous convenience route; full media sources use the worker API.
    pub fn create_image_output(&self, data: &gpui::ImageTextureData) -> Result<MetalImageOutput> {
        self.create_pooled_image(
            data,
            &self.image_staging_pool(data.staging_byte_len()),
            None,
        )
    }
    fn create_pooled_image(
        &self,
        data: &gpui::ImageTextureData,
        pool: &MetalImageStagingPool,
        guard: Option<std::sync::Arc<dyn Send + Sync>>,
    ) -> Result<MetalImageOutput> {
        ensure!(
            pool.queue.as_ptr() == self.queue.as_ptr(),
            "foreign image staging pool"
        );
        ensure!(
            self.queue.device().registryID() == self.device.registryID(),
            "host queue belongs to another device"
        );
        let bytes = data.staging_byte_len();
        let allocation = pool.buffers.checkout(bytes as u64, bytes, || {
            Ok(metal::Buffer(
                self.device
                    .newBufferWithLength_options(
                        bytes as usize,
                        MTLResourceOptions::StorageModeShared,
                    )
                    .expect("Metal buffer allocation failed"),
            ))
        })?;
        let desc = TextureDescriptor::new();
        unsafe { desc.setWidth(data.width() as usize) };
        unsafe { desc.setHeight(data.height() as usize) };
        unsafe { desc.setMipmapLevelCount(data.levels().len() as usize) };
        desc.setPixelFormat(match data.format() {
            gpui::ImageTextureFormat::Rgba8Srgb => MTLPixelFormat::RGBA8Unorm,
            gpui::ImageTextureFormat::LinearP3Rgba16Float => MTLPixelFormat::RGBA16Float,
        });
        desc.setStorageMode(MTLStorageMode::Private);
        desc.setUsage(MTLTextureUsage::ShaderRead);
        let texture = self
            .device
            .newTextureWithDescriptor(&desc)
            .expect("Metal texture allocation failed");
        let command = self
            .queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        let encoder = command
            .blitCommandEncoder()
            .expect("Metal blit encoder allocation failed");
        let buffer = allocation.resource();
        // The exclusive lease is never in the cache while a worker or a pending
        // GPU command owns it. Completion, not queue submission, permits refill.
        let mapped = unsafe {
            std::slice::from_raw_parts_mut(buffer.contents().cast::<u8>().as_ptr(), bytes)
        };
        let mut offset = 0usize;
        for (level, pixels) in data.levels().iter().enumerate() {
            let width = (data.width() >> level).max(1) as usize;
            let height = (data.height() >> level).max(1) as usize;
            let bpp = data.bytes_per_pixel();
            let row = (width * bpp as usize).div_ceil(256) * 256;
            for y in 0..height as usize {
                mapped[offset + y * row as usize..offset + y * row as usize + width as usize * bpp]
                    .copy_from_slice(
                        &pixels[y * width as usize * bpp..(y + 1) * width as usize * bpp],
                    );
            }
            unsafe {
                encoder.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin_options(buffer, offset as usize, row, row * height, MTLSize { width: width, height: height, depth: 1 }, &texture, 0, level as usize, MTLOrigin { x: 0, y: 0, z: 0 }, MTLBlitOption::empty())
            };
            offset += row as usize * height as usize;
        }
        encoder.endEncoding();
        let completion = std::sync::Mutex::new(Some((allocation, guard)));
        let block = block2::RcBlock::new(
            move |_: std::ptr::NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                completion.lock().unwrap().take();
            },
        );
        // SAFETY: Both views use the Objective-C block pointer ABI. Metal calls
        // the completion block only after this upload can no longer use the buffer.
        unsafe {
            command.addCompletedHandler(block2::RcBlock::as_ptr(&block));
        }
        Ok(MetalImageOutput {
            output: MetalCustomOutput {
                texture,
                commands: vec![command],
            },
            queue: self.queue.clone(),
        })
    }
}

/// Immutable wide-SDR output: RGBA16Float, linear Display P3/D65, premultiplied
/// alpha. Straight RGB and alpha must be finite in 0..=1; associated RGB is in
/// 0..=alpha and zero at zero alpha. The host applies clip/opacity once.
///
/// Requires an accepted `WindowColorMode::WideGamutSdr` request. Producers record
/// using the host queue and never submit or present. Pixel values are a producer
/// contract; structural validation cannot prove their color interpretation.
pub struct MetalWideSdrOutput {
    output: MetalCustomOutput,
    queue: CommandQueue,
}
impl MetalWideSdrOutput {
    /// Accept native objc2-owned resources from a producer.
    /// The texture and uncommitted commands retain their identities and transfer
    /// ownership to the host without a GPU copy, enqueue, commit or CPU wait.
    /// The same device, queue, pixel-format and usage checks as
    /// `from_linear_p3_premultiplied` apply.
    pub fn from_objc2_linear_p3_premultiplied(
        context: &MetalCustomContext,
        texture: Retained<ProtocolObject<dyn MTLTexture>>,
        commands: Vec<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    ) -> Result<Self> {
        Self::from_linear_p3_premultiplied(context, texture, commands)
    }

    /// Take ownership of an explicitly described wide texture and pending work.
    /// Empty commands require content already ready/submitted on the host queue.
    pub fn from_linear_p3_premultiplied(
        context: &MetalCustomContext,
        texture: Texture,
        commands: Vec<CommandBuffer>,
    ) -> Result<Self> {
        let result = Self {
            output: MetalCustomOutput { texture, commands },
            queue: context.queue.clone(),
        };
        result.validate_context(context)?;
        Ok(result)
    }

    /// Reuse immutable cached output. Pending commands retain identity; commands
    /// submitted by the host are omitted. External enqueue/error or queue changes
    /// are errors. No CPU wait is introduced. Under the producer protocol, host-only
    /// submission is cooperative; command status cannot identify who committed.
    pub fn reusable_for(&self, context: &MetalCustomContext) -> Result<Self> {
        self.validate_resources(context)?;
        let mut commands = Vec::new();
        for command in &self.output.commands {
            if wide_command_pending(command.status())? {
                commands.push(command.clone());
            }
        }
        Ok(Self {
            output: MetalCustomOutput {
                texture: self.output.texture.clone(),
                commands,
            },
            queue: self.queue.clone(),
        })
    }
    fn validate_resources(&self, context: &MetalCustomContext) -> Result<()> {
        ensure!(
            self.queue.as_ptr() == context.queue.as_ptr(),
            "wide output belongs to another queue"
        );
        ensure!(
            context.queue.device().registryID() == context.device.registryID(),
            "host queue belongs to another device"
        );
        let t = &self.output.texture;
        ensure!(
            t.device().registryID() == context.device.registryID(),
            "wide texture belongs to another device"
        );
        ensure!(
            t.pixelFormat() == MTLPixelFormat::RGBA16Float
                && t.textureType() == MTLTextureType::Type2D
                && t.sampleCount() == 1
                && t.usage().contains(MTLTextureUsage::ShaderRead)
                && t.width() > 0
                && t.height() > 0
                && t.width() <= 16384
                && t.height() <= 16384,
            "invalid wide texture format, usage, or extent"
        );
        Ok(())
    }
    pub(crate) fn validate_context(&self, context: &MetalCustomContext) -> Result<()> {
        self.validate_resources(context)?;
        validate_commands(context, &self.output.commands)
    }
    pub(crate) fn into_output(self) -> MetalCustomOutput {
        self.output
    }
}

fn wide_command_pending(status: MTLCommandBufferStatus) -> Result<bool> {
    match status {
        MTLCommandBufferStatus::NotEnqueued => Ok(true),
        MTLCommandBufferStatus::Committed
        | MTLCommandBufferStatus::Scheduled
        | MTLCommandBufferStatus::Completed => Ok(false),
        _ => anyhow::bail!("wide output command was externally enqueued or failed"),
    }
}

// Internal presentation wrapper preserves the public MetalCustomOutput shape.
pub(crate) struct PreparedMetalOutput {
    pub output: MetalCustomOutput,
    nv12: Option<MetalNv12Output>,
}
impl From<MetalCustomOutput> for PreparedMetalOutput {
    fn from(output: MetalCustomOutput) -> Self {
        Self { output, nv12: None }
    }
}
impl std::ops::Deref for PreparedMetalOutput {
    type Target = MetalCustomOutput;
    fn deref(&self) -> &Self::Target {
        &self.output
    }
}
#[derive(Clone)]
pub(crate) struct MetalNv12Output {
    y: Texture,
    uv: Texture,
    rows: [[f32; 4]; 3],
    queue: CommandQueue,
}
impl MetalNv12Output {
    fn reusable_for(&self, context: &MetalCustomContext) -> Result<Self> {
        ensure!(
            self.queue.as_ptr() == context.queue.as_ptr(),
            "NV12 output belongs to another queue"
        );
        ensure!(
            self.y.device().registryID() == context.device.registryID()
                && self.uv.device().registryID() == context.device.registryID(),
            "NV12 planes belong to another device"
        );
        Ok(self.clone())
    }
}
pub(crate) trait MetalSampleSource {
    fn color_texture(&self) -> &TextureRef;
    fn nv12_planes(&self) -> Option<&MetalNv12Output> {
        None
    }
}
impl MetalSampleSource for MetalCustomOutput {
    fn color_texture(&self) -> &TextureRef {
        &self.texture
    }
}
impl MetalSampleSource for PreparedMetalOutput {
    fn color_texture(&self) -> &TextureRef {
        &self.output.texture
    }
    fn nv12_planes(&self) -> Option<&MetalNv12Output> {
        self.nv12.as_ref()
    }
}

pub(crate) struct CustomCompositor {
    pipeline: RenderPipelineState,
    pub(crate) masks: CoverageCache<Texture>,
    pub(crate) mask_bytes: usize,
}

impl CustomCompositor {
    pub fn new(device: &DeviceRef) -> Result<Self> {
        let library = device
            .newLibraryWithSource_options_error(
                &objc2_foundation::NSString::from_str(include_str!("custom_draw.metal")),
                Some(&CompileOptions::new()),
            )
            .map_err(anyhow::Error::msg)?;
        let descriptor = RenderPipelineDescriptor::new();
        let vertex = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str("custom_vertex"))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str("custom_fragment"))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        descriptor.setVertexFunction(Some(&vertex));
        descriptor.setFragmentFunction(Some(&fragment));
        let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
        color.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        color.setBlendingEnabled(true);
        color.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
        color.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        color.setSourceAlphaBlendFactor(MTLBlendFactor::One);
        color.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        let pipeline = device
            .newRenderPipelineStateWithDescriptor_error(&descriptor)
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            pipeline,
            masks: Default::default(),
            mask_bytes: 0,
        })
    }

    pub fn extent(draw: &CustomDraw) -> Result<Size<DevicePixels>> {
        ensure!(
            draw.opacity.is_finite() && (0.0..=1.0).contains(&draw.opacity),
            "invalid custom opacity"
        );
        ensure!(
            [
                draw.bounds.origin.x.0,
                draw.bounds.origin.y.0,
                draw.content_mask.bounds.origin.x.0,
                draw.content_mask.bounds.origin.y.0,
                draw.content_mask.bounds.size.width.0,
                draw.content_mask.bounds.size.height.0,
                draw.corner_radii.top_left.0,
                draw.corner_radii.top_right.0,
                draw.corner_radii.bottom_left.0,
                draw.corner_radii.bottom_right.0
            ]
            .iter()
            .all(|v| v.is_finite()),
            "non-finite custom geometry"
        );
        if let Some(source) = draw.content.image_source_size() {
            ensure!(
                draw.coverage.is_none()
                    && draw.bounds.size.width.0.is_finite()
                    && draw.bounds.size.height.0.is_finite()
                    && draw.bounds.size.width.0 > 0.
                    && draw.bounds.size.height.0 > 0.
                    && source.width.0 > 0
                    && source.height.0 > 0
                    && source.width.0 <= 16384
                    && source.height.0 <= 16384,
                "invalid source-sized image preparation"
            );
            return Ok(source);
        }
        let width = draw.bounds.size.width.0.ceil();
        let height = draw.bounds.size.height.0.ceil();
        ensure!(
            width.is_finite()
                && height.is_finite()
                && width > 0.
                && height > 0.
                && width <= 16384.
                && height <= 16384.,
            "invalid custom texture extent"
        );
        Ok(size(
            DevicePixels(width as i32),
            DevicePixels(height as i32),
        ))
    }

    pub fn validate(
        context: &MetalCustomContext,
        draw: &CustomDraw,
        output: Box<dyn std::any::Any>,
    ) -> Result<MetalCustomOutput> {
        Self::validate_format(context, draw, output, MTLPixelFormat::BGRA8Unorm)
    }

    // Private prototype can validate an explicitly tagged wide output without
    // relaxing the existing public producer contract.
    pub(crate) fn validate_format(
        context: &MetalCustomContext,
        draw: &CustomDraw,
        output: Box<dyn std::any::Any>,
        format: MTLPixelFormat,
    ) -> Result<MetalCustomOutput> {
        let extent = Self::extent(draw)?;
        let source_sized = output.is::<MetalImageOutput>();
        let output = if source_sized {
            let image = *output.downcast::<MetalImageOutput>().unwrap();
            ensure!(
                image.queue.as_ptr() == context.queue.as_ptr(),
                "image output belongs to another queue"
            );
            Box::new(image.output) as Box<dyn std::any::Any>
        } else {
            output
        };
        let output = *output
            .downcast::<MetalCustomOutput>()
            .map_err(|_| anyhow::anyhow!("producer returned a non-Metal output"))?;
        ensure!(
            output.texture.device().registryID() == context.device.registryID(),
            "custom texture belongs to another device"
        );
        if source_sized {
            ensure!(
                (output.texture.pixelFormat() == MTLPixelFormat::RGBA16Float)
                    == (draw.content.image_color_mode() != gpui::ImageColorMode::EncodedSrgb),
                "image texture format does not match source color metadata"
            );
        }
        ensure!(
            (if source_sized {
                matches!(
                    output.texture.pixelFormat(),
                    MTLPixelFormat::RGBA8Unorm
                        | MTLPixelFormat::BGRA8Unorm
                        | MTLPixelFormat::RGBA16Float
                )
            } else {
                output.texture.pixelFormat() == format
            }) && output.texture.textureType() == MTLTextureType::Type2D
                && output.texture.sampleCount() == 1
                && output.texture.usage().contains(MTLTextureUsage::ShaderRead)
                && (source_sized
                    || (output.texture.width() == extent.width.0 as usize
                        && output.texture.height() == extent.height.0 as usize)),
            "invalid custom texture format, usage, or size"
        );
        validate_commands(context, &output.commands)?;
        Ok(output)
    }

    pub(crate) fn validate_nv12(
        context: &MetalCustomContext,
        raw: Box<dyn std::any::Any>,
    ) -> Result<PreparedMetalOutput> {
        let planes = *raw
            .downcast::<MetalNv12Output>()
            .map_err(|_| anyhow::anyhow!("expected native NV12 output"))?;
        let planes = planes.reusable_for(context)?;
        for (texture, format) in [
            (&planes.y, MTLPixelFormat::R8Unorm),
            (&planes.uv, MTLPixelFormat::RG8Unorm),
        ] {
            ensure!(
                texture.pixelFormat() == format
                    && texture.textureType() == MTLTextureType::Type2D
                    && texture.sampleCount() == 1
                    && texture.usage().contains(MTLTextureUsage::ShaderRead),
                "invalid NV12 texture view"
            );
        }
        ensure!(
            planes.y.width() == planes.uv.width() * 2
                && planes.y.height() == planes.uv.height() * 2,
            "NV12 plane extents do not match"
        );
        Ok(PreparedMetalOutput {
            output: MetalCustomOutput {
                texture: planes.y.clone(),
                commands: vec![],
            },
            nv12: Some(planes),
        })
    }

    pub fn draw(
        &self,
        draw: &CustomDraw,
        output: &impl MetalSampleSource,
        coverage: Option<&TextureRef>,
        native_source: bool,
        viewport: Size<DevicePixels>,
        encoder: &RenderCommandEncoderRef,
    ) {
        // All fields are float4 to make the Rust/MSL layout unambiguous.
        let adjustments = draw.content.image_linear_adjustments();
        let transform = adjustments
            .map(|v| v.rows())
            .unwrap_or_else(|| draw.content.image_color_transform());
        let vignette = adjustments
            .map(|v| v.vignette())
            .unwrap_or([0., 0., 1., 1.]);
        let linear = [
            if adjustments.is_some() { 1. } else { 0. },
            draw.content.image_color_mode().shader_value(),
            if adjustments.is_some_and(|v| v.is_p3()) {
                1.
            } else {
                0.
            },
            0.,
        ];
        let planes = output.nv12_planes();
        let yuv = planes.map(|p| p.rows).unwrap_or([[0.; 4]; 3]);
        let params: [[f32; 4]; 12] = [
            [
                draw.bounds.origin.x.0,
                draw.bounds.origin.y.0,
                draw.bounds.size.width.0,
                draw.bounds.size.height.0,
            ],
            [
                draw.content_mask.bounds.origin.x.0,
                draw.content_mask.bounds.origin.y.0,
                draw.content_mask.bounds.size.width.0,
                draw.content_mask.bounds.size.height.0,
            ],
            [
                draw.corner_radii.top_left.0,
                draw.corner_radii.top_right.0,
                draw.corner_radii.bottom_right.0,
                draw.corner_radii.bottom_left.0,
            ],
            [
                viewport.width.0 as f32,
                viewport.height.0 as f32,
                draw.opacity * draw.content.image_sample_opacity().clamp(0., 1.),
                (if coverage.is_some() { 1. } else { 0. })
                    + (if native_source { 2. } else { 0. })
                    + (if planes.is_some() { 4. } else { 0. }),
            ],
            transform[0],
            transform[1],
            transform[2],
            yuv[0],
            yuv[1],
            yuv[2],
            vignette,
            linear,
        ];
        encoder.setRenderPipelineState(&self.pipeline);
        unsafe {
            encoder.setVertexBytes_length_atIndex(
                std::ptr::NonNull::new(params.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                    .expect("non-null bytes"),
                std::mem::size_of_val(&params) as usize,
                0,
            )
        };
        unsafe {
            encoder.setFragmentBytes_length_atIndex(
                std::ptr::NonNull::new(params.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                    .expect("non-null bytes"),
                std::mem::size_of_val(&params) as usize,
                0,
            )
        };
        unsafe { encoder.setFragmentTexture_atIndex(Some(output.color_texture()), 0) };
        unsafe { encoder.setFragmentTexture_atIndex(coverage.or(Some(output.color_texture())), 1) };
        unsafe {
            encoder.setFragmentTexture_atIndex(
                Some(
                    planes
                        .map(|p| p.uv.as_ref())
                        .unwrap_or(output.color_texture()),
                ),
                2,
            )
        };
        unsafe { encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6) };
    }
}

fn validate_commands(context: &MetalCustomContext, commands: &[CommandBuffer]) -> Result<()> {
    for command in commands {
        let queue = command.commandQueue();
        ensure!(
            Retained::as_ptr(&queue) == Retained::as_ptr(&context.queue),
            "custom command belongs to another queue"
        );
        ensure!(
            command.status() == MTLCommandBufferStatus::NotEnqueued,
            "custom command already submitted"
        );
    }
    Ok(())
}

/// Revalidate the entire producer batch before committing any of its work.
/// Preparation of a later producer must not invalidate an earlier output.
pub(crate) fn submit_custom_commands<'a>(
    context: &MetalCustomContext,
    outputs: impl IntoIterator<Item = &'a MetalCustomOutput>,
) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    let mut pending = Vec::new();
    for output in outputs {
        validate_commands(context, &output.commands)?;
        for command in &output.commands {
            if seen.insert(command.as_ptr() as usize) {
                pending.push(command);
            }
        }
    }
    for command in pending {
        command.commit();
    }
    Ok(())
}

#[cfg(test)]
mod submission_tests;
#[cfg(test)]
mod wide_output_tests;

pub(crate) fn texture(device: &DeviceRef, extent: Size<DevicePixels>) -> Result<Texture> {
    ensure!(
        extent.width.0 > 0
            && extent.height.0 > 0
            && extent.width.0 <= 16384
            && extent.height.0 <= 16384,
        "invalid paint extent"
    );
    ensure!(
        (extent.width.0 as usize) * (extent.height.0 as usize) * 4 <= 64 * 1024 * 1024,
        "paint texture exceeds 64 MiB"
    );
    let desc = TextureDescriptor::new();
    unsafe { desc.setWidth(extent.width.0 as usize) };
    unsafe { desc.setHeight(extent.height.0 as usize) };
    desc.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    desc.setStorageMode(MTLStorageMode::Private);
    Ok(device
        .newTextureWithDescriptor(&desc)
        .expect("Metal texture allocation failed"))
}

pub(crate) fn coverage_scene(draw: &CustomDraw, extent: Size<DevicePixels>) -> Result<gpui::Scene> {
    use gpui::*;
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(
            ScaledPixels(extent.width.0 as f32),
            ScaledPixels(extent.height.0 as f32),
        ),
    );
    let mask = ContentMask { bounds };
    match draw.coverage.as_ref().expect("coverage") {
        CustomCoverage::Border { widths, style } => {
            ensure!(
                [widths.top.0, widths.right.0, widths.bottom.0, widths.left.0]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.),
                "invalid border widths"
            );
            scene.insert_primitive(Quad {
                bounds,
                content_mask: mask,
                background: transparent_black().into(),
                border_color: white(),
                border_widths: *widths,
                corner_radii: draw.corner_radii,
                border_style: *style,
                ..Default::default()
            });
        }
        CustomCoverage::Path(path) => {
            ensure!(path.vertices.len() <= 1_000_000, "coverage path too large");
            let mut path = path.as_ref().clone();
            let sx = bounds.size.width.0 / draw.bounds.size.width.0;
            let sy = bounds.size.height.0 / draw.bounds.size.height.0;
            for vertex in &mut path.vertices {
                vertex.xy_position.x =
                    ScaledPixels((vertex.xy_position.x.0 - draw.bounds.origin.x.0) * sx);
                vertex.xy_position.y =
                    ScaledPixels((vertex.xy_position.y.0 - draw.bounds.origin.y.0) * sy);
                ensure!(
                    [
                        vertex.xy_position.x.0,
                        vertex.xy_position.y.0,
                        vertex.st_position.x,
                        vertex.st_position.y
                    ]
                    .iter()
                    .all(|v| v.is_finite()),
                    "non-finite coverage path"
                );
                vertex.content_mask = mask;
            }
            path.bounds = bounds;
            path.content_mask = mask;
            path.color = white().into();
            scene.insert_primitive(path);
        }
    }
    scene.finish();
    Ok(scene)
}
