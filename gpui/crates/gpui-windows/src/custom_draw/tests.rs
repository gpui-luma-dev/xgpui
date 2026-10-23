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
    let sentinel = constant_buffer(&ctx.device, &[7; 16])?;
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
