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
