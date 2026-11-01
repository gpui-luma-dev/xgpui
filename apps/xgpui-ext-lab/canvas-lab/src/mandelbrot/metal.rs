//! Lab-only whole-viewport Metal compute. Outputs remain on the host device;
//! only the host submits commands. No readback or completion wait in production.
use super::*;
use anyhow::{Context as _, Result, ensure};
use gpui_apple::{
    custom_draw::{MetalCustomContext, MetalWideSdrOutput},
    metal::*,
};
use objc2_foundation::NSString;
use std::{any::Any, sync::Mutex};

#[derive(Default)]
pub(super) struct Engine {
    pipeline: Mutex<Option<(u64, ComputePipelineState)>>,
}
impl Engine {
    fn pipeline(&self, context: &MetalCustomContext) -> Result<ComputePipelineState> {
        let mut cached = self
            .pipeline
            .lock()
            .map_err(|_| anyhow::anyhow!("fractal pipeline poisoned"))?;
        if let Some((key, pipeline)) = cached.as_ref()
            && *key == context.cache_key()
        {
            return Ok(pipeline.clone());
        }
        let options = MTLCompileOptions::new();
        #[allow(deprecated)]
        options.setFastMathEnabled(false);
        let library = context
            .objc2_device()
            .newLibraryWithSource_options_error(
                &NSString::from_str(include_str!("viewport.metal")),
                Some(&options),
            )
            .context("compile Mandelbrot kernel")?;
        let function = library
            .newFunctionWithName(&NSString::from_str("mandelbrot_viewport"))
            .context("Mandelbrot kernel missing")?;
        let pipeline = context
            .objc2_device()
            .newComputePipelineStateWithFunction_error(&function)
            .context("create Mandelbrot compute pipeline")?;
        *cached = Some((context.cache_key(), pipeline.clone()));
        Ok(pipeline)
    }
}
struct Cached {
    output: MetalWideSdrOutput,
    command: CommandBuffer,
}
// SAFETY: Metal resources are retained, cached behind a mutex and never mutated
// after recording. The host alone submits the command on its original queue.
unsafe impl Send for Cached {}

