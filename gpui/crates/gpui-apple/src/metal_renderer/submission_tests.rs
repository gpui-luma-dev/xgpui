use super::*;
use crate::custom_draw::{MetalCustomContext, MetalCustomOutput};
use gpui::{CustomContent, CustomDraw};
use metal::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct LegacyCachedProducer {
    texture: Texture,
    command: CommandBuffer,
    queue: CommandQueue,
    calls: AtomicUsize,
}

// No request-aware method: this exercises the compatibility adapter as well.
impl CustomContent for LegacyCachedProducer {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        let context = backend.downcast_ref::<MetalCustomContext>().unwrap();
        assert_eq!(context.queue.as_ptr(), self.queue.as_ptr());
        self.calls.fetch_add(1, Ordering::Relaxed);
        let commands = if self.command.status() == MTLCommandBufferStatus::NotEnqueued {
            vec![self.command.clone()]
        } else {
            vec![]
        };
        Ok(Box::new(MetalCustomOutput {
            texture: self.texture.clone(),
            commands,
        }))
    }
}

// Ensure a failed assertion cannot leave the GPU waiting on an unsignaled event.
struct ReleaseEvent(SharedEvent);
impl Drop for ReleaseEvent {
    fn drop(&mut self) {
        self.0.set_signaled_value(1);
    }
}

#[test]
fn submission_renderer_reuses_pending_cached_output_without_cpu_wait() {
    objc2::rc::autoreleasepool(|_| {
        let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
        let extent = size(DevicePixels(8), DevicePixels(8));
        let texture = crate::custom_draw::texture(&renderer.device, extent).unwrap();
        let event = renderer.device.new_shared_event();
        let release = ReleaseEvent(event.clone());
        let command = renderer.command_queue.new_command_buffer().to_owned();
        command.encode_wait_for_event(&event, 1);
        let pass = RenderPassDescriptor::new();
        let attachment = pass.color_attachments().object_at(0).unwrap();
        attachment.set_texture(Some(&texture));
        attachment.set_load_action(MTLLoadAction::Clear);
        attachment.set_store_action(MTLStoreAction::Store);
        attachment.set_clear_color(MTLClearColor::new(1., 0., 0., 1.));
        command.new_render_command_encoder(pass).end_encoding();
        let producer = Arc::new(LegacyCachedProducer {
            texture,
            command: command.clone(),
            queue: renderer.command_queue.clone(),
            calls: AtomicUsize::new(0),
        });
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(8.), ScaledPixels(8.)),
        );
        let mut scene = Scene::default();
        for _ in 0..2 {
            scene.insert_primitive(CustomDraw {
                order: 0,
                bounds,
                content_mask: ContentMask { bounds },
                corner_radii: Default::default(),
                opacity: 1.,
                content: producer.clone(),
                coverage: None,
            });
        }
        scene.finish();
        // First frame shares one pending buffer across both draws. The second
        // frame returns empty command lists while the GPU is deliberately held.
        renderer.render_scene(&scene, extent).unwrap();
        renderer.render_scene(&scene, extent).unwrap();
        assert_eq!(producer.calls.load(Ordering::Relaxed), 4);
        assert_ne!(command.status(), MTLCommandBufferStatus::NotEnqueued);
        assert_ne!(command.status(), MTLCommandBufferStatus::Completed);
        drop(release);
        let image = renderer.render_scene_to_image(&scene, extent).unwrap();
        assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
        for pixel in image.pixels() {
            assert_eq!(pixel.0, [255, 0, 0, 255]);
        }
    });
}
