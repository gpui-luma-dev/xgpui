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
/// Cached output with no commands must already be ready or have been submitted
/// earlier on the host queue. The host does not wait for CPU-observed completion.
pub struct MetalCustomOutput {
    /// Immutable sampleable output matching the requested physical extent.
    pub texture: Texture,
    /// Pending preparation commands, or empty for already-submitted cached output.
    pub commands: Vec<CommandBuffer>,
}

pub(crate) struct CustomCompositor {
    pipeline: RenderPipelineState,
    pub(crate) masks: std::collections::HashMap<Vec<u32>, Texture>,
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
        let extent = Self::extent(draw)?;
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
        validate_commands(context, &output.commands)?;
        Ok(output)
    }

    pub fn draw(
        &self,
        draw: &CustomDraw,
        output: &MetalCustomOutput,
        coverage: Option<&TextureRef>,
        native_source: bool,
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
                (if coverage.is_some() { 1. } else { 0. }) + (if native_source { 2. } else { 0. }),
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
        encoder.set_fragment_texture(1, coverage.or(Some(&output.texture)));
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

/// Exact normalized geometry key: clipping, screen origin and producer are excluded.
pub(crate) fn coverage_key(draw: &CustomDraw) -> Vec<u32> {
    let mut key = vec![
        draw.bounds.size.width.0.to_bits(),
        draw.bounds.size.height.0.to_bits(),
    ];
    for r in [
        draw.corner_radii.top_left,
        draw.corner_radii.top_right,
        draw.corner_radii.bottom_right,
        draw.corner_radii.bottom_left,
    ] {
        key.push(r.0.to_bits());
    }
    match draw.coverage.as_ref().expect("coverage") {
        gpui::CustomCoverage::Border { widths, style } => {
            key.extend([0, *style as u32]);
            for w in [widths.top, widths.right, widths.bottom, widths.left] {
                key.push(w.0.to_bits());
            }
        }
        gpui::CustomCoverage::Path(path) => {
            key.push(1);
            for v in &path.vertices {
                key.extend([
                    (v.xy_position.x.0 - draw.bounds.origin.x.0).to_bits(),
                    (v.xy_position.y.0 - draw.bounds.origin.y.0).to_bits(),
                    v.st_position.x.to_bits(),
                    v.st_position.y.to_bits(),
                ]);
            }
        }
    }
    key
}

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
