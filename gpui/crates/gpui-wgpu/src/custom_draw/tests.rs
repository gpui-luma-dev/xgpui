use super::*;

#[test]
fn wgpu_custom_recording_readiness_and_extent() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    eprintln!(
        "custom paint test backend: {:?}",
        host.adapter.get_info().backend
    );
    let extent = size(DevicePixels(8), DevicePixels(4));
    let first = WgpuCustomContext::new(host.device.clone(), &host.queue);
    let output = first.create_output(extent)?;
    assert!(first.can_reuse(&output));
    let second = WgpuCustomContext::new(host.device.clone(), &host.queue);
    assert!(!second.can_reuse(&output)); // Unsubmitted work in another recording.
    first.submit()?;
    assert!(second.can_reuse(&output)); // Same-queue ordering; no CPU/GPU wait.
    assert!(output.clone().validate(&second, extent).is_ok());
    assert!(
        output
            .clone()
            .validate(&second, size(DevicePixels(9), DevicePixels(4)))
            .is_err()
    );
    assert!(first.record(|_| Ok(())).is_err());
    assert!(first.submit().is_err());
    Ok(())
}

#[test]
fn wgpu_custom_abandoned_recording_is_not_reusable() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    let output = {
        let context = WgpuCustomContext::new(host.device.clone(), &host.queue);
        context.create_output(size(DevicePixels(8), DevicePixels(4)))?
    };
    let next = WgpuCustomContext::new(host.device.clone(), &host.queue);
    assert!(!next.can_reuse(&output));
    assert!(
        output
            .validate(&next, size(DevicePixels(8), DevicePixels(4)))
            .is_err()
    );
    assert!(
        next.create_output(size(DevicePixels(0), DevicePixels(4)))
            .is_err()
    );
    Ok(())
}

#[test]
fn wgpu_custom_foreign_queue_output_is_rejected() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    let first = WgpuCustomContext::new(host.device.clone(), &host.queue);
    let extent = size(DevicePixels(8), DevicePixels(4));
    let output = first.create_output(extent)?;
    first.submit()?;
    let mut other = WgpuCustomContext::new(host.device.clone(), &host.queue);
    // An unavailable second queue/device is unnecessary to test the ownership
    // token: only host-constructed outputs can carry it, and fields are private.
    other.key = other.key.wrapping_add(1);
    assert!(!other.can_reuse(&output));
    assert!(output.validate(&other, extent).is_err());
    Ok(())
}

fn test_buffer(host: &crate::WgpuContext, byte: u8) -> wgpu::Buffer {
    host.device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("transaction_witness"),
            contents: &[byte; 16],
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        })
}
fn read_buffer(host: &crate::WgpuContext, buffer: &wgpu::Buffer) -> Result<Vec<u8>> {
    let staging = host.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("transaction_readback"),
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = host.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, 16);
    let submission = host.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    host.device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(std::time::Duration::from_secs(30)),
    })?;
    receiver.recv_timeout(std::time::Duration::from_secs(30))??;
    let bytes = staging.slice(..).get_mapped_range()?.to_vec();
    staging.unmap();
    Ok(bytes)
}

