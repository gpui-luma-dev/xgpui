use super::*;
use crate::{Gradient, GradientStop, MeshGradient, MeshGrid, SolidColor};
use gpui::{CustomContent, size};
use metal::*;
use std::sync::Arc;

fn context() -> MetalCustomContext {
    let device = Device::system_default().expect("Metal device required");
    MetalCustomContext {
        queue: device.new_command_queue(),
        device,
    }
}
fn extent(w: i32, h: i32) -> Size<DevicePixels> {
    size(DevicePixels(w), DevicePixels(h))
}
fn generate(
    ctx: &MetalCustomContext,
    extent: Size<DevicePixels>,
) -> Result<(Texture, CommandBuffer)> {
    let desc = TextureDescriptor::new();
    desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    desc.set_width(extent.width.0 as u64);
    desc.set_height(extent.height.0 as u64);
    desc.set_storage_mode(MTLStorageMode::Shared);
    desc.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    let texture = ctx.device.new_texture(&desc);
    let command = ctx.queue.new_command_buffer().to_owned();
    let pass = RenderPassDescriptor::new();
    let attachment = pass.color_attachments().object_at(0).unwrap();
    attachment.set_texture(Some(&texture));
    attachment.set_load_action(MTLLoadAction::Clear);
    attachment.set_store_action(MTLStoreAction::Store);
    attachment.set_clear_color(MTLClearColor::new(1., 0., 0., 1.));
    command.new_render_command_encoder(pass).end_encoding();
    Ok((texture, command))
}
fn prepare(
    cache: &OutputCache,
    ctx: &MetalCustomContext,
    size: Size<DevicePixels>,
) -> (MetalCustomOutput, bool) {
    cache.prepare(ctx, size, || generate(ctx, size)).unwrap()
}
fn submit(output: &MetalCustomOutput) {
    for command in &output.commands {
        command.commit();
        command.wait_until_completed();
        assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
    }
}
fn red_pixel(texture: &TextureRef) {
    let mut pixel = [0u8; 4];
    texture.get_bytes(
        pixel.as_mut_ptr().cast(),
        4,
        MTLRegion::new_2d(0, 0, 1, 1),
        0,
    );
    assert_eq!(pixel, [0, 0, 255, 255]);
}

#[test]
fn pending_command_is_shared_until_host_submission() {
    let ctx = context();
    let cache = OutputCache::default();
    let (first, generated) = prepare(&cache, &ctx, extent(4, 4));
    assert!(generated);
    let (pending, generated) = cache
        .prepare(&ctx, extent(4, 4), || panic!("cache miss"))
        .unwrap();
    assert!(!generated);
    assert_eq!(pending.commands.len(), 1);
    assert_eq!(
        pending.commands[0].status(),
        MTLCommandBufferStatus::NotEnqueued
    );
    assert!(std::ptr::eq::<CommandBufferRef>(
        &*first.commands[0],
        &*pending.commands[0]
    ));
    assert!(std::ptr::eq::<TextureRef>(
        &*first.texture,
        &*pending.texture
    ));
    // Host deduplicates shared pending commands and submits once.
    submit(&first);
    let (ready, generated) = prepare(&cache, &ctx, extent(4, 4));
    assert!(!generated);
    assert!(ready.commands.is_empty());
    red_pixel(&ready.texture);
}

#[test]
fn eviction_removes_only_lru_entry_and_retained_pending_output_survives() {
    let ctx = context();
    let cache = OutputCache {
        budget: 128,
        ..Default::default()
    };
    let (_, _) = prepare(&cache, &ctx, extent(4, 4));
    let (evicted, _) = prepare(&cache, &ctx, extent(8, 2));
    assert!(!prepare(&cache, &ctx, extent(4, 4)).1); // A is most recent.
    assert!(prepare(&cache, &ctx, extent(2, 8)).1); // Evict only B.
    assert_eq!(cache.texture_info().unwrap().1, 128);
    assert_eq!(cache.state.lock().unwrap().entries.len(), 2);
    assert!(!prepare(&cache, &ctx, extent(4, 4)).1); // A survives, not clear-all.
    assert!(prepare(&cache, &ctx, extent(8, 2)).1); // B must regenerate.
    // Evicting cache ownership cannot cancel a previously returned command/texture.
    submit(&evicted);
    red_pixel(&evicted.texture);
}

