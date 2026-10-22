//! Metal integration for retained, independently rendered GPU content.
use anyhow::{Result, ensure};
use foreign_types::ForeignType;
use gpui::{CustomDraw, DevicePixels, Size, size};
use metal::*;

/// Borrowed by producers during preparation. Handles belong to the host device.
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
pub struct MetalCustomOutput {
    /// Immutable sampleable output matching the requested physical extent.
    pub texture: Texture,
    /// Pending preparation commands, or empty for already-submitted cached output.
    pub commands: Vec<CommandBuffer>,
}

pub(crate) struct CustomCompositor {
    pipeline: RenderPipelineState,
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
        Ok(Self { pipeline })
    }

    pub fn prepare(context: &MetalCustomContext, draw: &CustomDraw) -> Result<MetalCustomOutput> {
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
        let extent = size(DevicePixels(width as i32), DevicePixels(height as i32));
        let output = draw.content.prepare(context, extent)?;
        let output = *output
            .downcast::<MetalCustomOutput>()
            .map_err(|_| anyhow::anyhow!("producer returned a non-Metal output"))?;
        ensure!(
            output.texture.device().registry_id() == context.device.registry_id(),
            "custom texture belongs to another device"
        );
        ensure!(
            output.texture.pixel_format() == MTLPixelFormat::BGRA8Unorm
                && output.texture.texture_type() == MTLTextureType::D2
                && output.texture.sample_count() == 1
                && output.texture.usage().contains(MTLTextureUsage::ShaderRead)
                && output.texture.width() == extent.width.0 as u64
                && output.texture.height() == extent.height.0 as u64,
            "invalid custom texture format, usage, or size"
        );
        for command in &output.commands {
            ensure!(
                command.status() == MTLCommandBufferStatus::NotEnqueued,
                "custom command already submitted"
            );
        }
        Ok(output)
    }

    pub fn draw(
        &self,
        draw: &CustomDraw,
        output: &MetalCustomOutput,
        viewport: Size<DevicePixels>,
        encoder: &RenderCommandEncoderRef,
    ) {
        // All fields are float4 to make the Rust/MSL layout unambiguous.
        let params: [[f32; 4]; 4] = [
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
                0.,
            ],
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
        encoder.set_fragment_texture(0, Some(&output.texture));
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
    }
}