#[test]
fn wgpu_custom_failed_transactions_discard_encoded_work() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    let extent = size(DevicePixels(8), DevicePixels(4));
    // Errors after encoding, invalid output, callback errors, and swallowed
    // callback errors must all discard the commands, not merely their output.
    for mode in 0..4 {
        let witness = test_buffer(&host, 17);
        let healthy = test_buffer(&host, 23);
        let mut batch = PreparationBatch::new(host.device.clone(), &host.queue);
        let mut rejected_output = None;
        let result: Result<()> = batch.prepare(|context| {
            let output = context.create_output(extent)?;
            rejected_output = Some(output.clone());
            context.record(|encoder| {
                encoder.clear_buffer(&witness, 0, None);
                Ok(())
            })?;
            if mode < 2 {
                if mode == 0 {
                    anyhow::bail!("producer rejected after encoding");
                }
                output.validate(context, size(DevicePixels(9), DevicePixels(4)))?;
            } else {
                let encoded: Result<()> = context.record(|encoder| {
                    encoder.clear_buffer(&witness, 0, None);
                    anyhow::bail!("encoding rejected");
                });
                if mode == 2 {
                    encoded?;
                } else {
                    assert!(encoded.is_err());
                    assert!(context.record(|_| Ok(())).is_err());
                    assert!(context.create_output(extent).is_err());
                }
            }
            Ok(())
        });
        assert!(result.is_err(), "failure mode {mode} was accepted");
        let healthy_output = batch.prepare(|context| {
            assert!(!context.can_reuse(rejected_output.as_ref().unwrap()));
            context.record(|encoder| {
                encoder.clear_buffer(&healthy, 0, None);
                Ok(())
            })?;
            context.create_output(extent)?.validate(context, extent)
        })?;
        batch.submit()?;
        let next = WgpuCustomContext::new(host.device.clone(), &host.queue);
        assert!(!next.can_reuse(rejected_output.as_ref().unwrap()));
        assert!(next.can_reuse(&healthy_output));
        assert_eq!(
            read_buffer(&host, &witness)?,
            vec![17; 16],
            "failed commands ran in mode {mode}"
        );
        assert_eq!(read_buffer(&host, &healthy)?, vec![0; 16]);
    }
    Ok(())
}

#[test]
fn wgpu_custom_staged_reuse_and_submission_order() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    let extent = size(DevicePixels(8), DevicePixels(4));
    let source = test_buffer(&host, 17);
    let destination = test_buffer(&host, 23);
    let outsider = WgpuCustomContext::new(host.device.clone(), &host.queue);
    let mut batch = PreparationBatch::new(host.device.clone(), &host.queue);
    let output = batch.prepare(|context| {
        context.record(|encoder| {
            encoder.clear_buffer(&source, 0, None);
            Ok(())
        })?;
        context.create_output(extent)?.validate(context, extent)
    })?;
    assert!(!outsider.can_reuse(&output));
    let rejected: Result<()> = batch.prepare(|context| {
        assert!(context.can_reuse(&output));
        context.record(|encoder| {
            encoder.copy_buffer_to_buffer(&destination, 0, &source, 0, 16);
            Ok(())
        })?;
        anyhow::bail!("failure after a staged cache hit");
    });
    assert!(rejected.is_err());
    batch.prepare(|context| {
        assert!(context.can_reuse(&output));
        output.clone().validate(context, extent)?;
        context.record(|encoder| {
            encoder.copy_buffer_to_buffer(&source, 0, &destination, 0, 16);
            Ok(())
        })
    })?;
    // Cache-only preparation requires no encoding and still retains readiness.
    batch.prepare(|context| output.clone().validate(context, extent))?;
    batch.submit()?;
    assert!(outsider.can_reuse(&output));
    assert_eq!(read_buffer(&host, &destination)?, vec![0; 16]);
    // Aborting a later batch must not invalidate an already-submitted cache hit.
    let mut later = PreparationBatch::new(host.device.clone(), &host.queue);
    later.prepare(|context| output.clone().validate(context, extent))?;
    drop(later);
    assert!(outsider.can_reuse(&output));
    Ok(())
}

#[test]
fn wgpu_custom_dropped_batch_abandons_all_staged_work() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    let extent = size(DevicePixels(8), DevicePixels(4));
    let witness = test_buffer(&host, 17);
    let mut batch = PreparationBatch::new(host.device.clone(), &host.queue);
    let first = batch.prepare(|context| {
        context.record(|encoder| {
            encoder.clear_buffer(&witness, 0, None);
            Ok(())
        })?;
        context.create_output(extent)?.validate(context, extent)
    })?;
    let second =
        batch.prepare(|context| context.create_output(extent)?.validate(context, extent))?;
    let aborted: Result<()> = batch.prepare(|_| anyhow::bail!("later preparation failed"));
    assert!(aborted.is_err());
    drop(batch);
    let mut next = PreparationBatch::new(host.device.clone(), &host.queue);
    next.prepare(|context| {
        assert!(!context.can_reuse(&first));
        assert!(!context.can_reuse(&second));
        assert!(first.clone().validate(context, extent).is_err());
        assert!(second.clone().validate(context, extent).is_err());
        Ok(())
    })?;
    next.submit()?;
    assert_eq!(read_buffer(&host, &witness)?, vec![17; 16]);
    Ok(())
}

