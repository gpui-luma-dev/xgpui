//! Native objc2 Metal resource types used by the renderer and producers.
use objc2::{rc::Retained, runtime::ProtocolObject};
pub use objc2_foundation::{NSRange, NSUInteger};
pub use objc2_metal::*;
pub use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
pub type Device = Retained<ProtocolObject<dyn MTLDevice>>;
pub type DeviceRef = ProtocolObject<dyn MTLDevice>;
pub type CommandQueue = Retained<ProtocolObject<dyn MTLCommandQueue>>;
pub type CommandQueueRef = ProtocolObject<dyn MTLCommandQueue>;
pub type CommandBuffer = Retained<ProtocolObject<dyn MTLCommandBuffer>>;
pub type CommandBufferRef = ProtocolObject<dyn MTLCommandBuffer>;
#[derive(Clone, Debug)]
pub struct Buffer(pub Retained<ProtocolObject<dyn MTLBuffer>>);
impl std::ops::Deref for Buffer {
    type Target = ProtocolObject<dyn MTLBuffer>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
// SAFETY: CPU buffer access is controlled by exclusive staging-pool leases and
// the renderer's pool mutex; GPU ownership is retained through completion.
unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}
impl AsRef<BufferRef> for Buffer {
    fn as_ref(&self) -> &BufferRef {
        &self.0
    }
}

pub type BufferRef = ProtocolObject<dyn MTLBuffer>;
pub type Texture = Retained<ProtocolObject<dyn MTLTexture>>;
pub type TextureRef = ProtocolObject<dyn MTLTexture>;
pub type Library = Retained<ProtocolObject<dyn MTLLibrary>>;
pub type LibraryRef = ProtocolObject<dyn MTLLibrary>;
pub type Function = Retained<ProtocolObject<dyn MTLFunction>>;
pub type FunctionRef = ProtocolObject<dyn MTLFunction>;
pub type RenderPipelineState = Retained<ProtocolObject<dyn MTLRenderPipelineState>>;
pub type RenderPipelineStateRef = ProtocolObject<dyn MTLRenderPipelineState>;
pub type ComputePipelineState = Retained<ProtocolObject<dyn MTLComputePipelineState>>;
pub type ComputePipelineStateRef = ProtocolObject<dyn MTLComputePipelineState>;
pub type RenderCommandEncoder = Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>;
pub type RenderCommandEncoderRef = ProtocolObject<dyn MTLRenderCommandEncoder>;
pub type BlitCommandEncoder = Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>;
pub type BlitCommandEncoderRef = ProtocolObject<dyn MTLBlitCommandEncoder>;
pub type ComputeCommandEncoder = Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>;
pub type ComputeCommandEncoderRef = ProtocolObject<dyn MTLComputeCommandEncoder>;
pub type SharedEvent = Retained<ProtocolObject<dyn MTLSharedEvent>>;
pub type SharedEventRef = ProtocolObject<dyn MTLSharedEvent>;
pub type TextureDescriptor = MTLTextureDescriptor;
pub type CompileOptions = MTLCompileOptions;
pub type RenderPipelineDescriptor = MTLRenderPipelineDescriptor;
pub type RenderPassDescriptor = MTLRenderPassDescriptor;
pub type VertexDescriptor = MTLVertexDescriptor;
pub type MetalLayer = Retained<CAMetalLayer>;
pub type MetalLayerRef = CAMetalLayer;

/// Object identity only; never transfers ownership.
pub trait ObjectIdentity: objc2::Message {
    fn as_ptr(&self) -> *mut objc2::runtime::AnyObject {
        std::ptr::from_ref(self).cast_mut().cast()
    }
}
impl<T: objc2::Message> ObjectIdentity for T {}
