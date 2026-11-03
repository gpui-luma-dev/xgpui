use super::*;
use crate::metal::*;
use gpui::{Bounds, ContentMask, Corners, CustomContent, ScaledPixels, point};
use std::sync::Arc;

#[derive(Debug)]
struct Unused;
impl CustomContent for Unused {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        unreachable!()
    }
}

fn draw() -> CustomDraw {
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::default(),
        opacity: 1.,
        content: Arc::new(Unused),
        coverage: None,
    }
}

fn with_context(test: impl FnOnce(MetalCustomContext)) {
    objc2::rc::autoreleasepool(|_| {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let queue = device
            .newCommandQueue()
            .expect("Metal queue allocation failed");
        test(MetalCustomContext::new(device, queue));
    });
}

fn output(context: &MetalCustomContext, commands: Vec<CommandBuffer>) -> MetalCustomOutput {
    MetalCustomOutput {
        texture: texture(&context.device, size(DevicePixels(8), DevicePixels(8))).unwrap(),
        commands,
    }
}

fn validate(context: &MetalCustomContext, output: MetalCustomOutput) -> Result<MetalCustomOutput> {
    CustomCompositor::validate(context, &draw(), Box::new(output))
}

#[test]
fn submission_accepts_host_queue_and_commits_shared_buffer_once() {
    with_context(|context| {
        let command = context
            .queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        let output = validate(
            &context,
            output(&context, vec![command.clone(), command.clone()]),
        )
        .unwrap();
        // Duplicate both inside a result and across results. A second commit
        // would be rejected by Metal API validation.
        submit_custom_commands(&context, [&output, &output]).unwrap();
        command.waitUntilCompleted();
        assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
    });
}

#[test]
fn submission_rejects_another_queue_on_the_same_device() {
    with_context(|context| {
        let other_queue = context
            .device
            .newCommandQueue()
            .expect("Metal queue allocation failed");
        let command = other_queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        let error = validate(&context, output(&context, vec![command.clone()]))
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "custom command belongs to another queue");
        assert_eq!(command.status(), MTLCommandBufferStatus::NotEnqueued);
    });
}

#[test]
fn submission_rejects_enqueued_and_completed_commands() {
    with_context(|context| {
        let command = context
            .queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        command.enqueue();
        let result = validate(&context, output(&context, vec![command.clone()]));
        // Commit even if the assertion fails, to release this queue's slot.
        command.commit();
        command.waitUntilCompleted();
        assert_eq!(
            result.err().unwrap().to_string(),
            "custom command already submitted"
        );
        let error = validate(&context, output(&context, vec![command]))
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "custom command already submitted");
    });
}

#[test]
fn submission_validates_whole_batch_before_committing_any_commands() {
    with_context(|context| {
        let good_command = context
            .queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        let good = output(&context, vec![good_command.clone()]);
        let other_queue = context
            .device
            .newCommandQueue()
            .expect("Metal queue allocation failed");
        let bad_command = other_queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        let bad = output(&context, vec![bad_command.clone()]);
        assert!(submit_custom_commands(&context, [&good, &bad]).is_err());
        assert_eq!(good_command.status(), MTLCommandBufferStatus::NotEnqueued);
        assert_eq!(bad_command.status(), MTLCommandBufferStatus::NotEnqueued);
    });
}

#[test]
fn submission_rechecks_command_state_after_preparation() {
    with_context(|context| {
        let command = context
            .queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        let output = validate(&context, output(&context, vec![command.clone()])).unwrap();
        command.enqueue(); // Simulate a later producer violating ownership.
        let result = submit_custom_commands(&context, [&output]);
        command.commit();
        command.waitUntilCompleted();
        assert_eq!(
            result.unwrap_err().to_string(),
            "custom command already submitted"
        );
    });
}

