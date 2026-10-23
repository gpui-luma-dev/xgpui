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
