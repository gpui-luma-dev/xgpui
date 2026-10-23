use super::*;
use ::metal::*;
use gpui_apple::custom_draw::{MetalCustomContext, MetalCustomOutput};
use std::{collections::HashMap, sync::Mutex};

#[derive(Default)]
pub(super) struct PipelineCache(Mutex<Option<(u64, RenderPipelineState, Device)>>);
impl PipelineCache {
    fn get(&self, context: &MetalCustomContext, program: &Program) -> Result<RenderPipelineState> {
        let mut cache = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("portable Metal pipeline cache poisoned"))?;
        if let Some((key, pipeline, _)) = &*cache
            && *key == context.device.registry_id()
        {
            return Ok(pipeline.clone());
        }
        let library = context
            .device
            .new_library_with_source(&program.msl, &CompileOptions::new())
            .map_err(anyhow::Error::msg)
            .context("compiling translated portable MSL")?;
        let descriptor = RenderPipelineDescriptor::new();
        let vertex = library
            .get_function(&program.metal_entries[0], None)
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .get_function(&program.metal_entries[1], None)
            .map_err(anyhow::Error::msg)?;
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        descriptor
            .color_attachments()
            .object_at(0)
            .unwrap()
            .set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let pipeline = context
            .device
            .new_render_pipeline_state(&descriptor)
            .map_err(anyhow::Error::msg)?;
        *cache = Some((
            context.device.registry_id(),
            pipeline.clone(),
            context.device.clone(),
        ));
        Ok(pipeline)
    }
}

#[derive(Hash, PartialEq, Eq)]
struct Key {
    queue: u64,
    raster: [i32; 2],
    logical: [u32; 2],
}
struct Cached {
    texture: Texture,
    command: CommandBuffer,
    _queue: CommandQueue,
}
#[derive(Default)]
pub(super) struct OutputCache(Mutex<HashMap<Key, Cached>>);
impl Snapshot {
    pub(super) fn prepare_metal(
        &self,
        context: &MetalCustomContext,
        request: &PreparationRequest,
    ) -> Result<Box<dyn std::any::Any>> {
        let extent = request.raster_size();
        ensure!(
            extent.width.0 as u64 * extent.height.0 as u64 * 4 <= 64 * 1024 * 1024
                && extent.width.0 <= 16384
                && extent.height.0 <= 16384,
            "portable paint extent exceeds budget"
        );
        let key = Key {
            queue: context.cache_key(),
            raster: [extent.width.0, extent.height.0],
            logical: [
                f32::from(request.logical_size().width).to_bits(),
                f32::from(request.logical_size().height).to_bits(),
            ],
        };
        let mut cache = self
            .metal
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("portable Metal output cache poisoned"))?;
        if let Some(output) = cache.get(&key) {
            let commands = match output.command.status() {
                MTLCommandBufferStatus::NotEnqueued => vec![output.command.clone()],
                MTLCommandBufferStatus::Error => bail!("portable shader GPU preparation failed"),
                MTLCommandBufferStatus::Enqueued => {
                    bail!("portable shader command enqueued outside host")
                }
                _ => vec![],
            };
            return Ok(Box::new(MetalCustomOutput {
                texture: output.texture.clone(),
                commands,
            }));
        }
        let pipeline = self
            .program
            .0
            .metal_pipeline
            .get(context, &self.program.0)?;
        let descriptor = TextureDescriptor::new();
        descriptor.set_width(extent.width.0 as u64);
        descriptor.set_height(extent.height.0 as u64);
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        descriptor.set_storage_mode(MTLStorageMode::Private);
        descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        let texture = context.device.new_texture(&descriptor);
        let command = context.queue.new_command_buffer().to_owned();
        let pass = RenderPassDescriptor::new();
        let attachment = pass.color_attachments().object_at(0).unwrap();
        attachment.set_texture(Some(&texture));
        attachment.set_load_action(MTLLoadAction::DontCare);
        attachment.set_store_action(MTLStoreAction::Store);
        let encoder = command.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&pipeline);
        let geometry = self.geometry(request);
        encoder.set_fragment_bytes(0, 16, geometry.as_ptr().cast());
        encoder.set_fragment_bytes(1, self.bytes.len() as u64, self.bytes.as_ptr().cast());
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
        encoder.end_encoding();
        let bytes: u64 = cache
            .values()
            .map(|v| v.texture.width() * v.texture.height() * 4)
            .sum();
        if bytes + extent.width.0 as u64 * extent.height.0 as u64 * 4 > 64 * 1024 * 1024 {
            cache.clear();
        }
        cache.insert(
            key,
            Cached {
                texture: texture.clone(),
                command: command.clone(),
                _queue: context.queue.clone(),
            },
        );
        Ok(Box::new(MetalCustomOutput {
            texture,
            commands: vec![command],
        }))
    }
}
