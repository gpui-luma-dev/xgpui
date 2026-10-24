use super::*;
fn texture(context: &MetalCustomContext) -> Texture {
    let d = TextureDescriptor::new();
    d.set_width(8);
    d.set_height(8);
    d.set_pixel_format(MTLPixelFormat::RGBA16Float);
    d.set_usage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
    context.device.new_texture(&d)
}
fn context() -> MetalCustomContext {
    let device = Device::system_default().unwrap();
    let queue = device.new_command_queue();
    MetalCustomContext { device, queue }
}
#[test]
fn wide_output_pending_reuse_submission_and_queue_separation() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let c = context();
        let command = c.queue.new_command_buffer().to_owned();
        let cached = MetalWideSdrOutput::from_linear_p3_premultiplied(
            &c,
            texture(&c),
            vec![command.clone()],
        )?;
        let first = cached.reusable_for(&c)?;
        let second = cached.reusable_for(&c)?;
        assert_eq!(
            first.output.texture.as_ptr(),
            second.output.texture.as_ptr()
        );
        assert_eq!(
            first.output.commands[0].as_ptr(),
            second.output.commands[0].as_ptr()
        );
        let other = MetalCustomContext {
            device: c.device.clone(),
            queue: c.device.new_command_queue(),
        };
        assert!(cached.reusable_for(&other).is_err());
        assert_eq!(command.status(), MTLCommandBufferStatus::NotEnqueued);
        submit_custom_commands(&c, [&first.output, &second.output])?;
        let ready = cached.reusable_for(&c)?;
        assert!(ready.output.commands.is_empty());
        command.wait_until_completed();
        assert_ne!(command.status(), MTLCommandBufferStatus::Error);
        assert!(cached.reusable_for(&c)?.output.commands.is_empty());
        Ok(())
    })
}
#[test]
fn wide_output_rejects_external_enqueue_and_wrong_queue_commands() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let c = context();
        let other = c.device.new_command_queue();
        let wrong = other.new_command_buffer().to_owned();
        assert!(
            MetalWideSdrOutput::from_linear_p3_premultiplied(&c, texture(&c), vec![wrong.clone()])
                .is_err()
        );
        assert_eq!(wrong.status(), MTLCommandBufferStatus::NotEnqueued);
        let command = c.queue.new_command_buffer().to_owned();
        let cached = MetalWideSdrOutput::from_linear_p3_premultiplied(
            &c,
            texture(&c),
            vec![command.clone()],
        )?;
        command.enqueue();
        let reuse = cached.reusable_for(&c);
        let raw = cached.validate_context(&c);
        // Release the reserved queue slot even if an assertion below fails.
        command.commit();
        command.wait_until_completed();
        assert!(reuse.is_err());
        assert!(raw.is_err());
        Ok(())
    })
}

#[test]
fn wide_output_command_state_rejects_errors_without_running_faulty_gpu_work() {
    assert!(wide_command_pending(MTLCommandBufferStatus::NotEnqueued).unwrap());
    for status in [
        MTLCommandBufferStatus::Committed,
        MTLCommandBufferStatus::Scheduled,
        MTLCommandBufferStatus::Completed,
    ] {
        assert!(!wide_command_pending(status).unwrap());
    }
    assert!(wide_command_pending(MTLCommandBufferStatus::Enqueued).is_err());
    assert!(wide_command_pending(MTLCommandBufferStatus::Error).is_err());
}
