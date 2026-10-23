use super::*;

fn host() -> Result<Dx11CustomContext> {
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    Dx11CustomContext::new(device.unwrap(), context.unwrap())
}
fn extent() -> Size<DevicePixels> {
    size(DevicePixels(8), DevicePixels(8))
}
fn clear(context: &Dx11CustomContext, output: &Dx11WideSdrOutput, color: [f32; 4]) -> Result<()> {
    context.record_wide_sdr::<()>(output, |deferred, target| {
        unsafe {
            deferred.ClearRenderTargetView(target, &color);
        }
        Ok(())
    })
}
fn read(context: &Dx11CustomContext, texture: &ID3D11Texture2D) -> Result<Vec<[u16; 4]>> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
    }
    assert_eq!(desc.Format, DXGI_FORMAT_R16G16B16A16_FLOAT);
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    unsafe {
        context
            .device
            .CreateTexture2D(&desc, None, Some(&mut staging))?;
    }
    let staging = staging.unwrap();
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context.owner.CopyResource(&staging, texture);
        context
            .owner
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }
    let mut pixels = Vec::new();
    for y in 0..desc.Height as usize {
        // SAFETY: successful Map; read Width*8 bytes per row before Unmap.
        let row = unsafe {
            std::slice::from_raw_parts(
                (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                desc.Width as usize * 8,
            )
        };
        pixels.extend(
            row.as_chunks::<8>()
                .0
                .iter()
                .map(|p| std::array::from_fn(|i| u16::from_le_bytes([p[i * 2], p[i * 2 + 1]]))),
        );
    }
    unsafe {
        context.owner.Unmap(&staging, 0);
    }
    Ok(pixels)
}
#[test]
fn wide_output_format_extent_and_legacy_compatibility() -> Result<()> {
    let context = host()?;
    for extent in [
        size(DevicePixels(0), DevicePixels(8)),
        size(DevicePixels(-1), DevicePixels(8)),
        size(DevicePixels(16385), DevicePixels(1)),
        size(DevicePixels(4096), DevicePixels(2049)),
    ] {
        assert!(context.create_wide_sdr_output(extent).is_err());
    }
    let output = context.create_wide_sdr_output(extent())?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        output.texture().GetDesc(&mut desc);
    }
    assert_eq!(desc.Format, DXGI_FORMAT_R16G16B16A16_FLOAT);
    assert_eq!(desc.SampleDesc.Count, 1);
    assert_eq!(desc.MipLevels, 1);
    assert_eq!(desc.ArraySize, 1);
    assert!(
        output
            .clone()
            .validate(&context, size(DevicePixels(7), DevicePixels(8)))
            .is_err()
    );
    let mut wrong = output.clone();
    wrong.bundle = allocate_texture(&context.device, extent())?;
    assert!(wrong.validate(&context, extent()).is_err());
    let legacy = context.create_output(extent())?;
    unsafe {
        legacy.texture().GetDesc(&mut desc);
    }
    assert_eq!(desc.Format, DXGI_FORMAT_B8G8R8A8_UNORM);
    assert!(context.can_reuse(&legacy));
    assert!(context.can_reuse_wide_sdr(&output));
    clear(&context, &output, [0.5, 0., 0., 0.5])?;
    context.clear(&legacy, [0., 1., 0., 1.])?;
    context.submit()?;
    assert!(
        read(&context, output.texture())?
            .iter()
            .all(|p| *p == [0x3800, 0, 0, 0x3800])
    );
    assert!(output.validate(&context, extent()).is_ok());
    assert!(legacy.validate(&context, extent()).is_ok());
    Ok(())
}
#[test]
fn wide_output_pending_submitted_and_context_separation() -> Result<()> {
    let context = host()?;
    let output = context.create_wide_sdr_output(extent())?;
    let next = Dx11CustomContext::new(context.device.clone(), context.owner.clone())?;
    assert!(context.can_reuse_wide_sdr(&output));
    assert!(!next.can_reuse_wide_sdr(&output));
    assert!(
        next.record_wide_sdr::<()>(&output, |_, _| panic!("foreign writer called"))
            .is_err()
    );
    let foreign = host()?;
    assert!(!foreign.can_reuse_wide_sdr(&output));
    assert!(output.clone().validate(&foreign, extent()).is_err());
    let mut other_owner = None;
    unsafe {
        context
            .device
            .CreateDeferredContext(0, Some(&mut other_owner))?;
    }
    let separate = Dx11CustomContext::new(context.device.clone(), other_owner.unwrap())?;
    clear(&context, &output, [0., 0., 1., 1.])?;
    context.submit()?;
    assert!(next.can_reuse_wide_sdr(&output));
    assert!(!separate.can_reuse_wide_sdr(&output));
    assert!(
        next.record_wide_sdr::<()>(&output, |_, _| panic!("submitted writer called"))
            .is_err()
    );
    assert!(context.create_wide_sdr_output(extent()).is_err());
    assert!(
        context
            .record_wide_sdr::<()>(&output, |_, _| panic!("finished writer called"))
            .is_err()
    );
    assert_eq!(output.texture().as_raw(), output.clone().texture().as_raw());
    Ok(())
}
#[test]
fn wide_output_staged_reuse_submits_original_commands_once() -> Result<()> {
    let context = host()?;
    let mut batch = PreparationBatch::new(context.device.clone(), context.owner.clone());
    let output = batch.prepare(|ctx| {
        let output = ctx.create_wide_sdr_output(extent())?;
        clear(ctx, &output, [0., 1., 0., 1.])?;
        output.clone().validate(ctx, extent())?;
        Ok(output)
    })?;
    assert!(!context.can_reuse_wide_sdr(&output));
    let mut unrelated = PreparationBatch::new(context.device.clone(), context.owner.clone());
    assert!(
        unrelated
            .prepare(|ctx| output.clone().validate(ctx, extent()))
            .is_err()
    );
    for _ in 0..3 {
        batch.prepare(|ctx| {
            assert!(ctx.can_reuse_wide_sdr(&output));
            assert!(
                ctx.record_wide_sdr::<()>(&output, |_, _| panic!("staged writer called"))
                    .is_err()
            );
            output.clone().validate(ctx, extent())
        })?;
    }
    assert_eq!(
        batch
            .pending
            .iter()
            .filter(|(commands, _)| commands.is_some())
            .count(),
        1
    );
    batch.submit()?;
    assert!(context.can_reuse_wide_sdr(&output));
    assert!(
        read(&context, output.texture())?
            .iter()
            .all(|p| *p == [0, 0x3c00, 0, 0x3c00])
    );
    let mut later = PreparationBatch::new(context.device.clone(), context.owner.clone());
    later.prepare(|ctx| output.clone().validate(ctx, extent()))?;
    drop(later);
    assert!(context.can_reuse_wide_sdr(&output));
    Ok(())
}
#[test]
fn wide_output_dropped_batch_abandons_staged_outputs_and_discards_work() -> Result<()> {
    let host = host()?;
    let witness = host.create_wide_sdr_output(extent())?;
    clear(&host, &witness, [1., 0., 0., 1.])?;
    host.submit()?;
    let mut batch = PreparationBatch::new(host.device.clone(), host.owner.clone());
    let staged = batch.prepare(|ctx| {
        let output = ctx.create_wide_sdr_output(extent())?;
        ctx.record_wide_sdr::<()>(&output, |deferred, target| {
            unsafe {
                deferred.ClearRenderTargetView(target, &[0., 1., 0., 1.]);
                deferred.CopyResource(witness.texture(), output.texture());
            }
            Ok(())
        })?;
        Ok(output)
    })?;
    let failed: Result<()> = batch.prepare(|_| anyhow::bail!("later validation failed"));
    assert!(failed.is_err());
    drop(batch);
    assert!(!host.can_reuse_wide_sdr(&staged));
    assert!(staged.validate(&host, extent()).is_err());
    assert!(host.can_reuse_wide_sdr(&witness));
    assert!(
        read(&host, witness.texture())?
            .iter()
            .all(|p| *p == [0x3c00, 0, 0, 0x3c00])
    );
    Ok(())
}
#[test]
fn wide_output_failed_recording_and_validation_abandon_escaped_outputs() -> Result<()> {
    let host = host()?;
    let witness = host.create_wide_sdr_output(extent())?;
    clear(&host, &witness, [1., 0., 0., 1.])?;
    host.submit()?;
    for fail_encoding in [false, true] {
        let mut escaped = None;
        let mut batch = PreparationBatch::new(host.device.clone(), host.owner.clone());
        let failed: Result<()> = batch.prepare(|ctx| {
            let output = ctx.create_wide_sdr_output(extent())?;
            escaped = Some(output.clone());
            ctx.record_wide_sdr::<()>(&output, |deferred, target| {
                unsafe {
                    deferred.ClearRenderTargetView(target, &[0., 1., 0., 1.]);
                    deferred.CopyResource(witness.texture(), output.texture());
                }
                if fail_encoding {
                    anyhow::bail!("encode failed");
                }
                Ok(())
            })?;
            output.validate(ctx, size(DevicePixels(9), DevicePixels(8)))?;
            Ok(())
        });
        assert!(failed.is_err());
        let healthy = batch.prepare(|ctx| {
            let output = ctx.create_wide_sdr_output(extent())?;
            clear(ctx, &output, [0., 0., 1., 1.])?;
            Ok(output)
        })?;
        batch.submit()?;
        assert!(host.can_reuse_wide_sdr(&healthy));
        assert!(!host.can_reuse_wide_sdr(escaped.as_ref().unwrap()));
        assert!(
            read(&host, witness.texture())?
                .iter()
                .all(|p| *p == [0x3c00, 0, 0, 0x3c00])
        );
    }
    Ok(())
}