#[test]
fn submission_rejects_invalid_texture_extent_and_format() {
    with_context(|context| {
        for (width, format) in [
            (9, MTLPixelFormat::BGRA8Unorm),
            (8, MTLPixelFormat::RGBA8Unorm),
        ] {
            let descriptor = TextureDescriptor::new();
            unsafe { descriptor.setWidth(width) };
            unsafe { descriptor.setHeight(8) };
            descriptor.setPixelFormat(format);
            descriptor.setUsage(MTLTextureUsage::ShaderRead);
            let output = MetalCustomOutput {
                texture: context
                    .device
                    .newTextureWithDescriptor(&descriptor)
                    .expect("Metal texture allocation failed"),
                commands: vec![],
            };
            assert_eq!(
                validate(&context, output).err().unwrap().to_string(),
                "invalid custom texture format, usage, or size"
            );
        }
    });
}

#[test]
#[cfg(target_os = "macos")]
fn submission_rejects_foreign_device_when_available() {
    with_context(|context| {
        let Some(other) = MTLCopyAllDevices()
            .to_vec()
            .into_iter()
            .find(|device| device.registryID() != context.device.registryID())
        else {
            eprintln!("foreign-device GPU test unavailable: this machine has one Metal device");
            return;
        };
        let other_context = MetalCustomContext::new(
            other.clone(),
            other
                .newCommandQueue()
                .expect("Metal queue allocation failed"),
        );
        let output = output(&other_context, vec![]);
        assert_eq!(
            validate(&context, output).err().unwrap().to_string(),
            "custom texture belongs to another device"
        );
        let command = other_context
            .queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed")
            .to_owned();
        assert!(validate_commands(&context, &[command]).is_err());
    });
}

#[test]
fn metal_staging_pool_returns_buffer_after_gpu_completion() {
    with_context(|context| {
        let pool = context.image_staging_pool(768);
        let data = gpui::ImageTextureData::new(2, 2, vec![vec![255; 16], vec![255; 4]]).unwrap();
        let staged = context
            .stage_image_upload_with_pool(&data, &pool, None)
            .unwrap();
        assert_eq!(pool.stats().cached_bytes, 0);
        assert!(
            context
                .stage_image_upload_with_pool(&data, &pool, None)
                .is_err()
        );
        let output = CustomCompositor::validate(&context, &draw(), Box::new(staged)).unwrap();
        submit_custom_commands(&context, [&output]).unwrap();
        output.commands[0].waitUntilCompleted();
        assert_eq!(pool.stats().cached_bytes, 768);
        let _next = context
            .stage_image_upload_with_pool(&data, &pool, None)
            .unwrap();
        assert_eq!(pool.stats().allocations, 1);
        assert_eq!(pool.stats().reuses, 1);
    });
}

#[test]
fn host_recording_bounds_unsubmitted_buffers_and_rotates_after_submission() {
    with_context(|context| {
        let first = context.record(|_| Ok(())).unwrap();
        for _ in 0..1024 {
            let next = context.record(|_| Ok(())).unwrap();
            assert_eq!(first.as_ptr(), next.as_ptr());
        }
        let output = output(&context, vec![first.clone()]);
        submit_custom_commands(&context, [&output]).unwrap();
        let next = context.record(|_| Ok(())).unwrap();
        assert_ne!(first.as_ptr(), next.as_ptr());
        first.waitUntilCompleted();
        assert_eq!(first.status(), MTLCommandBufferStatus::Completed);
    });
}

#[test]
fn upload_worker_cannot_encode_into_the_ui_preparation_batch() {
    with_context(|context| {
        let worker = context.clone();
        let error = std::thread::spawn(move || {
            worker
                .record(|_| panic!("wrong-thread encoder invoked"))
                .err()
                .unwrap()
                .to_string()
        })
        .join()
        .unwrap();
        assert!(error.contains("preparation thread"));
        assert!(context.record(|_| Ok(())).is_ok());
    });
}

#[test]
fn recursive_recording_returns_an_error_without_blocking_the_ui() {
    with_context(|context| {
        context
            .record(|_| {
                assert!(context.record(|_| Ok(())).is_err());
                Ok(())
            })
            .unwrap();
    });
}
