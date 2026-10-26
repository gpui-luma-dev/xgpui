//! Metal integration for retained, independently rendered GPU content.
pub mod core_video;
mod coverage_cache;
use anyhow::{Result, ensure};
pub(crate) use coverage_cache::{CoverageCache, CoverageKey};
use foreign_types::ForeignType;
use gpui::{CustomDraw, DevicePixels, Size, size};
use metal::*;

/// Borrowed by producers during preparation. Handles belong to the host device.
#[derive(Clone)]
pub struct MetalCustomContext {
    /// Active host device; create all producer resources on this device.
    pub device: Device,
    /// Host queue for recording commands. Only the host commits them.
    pub queue: CommandQueue,
}

impl MetalCustomContext {
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
            self.output.texture.device().registry_id() == context.device.registry_id(),
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
            self.queue.device().registry_id() == self.device.registry_id(),
            "host queue belongs to another device"
        );
        let bytes = data.staging_byte_len();
        let allocation = pool.buffers.checkout(bytes as u64, bytes, || {
            Ok(self
                .device
                .new_buffer(bytes as u64, MTLResourceOptions::StorageModeShared))
        })?;
        let desc = TextureDescriptor::new();
        desc.set_width(data.width() as u64);
        desc.set_height(data.height() as u64);
        desc.set_mipmap_level_count(data.levels().len() as u64);
        desc.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
        desc.set_storage_mode(MTLStorageMode::Private);
        desc.set_usage(MTLTextureUsage::ShaderRead);
        let texture = self.device.new_texture(&desc);
        let command = self.queue.new_command_buffer().to_owned();
        let encoder = command.new_blit_command_encoder();
        let buffer = allocation.resource();
        // The exclusive lease is never in the cache while a worker or a pending
        // GPU command owns it. Completion, not queue submission, permits refill.
        let mapped =
            unsafe { std::slice::from_raw_parts_mut(buffer.contents().cast::<u8>(), bytes) };
        let mut offset = 0usize;
        for (level, pixels) in data.levels().iter().enumerate() {
            let width = (data.width() >> level).max(1) as u64;
            let height = (data.height() >> level).max(1) as u64;
            let row = (width * 4).div_ceil(256) * 256;
            for y in 0..height as usize {
                mapped[offset + y * row as usize..offset + y * row as usize + width as usize * 4]
                    .copy_from_slice(&pixels[y * width as usize * 4..(y + 1) * width as usize * 4]);
            }
            encoder.copy_from_buffer_to_texture(
                buffer,
                offset as u64,
                row,
                row * height,
                MTLSize::new(width, height, 1),
                &texture,
                0,
                level as u64,
                MTLOrigin { x: 0, y: 0, z: 0 },
                MTLBlitOption::empty(),
            );
            offset += row as usize * height as usize;
        }
        encoder.end_encoding();
        let completion = std::sync::Mutex::new(Some((allocation, guard)));
        let block = block2::RcBlock::new(move |_: std::ptr::NonNull<objc2::runtime::AnyObject>| {
            completion.lock().unwrap().take();
        });
        // SAFETY: Both views use the Objective-C block pointer ABI. Metal calls
        // the completion block only after this upload can no longer use the buffer.
        unsafe {
            command.add_completed_handler(&*block2::RcBlock::as_ptr(&block).cast());
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
    /// are errors. No CPU wait is introduced. Like the legacy protocol, host-only
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
            context.queue.device().registry_id() == context.device.registry_id(),
            "host queue belongs to another device"
        );
        let t = &self.output.texture;
        ensure!(
            t.device().registry_id() == context.device.registry_id(),
            "wide texture belongs to another device"
        );
        ensure!(
            t.pixel_format() == MTLPixelFormat::RGBA16Float
                && t.texture_type() == MTLTextureType::D2
                && t.sample_count() == 1
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
            self.y.device().registry_id() == context.device.registry_id()
                && self.uv.device().registry_id() == context.device.registry_id(),
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
            .new_library_with_source(include_str!("custom_draw.metal"), &CompileOptions::new())
            .map_err(anyhow::Error::msg)?;
        let descriptor = RenderPipelineDescriptor::new();
        let vertex = library
            .get_function("custom_vertex", None)
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .get_function("custom_fragment", None)
            .map_err(anyhow::Error::msg)?;
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        let color = descriptor.color_attachments().object_at(0).unwrap();
        color.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        color.set_blending_enabled(true);
        color.set_source_rgb_blend_factor(MTLBlendFactor::SourceAlpha);
        color.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        color.set_source_alpha_blend_factor(MTLBlendFactor::One);
        color.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        let pipeline = device
            .new_render_pipeline_state(&descriptor)
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
            output.texture.device().registry_id() == context.device.registry_id(),
            "custom texture belongs to another device"
        );
        ensure!(
            (if source_sized {
                matches!(
                    output.texture.pixel_format(),
                    MTLPixelFormat::RGBA8Unorm | MTLPixelFormat::BGRA8Unorm
                )
            } else {
                output.texture.pixel_format() == format
            }) && output.texture.texture_type() == MTLTextureType::D2
                && output.texture.sample_count() == 1
                && output.texture.usage().contains(MTLTextureUsage::ShaderRead)
                && (source_sized
                    || (output.texture.width() == extent.width.0 as u64
                        && output.texture.height() == extent.height.0 as u64)),
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
                texture.pixel_format() == format
                    && texture.texture_type() == MTLTextureType::D2
                    && texture.sample_count() == 1
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
        let transform = draw.content.image_color_transform();
        let planes = output.nv12_planes();
        let yuv = planes.map(|p| p.rows).unwrap_or([[0.; 4]; 3]);
        let params: [[f32; 4]; 10] = [
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
                draw.opacity,
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
        ];
        encoder.set_render_pipeline_state(&self.pipeline);
        encoder.set_vertex_bytes(
            0,
            std::mem::size_of_val(&params) as u64,
            params.as_ptr().cast(),
        );
        encoder.set_fragment_bytes(
            0,
            std::mem::size_of_val(&params) as u64,
            params.as_ptr().cast(),
        );
        encoder.set_fragment_texture(0, Some(output.color_texture()));
        encoder.set_fragment_texture(1, coverage.or(Some(output.color_texture())));
        encoder.set_fragment_texture(
            2,
            Some(
                planes
                    .map(|p| p.uv.as_ref())
                    .unwrap_or(output.color_texture()),
            ),
        );
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
    }
}

fn validate_commands(context: &MetalCustomContext, commands: &[CommandBuffer]) -> Result<()> {
    for command in commands {
        // metal-rs does not bind MTLCommandBuffer.commandQueue. Both objects
        // remain retained while comparing the borrowed queue's identity.
        let queue: *mut objc2::runtime::AnyObject = unsafe {
            let object = &*command.as_ptr().cast::<objc2::runtime::AnyObject>();
            objc2::msg_send![object, commandQueue]
        };
        ensure!(
            queue.cast::<std::ffi::c_void>() == context.queue.as_ptr().cast(),
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
        (extent.width.0 as u64) * (extent.height.0 as u64) * 4 <= 64 * 1024 * 1024,
        "paint texture exceeds 64 MiB"
    );
    let desc = TextureDescriptor::new();
    desc.set_width(extent.width.0 as u64);
    desc.set_height(extent.height.0 as u64);
    desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    desc.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    desc.set_storage_mode(MTLStorageMode::Private);
    Ok(device.new_texture(&desc))
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