#[test]
fn cache_miss_failure_preserves_entries_and_oversized_output_bypasses_cache() {
    let ctx = context();
    let cache = OutputCache {
        budget: 64,
        ..Default::default()
    };
    prepare(&cache, &ctx, extent(4, 4));
    assert!(
        cache
            .prepare(&ctx, extent(8, 2), || anyhow::bail!("encode failed"))
            .is_err()
    );
    assert!(!prepare(&cache, &ctx, extent(4, 4)).1);
    assert!(prepare(&cache, &ctx, extent(8, 8)).1);
    assert!(!prepare(&cache, &ctx, extent(4, 4)).1);
    assert_eq!(cache.texture_info().unwrap().1, 64);
    for size in [
        extent(0, 4),
        extent(-1, 4),
        extent(16385, 1),
        extent(8192, 8192),
    ] {
        assert!(
            cache
                .prepare(&ctx, size, || panic!("invalid allocation"))
                .is_err()
        );
    }
}

struct Probe {
    name: &'static str,
    producer: Arc<dyn CustomContent>,
    generations: Box<dyn Fn() -> usize>,
}
fn producers() -> Vec<Probe> {
    let solid = Arc::new(SolidColor::new([1., 0., 0., 1.]).unwrap());
    let gradient = Arc::new(
        Gradient::linear(
            45.,
            vec![
                GradientStop::new(0., [1., 0., 0., 1.]),
                GradientStop::new(1., [0., 0., 1., 1.]),
            ],
        )
        .unwrap(),
    );
    let mesh = Arc::new(
        MeshGradient::new(MeshGrid::regular(2, 2, vec![[1., 0., 0.]; 4]).unwrap(), 18).unwrap(),
    );
    vec![
        Probe {
            name: "solid",
            producer: solid.clone(),
            generations: Box::new(move || solid.preparations()),
        },
        Probe {
            name: "gradient",
            producer: gradient.clone(),
            generations: Box::new(move || gradient.preparations()),
        },
        Probe {
            name: "mesh",
            producer: mesh.clone(),
            generations: Box::new(move || mesh.preparations()),
        },
    ]
}
fn output(
    probe: &Probe,
    ctx: &MetalCustomContext,
    extent: Size<DevicePixels>,
) -> MetalCustomOutput {
    *probe
        .producer
        .prepare(ctx, extent)
        .unwrap()
        .downcast::<MetalCustomOutput>()
        .unwrap()
}

#[test]
fn all_producers_reuse_alternating_sizes_and_preserve_pending_commands() {
    let ctx = context();
    for probe in producers() {
        let a = output(&probe, &ctx, extent(320, 240));
        let b = output(&probe, &ctx, extent(80, 20));
        let pending = output(&probe, &ctx, extent(320, 240));
        assert_eq!((probe.generations)(), 2, "{}", probe.name);
        assert!(std::ptr::eq::<CommandBufferRef>(
            &*a.commands[0],
            &*pending.commands[0]
        ));
        submit(&a);
        submit(&b);
        for _ in 0..4 {
            assert!(output(&probe, &ctx, extent(320, 240)).commands.is_empty());
            assert!(output(&probe, &ctx, extent(80, 20)).commands.is_empty());
        }
        assert_eq!((probe.generations)(), 2, "{}", probe.name);
    }
}

#[test]
fn all_producers_separate_queues_on_the_same_device() {
    let a = context();
    let b = MetalCustomContext {
        device: a.device.clone(),
        queue: a.device.new_command_queue(),
    };
    assert_ne!(a.cache_key(), b.cache_key());
    for probe in producers() {
        let first = output(&probe, &a, extent(16, 16));
        let second = output(&probe, &b, extent(16, 16));
        assert!(!std::ptr::eq::<TextureRef>(
            &*first.texture,
            &*second.texture
        ));
        assert!(!std::ptr::eq::<CommandBufferRef>(
            &*first.commands[0],
            &*second.commands[0]
        ));
        let again = output(&probe, &a, extent(16, 16));
        assert!(std::ptr::eq::<CommandBufferRef>(
            &*first.commands[0],
            &*again.commands[0]
        ));
        submit(&first);
        // Completing queue A cannot mark queue B's output submitted.
        assert_eq!(output(&probe, &b, extent(16, 16)).commands.len(), 1);
        assert!(output(&probe, &a, extent(16, 16)).commands.is_empty());
        submit(&second);
        assert!(output(&probe, &b, extent(16, 16)).commands.is_empty());
        assert_eq!((probe.generations)(), 2, "{}", probe.name);
    }
}