#[derive(Debug)]
struct UniformTestProducer;
impl gpui::CustomContent for UniformTestProducer {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: gpui::Size<gpui::DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        unreachable!()
    }
}
fn uniform_draw(index: usize, count: usize, phase: usize) -> gpui::CustomDraw {
    let x = if phase.is_multiple_of(2) {
        index
    } else {
        count - 1 - index
    } as f32
        * 2.;
    let bounds = gpui::Bounds::new(
        gpui::point(gpui::ScaledPixels(x), gpui::ScaledPixels(0.)),
        size(gpui::ScaledPixels(2.), gpui::ScaledPixels(8.)),
    );
    gpui::CustomDraw {
        order: 0,
        bounds,
        content_mask: gpui::ContentMask {
            bounds: gpui::Bounds::new(
                bounds.origin,
                size(
                    gpui::ScaledPixels(2.),
                    gpui::ScaledPixels((4 + index % 3) as f32),
                ),
            ),
        },
        corner_radii: gpui::Corners::default(),
        opacity: ((index + phase) % 4 + 1) as f32 / 4.,
        content: Arc::new(UniformTestProducer),
        coverage: None,
    }
}
#[test]
fn wgpu_custom_uniform_arena_preserves_draws_growth_and_queued_frames() -> Result<()> {
    let (host, format) = crate::WgpuContext::new_headless()?;
    let device = &host.device;
    let mut compositor = CustomCompositor::new(device, format);
    compositor.begin_uniforms(device, 0)?;
    assert_eq!(compositor.uniforms.creations, 0);
    let source = new_texture(device, size(DevicePixels(8), DevicePixels(8)), format)?;
    let view = source.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.,
                        g: 0.,
                        b: 0.,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    host.queue.submit([encoder.finish()]);
    let mut frames = Vec::new();
    for (phase, count) in [2, 7, 60, 300, 3, 300].into_iter().enumerate() {
        if phase == 3 {
            let capacity = compositor.uniforms.capacity;
            assert!(compositor.begin_uniforms(device, usize::MAX).is_err());
            assert_eq!(compositor.uniforms.capacity, capacity);
        }
        let before = compositor.uniforms.creations;
        compositor.begin_uniforms(device, count)?;
        assert!(
            compositor
                .uniforms
                .stride
                .is_multiple_of(device.limits().min_uniform_buffer_offset_alignment as u64)
        );
        if phase >= 4 {
            assert_eq!(compositor.uniforms.creations, before);
        }
        let draws: Vec<_> = (0..count).map(|i| uniform_draw(i, count, phase)).collect();
        let prepared: Vec<_> = draws
            .iter()
            .map(|draw| {
                compositor.prepare(
                    device,
                    draw,
                    &source,
                    None,
                    false,
                    size(DevicePixels(128), DevicePixels(8)),
                )
            })
            .collect();
        compositor.upload_uniforms(&host.queue);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("uniform_frame_target"),
            size: wgpu::Extent3d {
                width: 128,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform_frame_readback"),
            size: 128 * 8 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&compositor.pipeline);
            for draw in &prepared {
                pass.set_bind_group(0, &draw.bind_group, &[draw.uniform_offset]);
                pass.draw(0..6, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(128 * 4),
                    rows_per_image: Some(8),
                },
            },
            wgpu::Extent3d {
                width: 128,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        let submission = host.queue.submit([encoder.finish()]);
        // Queue the next frame's upload without a CPU completion wait. Keep each
        // frame's snapshot for later readback, including frames before growth.
        frames.push((readback, submission, phase, count));
    }
    assert_eq!(
        compositor.uniforms.creations, 4,
        "only capacity growth creates uniform buffers"
    );
    for (buffer, submission, phase, count) in frames {
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(30)),
        })?;
        rx.recv_timeout(std::time::Duration::from_secs(30))??;
        {
            let bytes = buffer.slice(..).get_mapped_range()?;
            for y in 0..8 {
                for x in 0..128 {
                    let slot = x / 2;
                    let index = if phase.is_multiple_of(2) {
                        slot
                    } else {
                        count.saturating_sub(1 + slot)
                    };
                    let expected = if slot < count && y < 4 + index % 3 {
                        (((index + phase) % 4 + 1) as f32 / 4. * 255.).round() as u8
                    } else {
                        0
                    };
                    let at = (y * 128 + x) * 4;
                    let red = if format == wgpu::TextureFormat::Bgra8Unorm {
                        bytes[at + 2]
                    } else {
                        bytes[at]
                    };
                    assert!(
                        red.abs_diff(expected) <= 1,
                        "frame {phase}, ({x},{y}): {red} != {expected}"
                    );
                    assert!(bytes[at + 3].abs_diff(expected) <= 1);
                }
            }
        }
        buffer.unmap();
    }
    Ok(())
}