pub(super) struct Frame {
    request: Request,
    engine: Arc<Engine>,
    cached: Mutex<Option<Cached>>,
    failure: Mutex<Option<String>>,
}
impl Frame {
    pub fn new(request: Request, engine: Arc<Engine>) -> Self {
        Self {
            request,
            engine,
            cached: Mutex::new(None),
            failure: Mutex::new(None),
        }
    }
    pub fn ready(&self) -> bool {
        self.cached
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|cache| cache.command.status() == MTLCommandBufferStatus::Completed)
    }
    pub fn error(&self) -> Option<String> {
        if let Some(cache) = self.cached.lock().unwrap().as_ref()
            && cache.command.status() == MTLCommandBufferStatus::Error
        {
            return Some("Mandelbrot GPU command failed".into());
        }
        self.failure.lock().unwrap().clone()
    }
    pub fn prepare(&self, backend: &dyn Any) -> Result<Box<dyn Any>> {
        let result = self.prepare_inner(backend);
        if let Err(error) = &result {
            *self.failure.lock().unwrap() = Some(error.to_string());
        }
        result
    }
    fn prepare_inner(&self, backend: &dyn Any) -> Result<Box<dyn Any>> {
        let context = backend
            .downcast_ref::<MetalCustomContext>()
            .context("native Mandelbrot GPU requires Metal")?;
        let mut cached = self
            .cached
            .lock()
            .map_err(|_| anyhow::anyhow!("Mandelbrot frame poisoned"))?;
        if let Some(cache) = cached.as_ref() {
            return Ok(Box::new(cache.output.reusable_for(context)?));
        }
        let pipeline = self.engine.pipeline(context)?;
        let [width, height] = self.request.dimensions;
        ensure!(
            width > 0 && height > 0 && width <= 1920 && height <= 1920,
            "invalid Mandelbrot frame dimensions"
        );
        let descriptor = MTLTextureDescriptor::new();
        // SAFETY: positive bounded sizes validated above; allocation failure is handled.
        unsafe {
            descriptor.setWidth(width as usize);
            descriptor.setHeight(height as usize);
        }
        descriptor.setPixelFormat(MTLPixelFormat::RGBA16Float);
        descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
        descriptor.setStorageMode(MTLStorageMode::Private);
        let texture = context
            .objc2_device()
            .newTextureWithDescriptor(&descriptor)
            .context("allocate Mandelbrot viewport")?;
        let command = context
            .objc2_queue()
            .commandBuffer()
            .context("allocate Mandelbrot commands")?;
        encode(&command, &pipeline, &texture, self.request)?;
        let output = MetalWideSdrOutput::from_objc2_linear_p3_premultiplied(
            context,
            texture,
            vec![command.clone()],
        )?;
        let returned = output.reusable_for(context)?;
        *cached = Some(Cached { output, command });
        Ok(Box::new(returned))
    }
}
#[repr(C)]
struct Parameters {
    coordinates: [f32; 4],
    limit: u32,
    padding: [u32; 3],
}
fn encode(
    command: &CommandBufferRef,
    pipeline: &ComputePipelineStateRef,
    texture: &TextureRef,
    request: Request,
) -> Result<()> {
    let view = request.view;
    let parameters = Parameters {
        coordinates: [
            (view.center.x / 300. - 0.5) as f32,
            (view.center.y / 300.) as f32,
            (view.viewport.x * request.margin() / (view.zoom * 300.)) as f32,
            (view.viewport.y * request.margin() / (view.zoom * 300.)) as f32,
        ],
        limit: request.limit,
        padding: [0; 3],
    };
    let encoder = command
        .computeCommandEncoder()
        .context("create Mandelbrot encoder")?;
    encoder.setComputePipelineState(pipeline);
    // SAFETY: setBytes copies this fully initialized repr(C) record immediately;
    // index 0 and texture 0 match the kernel declarations. Commands retain texture.
    unsafe {
        encoder.setBytes_length_atIndex(
            std::ptr::NonNull::from(&parameters).cast(),
            std::mem::size_of::<Parameters>(),
            0,
        );
        encoder.setTexture_atIndex(Some(texture), 0);
    }
    encoder.dispatchThreads_threadsPerThreadgroup(
        MTLSize {
            width: request.dimensions[0] as usize,
            height: request.dimensions[1] as usize,
            depth: 1,
        },
        MTLSize {
            width: 8,
            height: 8,
            depth: 1,
        },
    );
    encoder.endEncoding();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    #[ignore = "manual local GPU timing; not a performance assertion"]
    fn full_resolution_gpu_timing() {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let queue = device.newCommandQueue().unwrap();
        let context = MetalCustomContext { device, queue };
        let engine = Arc::new(Engine::default());
        for zoom in [1., 32., 64.] {
            let view = View {
                center: WorldPoint::new(-75., 30.),
                zoom,
                viewport: WorldPoint::new(1100., 634.),
                scale: 2.,
            };
            for _ in 0..3 {
                let frame = Frame::new(Request::new(view, true, true), engine.clone());
                let _output = frame.prepare(&context).unwrap();
                let command = frame
                    .cached
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .command
                    .clone();
                // This manual test owns its queue; production is host-submitted.
                command.commit();
                command.waitUntilCompleted();
                assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
                eprintln!(
                    "zoom {zoom}: {:.3} ms GPU",
                    (command.GPUEndTime() - command.GPUStartTime()) * 1000.
                );
            }
        }
    }
    #[::core::prelude::v1::test]
    fn metal_viewport_matches_f64_reference_at_stable_points() {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let queue = device.newCommandQueue().unwrap();
        let context = MetalCustomContext { device, queue };
        let pipeline = Engine::default().pipeline(&context).unwrap();
        for (real, imaginary) in [(0., 0.), (-1., 0.), (2., 2.), (0.5, 0.5), (-0.25, 0.5)] {
            let view = View {
                center: WorldPoint::new((real + 0.5) * 300., imaginary * 300.),
                zoom: 64.,
                viewport: WorldPoint::new(0.1, 0.1),
                scale: 1.,
            };
            let request = Request {
                dimensions: [3, 3],
                ..Request::new(view, false, true)
            };
            let descriptor = MTLTextureDescriptor::new();
            unsafe {
                descriptor.setWidth(3);
                descriptor.setHeight(3);
            }
            descriptor.setPixelFormat(MTLPixelFormat::RGBA32Float);
            descriptor.setUsage(MTLTextureUsage::ShaderWrite | MTLTextureUsage::ShaderRead);
            descriptor.setStorageMode(MTLStorageMode::Shared);
            let texture = context
                .objc2_device()
                .newTextureWithDescriptor(&descriptor)
                .unwrap();
            let command = context.objc2_queue().commandBuffer().unwrap();
            encode(&command, &pipeline, &texture, request).unwrap();
            // This test owns its queue. Production producers never commit/wait.
            command.commit();
            command.waitUntilCompleted();
            assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
            let mut pixels = [[0_f32; 4]; 9];
            unsafe {
                texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                    std::ptr::NonNull::new(pixels.as_mut_ptr()).unwrap().cast(),
                    3 * 16,
                    MTLRegion {
                        origin: MTLOrigin { x: 0, y: 0, z: 0 },
                        size: MTLSize {
                            width: 3,
                            height: 3,
                            depth: 1,
                        },
                    },
                    0,
                );
            }
            let reference = render_cpu(request, &AtomicU64::new(1), 1).unwrap();
            for (actual, expected) in pixels.iter().zip(reference) {
                for channel in 0..4 {
                    assert!(
                        (actual[channel] - expected[channel]).abs() < 3e-4,
                        "{real},{imaginary}: {actual:?} vs {expected:?}"
                    );
                }
            }
        }
    }
}

