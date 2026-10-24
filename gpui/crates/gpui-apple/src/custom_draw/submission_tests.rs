use super::*;
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
        let device = Device::system_default().expect("Metal device");
        let queue = device.new_command_queue();
        test(MetalCustomContext { device, queue });
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
        let command = context.queue.new_command_buffer().to_owned();
        let output = validate(
            &context,
            output(&context, vec![command.clone(), command.clone()]),
        )
        .unwrap();
        // Duplicate both inside a result and across results. A second commit
        // would be rejected by Metal API validation.
        submit_custom_commands(&context, [&output, &output]).unwrap();
        command.wait_until_completed();
        assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
    });
}

#[test]
fn submission_rejects_another_queue_on_the_same_device() {
    with_context(|context| {
        let other_queue = context.device.new_command_queue();
        let command = other_queue.new_command_buffer().to_owned();
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
        let command = context.queue.new_command_buffer().to_owned();
        command.enqueue();
        let result = validate(&context, output(&context, vec![command.clone()]));
        // Commit even if the assertion fails, to release this queue's slot.
        command.commit();
        command.wait_until_completed();
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
        let good_command = context.queue.new_command_buffer().to_owned();
        let good = output(&context, vec![good_command.clone()]);
        let other_queue = context.device.new_command_queue();
        let bad_command = other_queue.new_command_buffer().to_owned();
        let bad = output(&context, vec![bad_command.clone()]);
        assert!(submit_custom_commands(&context, [&good, &bad]).is_err());
        assert_eq!(good_command.status(), MTLCommandBufferStatus::NotEnqueued);
        assert_eq!(bad_command.status(), MTLCommandBufferStatus::NotEnqueued);
    });
}

#[test]
fn submission_rechecks_command_state_after_preparation() {
    with_context(|context| {
        let command = context.queue.new_command_buffer().to_owned();
        let output = validate(&context, output(&context, vec![command.clone()])).unwrap();
        command.enqueue(); // Simulate a later producer violating ownership.
        let result = submit_custom_commands(&context, [&output]);
        command.commit();
        command.wait_until_completed();
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
            descriptor.set_width(width);
            descriptor.set_height(8);
            descriptor.set_pixel_format(format);
            descriptor.set_usage(MTLTextureUsage::ShaderRead);
            let output = MetalCustomOutput {
                texture: context.device.new_texture(&descriptor),
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
        let Some(other) = Device::all()
            .into_iter()
            .find(|device| device.registry_id() != context.device.registry_id())
        else {
            eprintln!("foreign-device GPU test unavailable: this machine has one Metal device");
            return;
        };
        let other_context = MetalCustomContext {
            queue: other.new_command_queue(),
            device: other,
        };
        let output = output(&other_context, vec![]);
        assert_eq!(
            validate(&context, output).err().unwrap().to_string(),
            "custom texture belongs to another device"
        );
        let command = other_context.queue.new_command_buffer().to_owned();
        assert!(validate_commands(&context, &[command]).is_err());
    });
}