#[test]
fn wgpu_source_image_readiness_and_size_contract() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    let data = gpui::ImageTextureData::new(2, 2, vec![vec![255; 16], vec![255; 4]])?;
    let first = WgpuCustomContext::new(host.device.clone(), &host.queue);
    let output = first.create_image_output(&data)?;
    assert_eq!(output.texture().mip_level_count(), 2);
    let second = WgpuCustomContext::new(host.device.clone(), &host.queue);
    assert!(!second.can_reuse(&output));
    first.submit()?;
    assert!(
        output
            .validate(&second, size(DevicePixels(8), DevicePixels(4)))
            .is_ok()
    );
    let abandoned = {
        let context = WgpuCustomContext::new(host.device.clone(), &host.queue);
        context.create_image_output(&data)?
    };
    assert!(!second.can_reuse(&abandoned));
    Ok(())
}

#[test]
fn wgpu_worker_staging_can_be_readopted_after_abandoned_batch() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    let context = WgpuCustomContext::new(host.device.clone(), &host.queue);
    let worker = context.image_upload_context();
    let data = gpui::ImageTextureData::new(2, 2, vec![vec![255; 16], vec![255; 4]])?;
    let staged = std::thread::spawn(move || worker.stage(&data))
        .join()
        .unwrap()?;
    let abandoned = {
        let context = WgpuCustomContext::new(host.device.clone(), &host.queue);
        context.adopt_image_upload(&staged)?
    };
    assert!(!context.can_reuse(&abandoned));
    let adopted = context.adopt_image_upload(&staged)?;
    assert!(!adopted.is_submitted());
    context.submit()?;
    assert!(adopted.is_submitted());
    let other = WgpuCustomContext::new(host.device.clone(), &host.queue);
    assert!(other.can_reuse(&adopted));
    // A staged source is queue-bound, including before its first adoption.
    let mut foreign = staged;
    foreign.queue = Arc::new((*host.queue).clone());
    assert!(other.adopt_image_upload(&foreign).is_err());
    Ok(())
}

#[test]
fn wgpu_staging_pool_cannot_recycle_before_upload_completion() -> Result<()> {
    let (host, _) = crate::WgpuContext::new_headless()?;
    let context = WgpuCustomContext::new(host.device.clone(), &host.queue);
    let worker = context.image_upload_context();
    let pool = worker.staging_pool(768);
    let data = gpui::ImageTextureData::new(2, 2, vec![vec![255; 16], vec![255; 4]])?;
    let staged = worker.stage_with_pool(&data, &pool)?;
    let output = context.adopt_image_upload(&staged)?;
    drop(staged);
    assert_eq!(pool.stats().cached_bytes, 0);
    assert!(worker.stage_with_pool(&data, &pool).is_err());
    context.submit()?;
    host.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(10)),
    })?;
    assert!(output.is_submitted());
    // Another test thread may be delivering completion callbacks for the shared
    // device. GPU completion alone does not prove that thread has returned this
    // lease yet; wait for the observable pool return before asserting reuse.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while pool.stats().cached_bytes == 0 && std::time::Instant::now() < deadline {
        host.device.poll(wgpu::PollType::Poll)?;
        std::thread::yield_now();
    }
    assert_eq!(pool.stats().cached_bytes, 768);
    let staged = worker.stage_with_pool(&data, &pool)?;
    assert_eq!(pool.stats().allocations, 1);
    assert_eq!(pool.stats().reuses, 1);
    drop(staged);
    Ok(())
}