// Small interactive CPU frames do not need the media loader's multi-stage
// background staging lifecycle. Convert pixels off-thread, then record one
// source-sized upload on the host queue; the displayed front remains unchanged.
pub(super) struct CpuFrame {
    data: ImageTextureData,
    cached: Mutex<Option<gpui_apple::custom_draw::MetalImageOutput>>,
    failure: Mutex<Option<String>>,
}
impl CpuFrame {
    pub fn new(data: ImageTextureData) -> Self {
        Self {
            data,
            cached: Mutex::new(None),
            failure: Mutex::new(None),
        }
    }
    pub fn ready(&self) -> bool {
        self.cached
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|output| output.is_submitted())
    }
    pub fn error(&self) -> Option<String> {
        self.failure.lock().unwrap().clone()
    }
    pub fn prepare(&self, backend: &dyn Any) -> Result<Box<dyn Any>> {
        let result = self.prepare_inner(backend);
        if let Err(error) = &result {
            *self.failure.lock().unwrap() = Some(error.to_string());
        }
        result
    }
    fn prepare_inner(&self, backend: &dyn Any) -> Result<Box<dyn Any>> {
        let context = backend
            .downcast_ref::<MetalCustomContext>()
            .context("native CPU frame upload requires Metal")?;
        let mut cached = self
            .cached
            .lock()
            .map_err(|_| anyhow::anyhow!("CPU frame upload poisoned"))?;
        if let Some(output) = cached.as_ref() {
            return Ok(Box::new(output.reusable_for(context)?));
        }
        let output = context.create_image_output(&self.data)?;
        let returned = output.reusable_for(context)?;
        *cached = Some(output);
        Ok(Box::new(returned))
    }
}
