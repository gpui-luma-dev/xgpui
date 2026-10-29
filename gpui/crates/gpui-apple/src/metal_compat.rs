//! Temporary, ownership-preserving bridge while the renderer migrates (#73).
//!
//! These casts change the Rust binding for the same Objective-C object. Borrowed
//! views cannot outlive their owner; owned conversions transfer exactly one +1
//! reference. Remove this module when the remaining metal-rs renderer is ported.

use foreign_types::{ForeignType, ForeignTypeRef};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::{MTLCommandBuffer, MTLCommandQueue, MTLDevice, MTLTexture};

pub(crate) fn device(device: &metal::DeviceRef) -> &ProtocolObject<dyn MTLDevice> {
    // SAFETY: metal::DeviceRef wraps a non-null object implementing MTLDevice.
    // The returned borrow is tied to that object's existing owner.
    unsafe { &*device.as_ptr().cast() }
}

pub(crate) fn queue(queue: &metal::CommandQueueRef) -> &ProtocolObject<dyn MTLCommandQueue> {
    // SAFETY: the legacy queue implements MTLCommandQueue and stays borrowed.
    unsafe { &*queue.as_ptr().cast() }
}

pub(crate) fn legacy_texture(texture: &ProtocolObject<dyn MTLTexture>) -> &metal::TextureRef {
    // SAFETY: both bindings describe the same MTLTexture object. No ownership
    // changes; the returned view lives only as long as the objc2 texture borrow.
    unsafe { metal::TextureRef::from_ptr(std::ptr::from_ref(texture).cast_mut().cast()) }
}

pub(crate) fn into_legacy_texture(
    texture: Retained<ProtocolObject<dyn MTLTexture>>,
) -> metal::Texture {
    // SAFETY: into_raw transfers the +1 reference; metal::Texture releases that
    // reference on drop. The object implements the required MTLTexture protocol.
    unsafe { metal::Texture::from_ptr(Retained::into_raw(texture).cast()) }
}

pub(crate) fn into_legacy_command(
    command: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
) -> metal::CommandBuffer {
    // SAFETY: transfer the existing +1 reference, without retaining/releasing it
    // in between, to the legacy owner of the same MTLCommandBuffer object.
    unsafe { metal::CommandBuffer::from_ptr(Retained::into_raw(command).cast()) }
}
