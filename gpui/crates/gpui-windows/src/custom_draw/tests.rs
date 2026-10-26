use super::*;
use gpui::{Bounds, ContentMask, Corners, CustomContent, ScaledPixels, point};
use std::any::Any;

fn warp() -> Result<Dx11CustomContext> {
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
fn read(ctx: &Dx11CustomContext, texture: &ID3D11Texture2D) -> Result<Vec<[u8; 4]>> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
    }
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    unsafe {
        ctx.device
            .CreateTexture2D(&desc, None, Some(&mut staging))?;
    }
    let staging = staging.unwrap();
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        ctx.owner.CopyResource(&staging, texture);
        ctx.owner
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }
    let mut pixels = Vec::new();
    for y in 0..desc.Height as usize {
        // SAFETY: Map succeeded, each row contains Width BGRA pixels; read before Unmap.
        let row = unsafe {
            std::slice::from_raw_parts(
                (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                desc.Width as usize * 4,
            )
        };
        pixels.extend(row.chunks_exact(4).map(|p| [p[2], p[1], p[0], p[3]]));
    }
    unsafe {
        ctx.owner.Unmap(&staging, 0);
    }
    Ok(pixels)
}
#[test]
fn dx11_custom_submission_and_output_validation() -> Result<()> {
    let ctx = warp()?;
    let extent = size(DevicePixels(8), DevicePixels(8));
    assert!(
        ctx.create_output(size(DevicePixels(0), DevicePixels(8)))
            .is_err()
    );
    assert!(
        ctx.create_output(size(DevicePixels(16384), DevicePixels(16384)))
            .is_err()
    );
    let output = ctx.create_output(extent)?;
    ctx.clear(&output, [1., 0., 0., 1.])?;
    assert!(ctx.can_reuse(&output));
    let next = Dx11CustomContext::new(ctx.device.clone(), ctx.owner.clone())?;
    assert!(!next.can_reuse(&output));
    assert!(
        output
            .clone()
            .validate(&ctx, size(DevicePixels(9), DevicePixels(8)))
            .is_err()
    );
    let foreign = warp()?;
    assert!(output.clone().validate(&foreign, extent).is_err());
    ctx.submit()?;
    assert!(next.can_reuse(&output));
    assert!(ctx.submit().is_err());
    assert!(ctx.clear(&output, [0.; 4]).is_err());
    assert!(
        read(&ctx, output.texture())?
            .iter()
            .all(|p| *p == [255, 0, 0, 255])
    );
    let abandoned = next.create_output(extent)?;
    drop(next);
    assert!(!ctx.can_reuse(&abandoned));
    assert!(abandoned.validate(&ctx, extent).is_err());
    Ok(())
}
#[derive(Debug)]
struct Unused;
impl CustomContent for Unused {
    fn prepare(&self, _: &dyn Any, _: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        unreachable!()
    }
}
#[test]
fn dx11_custom_composition_preserves_alpha_clip_and_immediate_state() -> Result<()> {
    let ctx = warp()?;
    let extent = size(DevicePixels(8), DevicePixels(8));
    let source = ctx.create_output(extent)?;
    ctx.clear(&source, [1., 0., 0., 0.5])?;
    ctx.submit()?;
    let target = allocate_texture(&ctx.device, extent)?;
    let sentinel = dynamic_uniform_buffer(&ctx.device)?;
    unsafe {
        ctx.owner.ClearRenderTargetView(&target.rtv, &[0.; 4]);
        ctx.owner
            .PSSetConstantBuffers(0, Some(&[Some(sentinel.clone())]));
    }
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    let draw = CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask {
            bounds: Bounds::new(bounds.origin, size(ScaledPixels(4.), ScaledPixels(8.))),
        },
        corner_radii: Corners::default(),
        opacity: 0.5,
        content: Arc::new(Unused),
        coverage: None,
    };
    CustomCompositor::new(&ctx.device)?.draw(
        &ctx.owner,
        &target.rtv,
        extent,
        &[draw],
        &[PreparedDraw {
            source: source.validate(&ctx, extent)?,
            coverage: None,
            native: false,
            wide: false,
        }],
    )?;
    let mut restored = [None];
    unsafe {
        ctx.owner.PSGetConstantBuffers(0, Some(&mut restored));
    }
    assert_eq!(restored[0].as_ref(), Some(&sentinel));
    let pixels = read(&ctx, &target.texture)?;
    for y in 0..8 {
        for x in 0..8 {
            let p = pixels[y * 8 + x];
            if x < 4 {
                assert!(
                    p[0].abs_diff(64) <= 1 && p[3].abs_diff(64) <= 1 && p[1] == 0 && p[2] == 0,
                    "{x},{y}: {p:?}"
                );
            } else {
                assert_eq!(p, [0; 4]);
            }
        }
    }
    Ok(())
}

