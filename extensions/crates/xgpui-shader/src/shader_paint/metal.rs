use super::*;
use gpui_apple::custom_draw::{MetalCustomContext, MetalCustomOutput};
use gpui_apple::metal::*;
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
            && *key == context.device.registryID()
        {
            return Ok(pipeline.clone());
        }
        let library = context
            .device
            .newLibraryWithSource_options_error(
                &objc2_foundation::NSString::from_str(&program.msl),
                Some(&CompileOptions::new()),
            )
            .map_err(anyhow::Error::msg)
            .context("compiling translated portable MSL")?;
        let descriptor = RenderPipelineDescriptor::new();
        let vertex = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str(
                &program.metal_entries[0],
            ))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str(
                &program.metal_entries[1],
            ))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        descriptor.setVertexFunction(Some(&vertex));
        descriptor.setFragmentFunction(Some(&fragment));
        unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }
            .setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        let pipeline = context
            .device
            .newRenderPipelineStateWithDescriptor_error(&descriptor)
            .map_err(anyhow::Error::msg)?;
        *cache = Some((
            context.device.registryID(),
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
        unsafe { descriptor.setWidth(extent.width.0 as usize) };
        unsafe { descriptor.setHeight(extent.height.0 as usize) };
        descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        descriptor.setStorageMode(MTLStorageMode::Private);
        descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        let texture = context
            .device
            .newTextureWithDescriptor(&descriptor)
            .expect("Metal texture allocation failed");
        let command = context
            .queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        let pass = RenderPassDescriptor::new();
        let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        attachment.setTexture(Some(&texture));
        attachment.setLoadAction(MTLLoadAction::DontCare);
        attachment.setStoreAction(MTLStoreAction::Store);
        let encoder = command
            .renderCommandEncoderWithDescriptor(&pass)
            .expect("Metal render encoder allocation failed");
        encoder.setRenderPipelineState(&pipeline);
        let geometry = self.geometry(request);
        unsafe {
            encoder.setFragmentBytes_length_atIndex(
                std::ptr::NonNull::new(geometry.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                    .expect("non-null bytes"),
                16,
                0,
            )
        };
        unsafe {
            encoder.setFragmentBytes_length_atIndex(
                std::ptr::NonNull::new(self.bytes.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                    .expect("non-null bytes"),
                self.bytes.len() as usize,
                1,
            )
        };
        unsafe { encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6) };
        encoder.endEncoding();
        let bytes: u64 = cache
            .values()
            .map(|v| (v.texture.width() * v.texture.height() * 4) as u64)
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

// SAFETY: cached resources are immutable and access is serialized by the cache mutex.
unsafe impl Send for Cached {}

// SAFETY: pipeline state is immutable; the cache mutex serializes replacement and retains its device/queue.
unsafe impl Send for PipelineCache {}
unsafe impl Sync for PipelineCache {}