#[test]
fn dx11_custom_failed_transactions_discard_encoded_work() -> Result<()> {
    let host = warp()?;
    let extent = size(DevicePixels(8), DevicePixels(8));
    for mode in 0..4 {
        let witness = allocate_texture(&host.device, extent)?;
        unsafe {
            host.owner
                .ClearRenderTargetView(&witness.rtv, &[1., 0., 0., 1.]);
        }
        let mut batch = PreparationBatch::new(host.device.clone(), host.owner.clone());
        let mut rejected_output = None;
        let result: Result<()> = batch.prepare(|context| {
            let output = context.create_output(extent)?;
            rejected_output = Some(output.clone());
            context.record(|deferred| {
                unsafe {
                    deferred.ClearRenderTargetView(&witness.rtv, &[0., 1., 0., 1.]);
                }
                Ok(())
            })?;
            if mode < 2 {
                if mode == 0 {
                    anyhow::bail!("producer rejected after encoding");
                }
                output.validate(context, size(DevicePixels(9), DevicePixels(8)))?;
            } else {
                let encoded: Result<()> = context.record(|deferred| {
                    unsafe {
                        deferred.ClearRenderTargetView(&witness.rtv, &[0., 1., 0., 1.]);
                    }
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
        let healthy = batch.prepare(|context| {
            assert!(!context.can_reuse(rejected_output.as_ref().unwrap()));
            let output = context.create_output(extent)?;
            context.clear(&output, [0., 0., 1., 1.])?;
            output.clone().validate(context, extent)?;
            Ok(output)
        })?;
        batch.submit()?;
        assert!(!host.can_reuse(rejected_output.as_ref().unwrap()));
        assert!(host.can_reuse(&healthy));
        assert!(
            read(&host, &witness.texture)?
                .iter()
                .all(|p| *p == [255, 0, 0, 255]),
            "failed commands ran in mode {mode}"
        );
        assert!(
            read(&host, healthy.texture())?
                .iter()
                .all(|p| *p == [0, 0, 255, 255])
        );
    }
    Ok(())
}

#[test]
fn dx11_custom_staged_reuse_and_submission_order() -> Result<()> {
    let host = warp()?;
    let extent = size(DevicePixels(8), DevicePixels(8));
    let destination = allocate_texture(&host.device, extent)?;
    let mut batch = PreparationBatch::new(host.device.clone(), host.owner.clone());
    let output = batch.prepare(|context| {
        let output = context.create_output(extent)?;
        context.clear(&output, [0., 1., 0., 1.])?;
        output.clone().validate(context, extent)?;
        Ok(output)
    })?;
    assert!(!host.can_reuse(&output));
    let rejected: Result<()> = batch.prepare(|context| {
        assert!(context.can_reuse(&output));
        context.record(|deferred| {
            unsafe {
                deferred.ClearRenderTargetView(&output.bundle.rtv, &[1., 0., 0., 1.]);
            }
            Ok(())
        })?;
        anyhow::bail!("failure after a staged cache hit");
    });
    assert!(rejected.is_err());
    batch.prepare(|context| {
        assert!(context.can_reuse(&output));
        output.clone().validate(context, extent)?;
        assert!(context.clear(&output, [1., 0., 0., 1.]).is_err());
        context.record(|deferred| {
            unsafe {
                deferred.CopyResource(&destination.texture, output.texture());
            }
            Ok(())
        })
    })?;
    batch.prepare(|context| output.clone().validate(context, extent))?;
    batch.submit()?;
    assert!(host.can_reuse(&output));
    assert!(
        read(&host, &destination.texture)?
            .iter()
            .all(|p| *p == [0, 255, 0, 255])
    );
    let mut later = PreparationBatch::new(host.device.clone(), host.owner.clone());
    later.prepare(|context| output.clone().validate(context, extent))?;
    drop(later);
    assert!(host.can_reuse(&output));
    Ok(())
}

#[test]
fn dx11_custom_dropped_batch_abandons_all_staged_work() -> Result<()> {
    let host = warp()?;
    let extent = size(DevicePixels(8), DevicePixels(8));
    let witness = allocate_texture(&host.device, extent)?;
    unsafe {
        host.owner
            .ClearRenderTargetView(&witness.rtv, &[1., 0., 0., 1.]);
    }
    let mut batch = PreparationBatch::new(host.device.clone(), host.owner.clone());
    let first = batch.prepare(|context| {
        context.record(|deferred| {
            unsafe {
                deferred.ClearRenderTargetView(&witness.rtv, &[0., 1., 0., 1.]);
            }
            Ok(())
        })?;
        context.create_output(extent)
    })?;
    let second = batch.prepare(|context| context.create_output(extent))?;
    let aborted: Result<()> = batch.prepare(|_| anyhow::bail!("later preparation failed"));
    assert!(aborted.is_err());
    drop(batch);
    let mut next = PreparationBatch::new(host.device.clone(), host.owner.clone());
    next.prepare(|context| {
        assert!(!context.can_reuse(&first));
        assert!(!context.can_reuse(&second));
        assert!(first.clone().validate(context, extent).is_err());
        assert!(second.clone().validate(context, extent).is_err());
        Ok(())
    })?;
    next.submit()?;
    assert!(
        read(&host, &witness.texture)?
            .iter()
            .all(|p| *p == [255, 0, 0, 255])
    );
    Ok(())
}

#[test]
fn dx11_custom_uniform_buffer_reuses_storage_and_preserves_queued_draw_values() -> Result<()> {
    let ctx = warp()?;
    let source_extent = size(DevicePixels(8), DevicePixels(8));
    let output = ctx.create_output(source_extent)?;
    ctx.clear(&output, [1., 0., 0., 1.])?;
    ctx.submit()?;
    let source = output.validate(&ctx, source_extent)?;
    let compositor = CustomCompositor::new(&ctx.device)?;
    let uniform = compositor.uniform.clone();
    let extent = size(DevicePixels(128), DevicePixels(8));
    let mut frames = Vec::new();
    for (phase, count) in [2, 7, 60, 300, 3, 300].into_iter().enumerate() {
        let target = allocate_texture(&ctx.device, extent)?;
        unsafe {
            ctx.owner.ClearRenderTargetView(&target.rtv, &[0.; 4]);
        }
        let draws: Vec<_> = (0..count)
            .map(|index| {
                let x = if phase.is_multiple_of(2) {
                    index
                } else {
                    count - 1 - index
                } as f32
                    * 2.;
                let bounds = Bounds::new(
                    point(ScaledPixels(x), ScaledPixels(0.)),
                    size(ScaledPixels(2.), ScaledPixels(8.)),
                );
                CustomDraw {
                    order: 0,
                    bounds,
                    content_mask: ContentMask {
                        bounds: Bounds::new(
                            bounds.origin,
                            size(ScaledPixels(2.), ScaledPixels((4 + index % 3) as f32)),
                        ),
                    },
                    corner_radii: Corners::default(),
                    opacity: ((index + phase) % 4 + 1) as f32 / 4.,
                    content: Arc::new(Unused),
                    coverage: None,
                }
            })
            .collect();
        let outputs: Vec<_> = (0..count)
            .map(|_| PreparedDraw {
                source: source.clone(),
                coverage: None,
                native: false,
                wide: false,
            })
            .collect();
        compositor.draw(&ctx.owner, &target.rtv, extent, &draws, &outputs)?;
        assert_eq!(compositor.uniform, uniform);
        assert_eq!(
            compositor.uniform_creations, 1,
            "no per-draw or per-frame CreateBuffer"
        );
        frames.push((target, phase, count));
    }
    // Only read after all frames have been queued; WRITE_DISCARD must preserve
    // earlier command-list values and frames, not expose the last map everywhere.
    for (target, phase, count) in frames {
        let pixels = read(&ctx, &target.texture)?;
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
                let pixel = pixels[y * 128 + x];
                assert!(
                    pixel[0].abs_diff(expected) <= 1,
                    "frame {phase}, ({x},{y}): {pixel:?}"
                );
                assert!(pixel[3].abs_diff(expected) <= 1);
                assert_eq!([pixel[1], pixel[2]], [0, 0]);
            }
        }
    }
    Ok(())
}

#[test]
fn dx11_source_image_readiness_and_size_contract() -> Result<()> {
    let first = warp()?;
    let data = gpui::ImageTextureData::new(2, 2, vec![vec![255; 16], vec![255; 4]])?;
    let output = first.create_image_output(&data)?;
    let second = Dx11CustomContext::new(first.device.clone(), first.owner.clone())?;
    assert!(!second.can_reuse(&output));
    first.submit()?;
    assert!(
        output
            .validate(&second, size(DevicePixels(8), DevicePixels(4)))
            .is_ok()
    );
    Ok(())
}

#[test]
fn dx11_worker_staging_can_be_readopted_after_abandoned_recording() -> Result<()> {
    let context = warp()?;
    let worker = context.image_upload_context();
    let data = gpui::ImageTextureData::new(2, 2, vec![vec![255; 16], vec![255; 4]])?;
    let staged = std::thread::spawn(move || worker.stage(&data))
        .join()
        .unwrap()?;
    let abandoned = {
        let other = Dx11CustomContext::new(context.device.clone(), context.owner.clone())?;
        other.adopt_image_upload(&staged)?
    };
    assert!(!context.can_reuse(&abandoned));
    let adopted = context.adopt_image_upload(&staged)?;
    assert!(!adopted.is_submitted());
    context.submit()?;
    assert!(adopted.is_submitted());
    let foreign = warp()?;
    assert!(foreign.adopt_image_upload(&staged).is_err());
    Ok(())
}

#[test]
fn dxgi_snapshot_import_retains_native_texture_and_checks_device() -> Result<()> {
    let host = warp()?;
    let extent = size(DevicePixels(8), DevicePixels(8));
    let source = host.create_output(extent)?;
    host.clear(&source, [1., 0., 0., 1.])?;
    host.submit()?;
    let texture = source.texture().clone();
    // SAFETY: dedicated texture, writes submitted on this host context; never mutated again.
    let frame = unsafe { dxgi::DxgiFrame::from_bgra(texture.clone()) }?;
    assert_eq!(frame.size(), extent);
    let mut batch = PreparationBatch::new(host.device.clone(), host.owner.clone());
    let sampled = batch.prepare(|context| {
        let output = frame.prepare(context, size(DevicePixels(32), DevicePixels(16)))?;
        let output = *output.downcast::<Dx11CustomOutput>().unwrap();
        assert_eq!(
            output.texture().as_raw(),
            texture.as_raw(),
            "import must not copy pixels"
        );
        output.validate(context, size(DevicePixels(32), DevicePixels(16)))
    })?;
    batch.submit()?;
    drop(frame);
    drop(source);
    drop(texture);
    assert!(
        read(&host, &sampled.texture)?
            .iter()
            .all(|p| *p == [255, 0, 0, 255])
    );
    let foreign = warp()?;
    // SAFETY: same immutable resource; preparation must reject the foreign device.
    let frame = unsafe { dxgi::DxgiFrame::from_bgra(sampled.texture.clone()) }?;
    assert!(frame.prepare(&foreign, extent).is_err());
    assert!(frame.prepare(&(), extent).is_err());
    let unsupported = allocate_texture_format(&host.device, extent, DXGI_FORMAT_R8G8B8A8_UNORM, 4)?;
    // SAFETY: no writes; descriptor rejection is under test.
    assert!(unsafe { dxgi::DxgiFrame::from_bgra(unsupported.texture) }.is_err());
    Ok(())
}

#[test]
fn dxgi_shared_snapshot_holds_key_until_last_prepared_reference() -> Result<()> {
    use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
    use windows::Win32::Graphics::Dxgi::{
        DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIKeyedMutex, IDXGIResource1,
    };
    use windows::core::PCWSTR;
    let producer = warp()?;
    let consumer = warp()?;
    let extent = size(DevicePixels(8), DevicePixels(8));
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 8,
        Height: 8,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0)
            as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe {
        producer
            .device
            .CreateTexture2D(&desc, None, Some(&mut texture))?
    };
    let texture = texture.context("missing shared texture")?;
    let mutex: IDXGIKeyedMutex = texture.cast()?;
    let raw_acquire =
        |key| unsafe { (Interface::vtable(&mutex).AcquireSync)(mutex.as_raw(), key, 0) };
    assert_eq!(raw_acquire(0).0, 0);
    let mut rtv = None;
    unsafe {
        producer
            .device
            .CreateRenderTargetView(&texture, None, Some(&mut rtv))?;
        producer
            .owner
            .ClearRenderTargetView(rtv.as_ref().unwrap(), &[1., 0., 0., 1.]);
        mutex.ReleaseSync(1)?;
    }
    let resource: IDXGIResource1 = texture.cast()?;
    let raw = unsafe {
        resource.CreateSharedHandle(
            None,
            DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0,
            PCWSTR::null(),
        )?
    };
    // SAFETY: CreateSharedHandle transfers a new owned NT handle to the caller.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    // SAFETY: both devices use the 1=ready, 0=producer protocol; no unsynchronized aliases write.
    assert!(
        unsafe {
            dxgi::DxgiFrame::try_from_shared_bgra(&consumer.device, handle.as_handle(), 7, 0)
        }?
        .is_none()
    );
    let frame = unsafe {
        dxgi::DxgiFrame::try_from_shared_bgra(&consumer.device, handle.as_handle(), 1, 0)
    }?
    .unwrap();
    assert!(
        unsafe {
            dxgi::DxgiFrame::try_from_shared_bgra(&consumer.device, handle.as_handle(), 1, 0)
        }?
        .is_none()
    );
    drop(handle); // Opening retains the native allocation independently of the NT handle.
    let clone = frame.clone();
    let mut batch = PreparationBatch::new(consumer.device.clone(), consumer.owner.clone());
    let source = batch.prepare(|context| {
        let output = frame.prepare(context, extent)?;
        output
            .downcast::<Dx11CustomOutput>()
            .unwrap()
            .validate(context, extent)
    })?;
    batch.submit()?;
    drop(frame);
    drop(clone);
    assert_eq!(
        raw_acquire(0).0,
        windows::Win32::Foundation::WAIT_TIMEOUT.0 as i32,
        "prepared output must keep producer locked after frame handles drop"
    );
    let target = allocate_texture(&consumer.device, extent)?;
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    let draw = CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::default(),
        opacity: 1.,
        content: Arc::new(Unused),
        coverage: None,
    };
    let compositor = CustomCompositor::new(&consumer.device)?;
    let prepared = PreparedDraw {
        source,
        coverage: None,
        native: false,
        wide: false,
    };
    compositor.draw(
        &consumer.owner,
        &target.rtv,
        extent,
        &[draw],
        std::slice::from_ref(&prepared),
    )?;
    drop(prepared); // Ownership handoff follows actual compositor command-list execution.
    assert_eq!(raw_acquire(0).0, 0, "last output releases the producer key");
    unsafe { mutex.ReleaseSync(1)? };
    assert!(
        read(&consumer, &target.texture)?
            .iter()
            .all(|p| *p == [255, 0, 0, 255])
    );
    Ok(())
}
