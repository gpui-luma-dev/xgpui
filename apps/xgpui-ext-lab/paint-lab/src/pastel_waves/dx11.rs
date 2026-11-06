//! Native D3D11 producer for pastel waves: producer.wgsl translated to shader
//! model 5.0 (see hlsl.rs). A compute shader writes per-column curves into a
//! raw buffer, then one pixel shader shades the host-allocated output.
use super::native::PastelWavesNative;
use anyhow::{Context as _, Result};
use gpui::{DevicePixels, Size};
use gpui_windows::custom_draw::Dx11CustomContext;
use std::{
    any::Any,
    ffi::CString,
    sync::{Arc, Mutex},
};
use windows::{
    Win32::Graphics::{
        Direct3D::{Fxc::*, *},
        Direct3D11::*,
        Dxgi::Common::*,
    },
    core::{Interface, PCSTR},
};

struct Pipelines {
    device: ID3D11Device,
    columns: ID3D11ComputeShader,
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    raster: ID3D11RasterizerState,
}
struct PipelineCache(Mutex<Option<Arc<Pipelines>>>);
// SAFETY: D3D11 devices and the shader/state objects they create are
// free-threaded; the mutex serializes replacement of the cached entry.
unsafe impl Send for PipelineCache {}
unsafe impl Sync for PipelineCache {}
static PIPELINES: PipelineCache = PipelineCache(Mutex::new(None));

/// Matches `@workgroup_size` of `pastel_columns`.
const WORKGROUP: u32 = 64;
/// WGSL binding numbers, which hlsl.rs keeps as registers.
const COLUMNS_UAV: u32 = 1;
const COLUMNS_SRV: u32 = 2;

/// Compile once per host device.
fn pipelines(ctx: &Dx11CustomContext) -> Result<Arc<Pipelines>> {
    let mut cached = PIPELINES
        .0
        .lock()
        .map_err(|_| anyhow::anyhow!("pastel waves pipeline cache poisoned"))?;
    if let Some(entry) = cached.as_ref()
        && entry.device.as_raw() == ctx.device.as_raw()
    {
        return Ok(entry.clone());
    }
    let hlsl = super::hlsl::producer_hlsl()?;
    let compute = compile(&hlsl.source, &hlsl.compute, "cs_5_0")?;
    let vertex = compile(&hlsl.source, &hlsl.vertex, "vs_5_0")?;
    let fragment = compile(&hlsl.source, &hlsl.fragment, "ps_5_0")?;
    let (mut columns, mut vs, mut ps, mut raster) = (None, None, None, None);
    unsafe {
        ctx.device
            .CreateComputeShader(&compute, None, Some(&mut columns))?;
        ctx.device
            .CreateVertexShader(&vertex, None, Some(&mut vs))?;
        ctx.device
            .CreatePixelShader(&fragment, None, Some(&mut ps))?;
        ctx.device.CreateRasterizerState(
            &D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                CullMode: D3D11_CULL_NONE,
                DepthClipEnable: true.into(),
                ..Default::default()
            },
            Some(&mut raster),
        )?;
    }
    let entry = Arc::new(Pipelines {
        device: ctx.device.clone(),
        columns: columns.context("missing compute shader")?,
        vertex: vs.context("missing vertex shader")?,
        fragment: ps.context("missing pixel shader")?,
        raster: raster.context("missing rasterizer state")?,
    });
    *cached = Some(entry.clone());
    Ok(entry)
}

fn compile(source: &str, entry: &str, target: &str) -> Result<Vec<u8>> {
    let entry = CString::new(entry)?;
    let target = CString::new(target)?;
    let (mut code, mut errors) = (None, None);
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR(c"pastel-waves.hlsl".as_ptr().cast()),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        let message = errors
            .map(|blob| unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(
                    blob.GetBufferPointer().cast(),
                    blob.GetBufferSize(),
                ))
                .into_owned()
            })
            .unwrap_or_default();
        anyhow::bail!("pastel waves HLSL compilation: {error}: {message}");
    }
    let code = code.context("missing compiled pastel waves shader")?;
    Ok(unsafe {
        std::slice::from_raw_parts(code.GetBufferPointer().cast(), code.GetBufferSize()).to_vec()
    })
}

/// Record both passes into the host batch and return the output texture.
pub(super) fn prepare(
    waves: &PastelWavesNative,
    ctx: &Dx11CustomContext,
    extent: Size<DevicePixels>,
) -> Result<Box<dyn Any>> {
    let pipelines = pipelines(ctx)?;
    let device = &ctx.device;
    let width = extent.width.0.max(1) as usize;
    let height = extent.height.0.max(1) as usize;

    let uniforms = waves.uniforms(width, height);
    let bytes = uniforms.as_bytes();
    let mut constants = None;
    unsafe {
        device.CreateBuffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: bytes.len() as u32,
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            },
            Some(&D3D11_SUBRESOURCE_DATA {
                pSysMem: bytes.as_ptr().cast(),
                ..Default::default()
            }),
            Some(&mut constants),
        )?;
    }
    let constants = constants.context("missing pastel waves constants")?;

    // 9 ribbons x 2 float4 per column as a raw buffer: naga lowers WGSL
    // storage arrays to (RW)ByteAddressBuffer.
    let column_bytes = (width * 9 * 2 * 16) as u32;
    let mut columns = None;
    unsafe {
        device.CreateBuffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: column_bytes,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_UNORDERED_ACCESS.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
                ..Default::default()
            },
            None,
            Some(&mut columns),
        )?;
    }
    let columns = columns.context("missing pastel waves column buffer")?;
    let (mut uav, mut srv) = (None, None);
    unsafe {
        device.CreateUnorderedAccessView(
            &columns,
            Some(&D3D11_UNORDERED_ACCESS_VIEW_DESC {
                Format: DXGI_FORMAT_R32_TYPELESS,
                ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                    Buffer: D3D11_BUFFER_UAV {
                        FirstElement: 0,
                        NumElements: column_bytes / 4,
                        Flags: D3D11_BUFFER_UAV_FLAG_RAW.0 as u32,
                    },
                },
            }),
            Some(&mut uav),
        )?;
        device.CreateShaderResourceView(
            &columns,
            Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R32_TYPELESS,
                ViewDimension: D3D11_SRV_DIMENSION_BUFFEREX,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                    BufferEx: D3D11_BUFFEREX_SRV {
                        FirstElement: 0,
                        NumElements: column_bytes / 4,
                        Flags: D3D11_BUFFEREX_SRV_FLAG_RAW.0 as u32,
                    },
                },
            }),
            Some(&mut srv),
        )?;
    }
    let (uav, srv) = (
        uav.context("missing pastel waves column UAV")?,
        srv.context("missing pastel waves column SRV")?,
    );

    let output = ctx.create_output(extent)?;
    let mut rtv = None;
    unsafe {
        device.CreateRenderTargetView(output.texture(), None, Some(&mut rtv))?;
    }
    let rtv = rtv.context("missing pastel waves render target")?;

    ctx.record(|context| {
        unsafe {
            context.CSSetShader(&pipelines.columns, None);
            context.CSSetConstantBuffers(0, Some(&[Some(constants.clone())]));
            context.CSSetUnorderedAccessViews(COLUMNS_UAV, 1, Some(&Some(uav.clone())), None);
            context.Dispatch((width as u32).div_ceil(WORKGROUP), 1, 1);
            // Unbind so the buffer can be read as a shader resource.
            context.CSSetUnorderedAccessViews(COLUMNS_UAV, 1, Some(&None), None);
            context.CSSetShader(None::<&ID3D11ComputeShader>, None);

            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&pipelines.vertex, None);
            context.PSSetShader(&pipelines.fragment, None);
            context.RSSetState(&pipelines.raster);
            context.OMSetBlendState(None, None, u32::MAX);
            context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: width as f32,
                Height: height as f32,
                MinDepth: 0.,
                MaxDepth: 1.,
                ..Default::default()
            }]));
            context.VSSetConstantBuffers(0, Some(&[Some(constants.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(constants.clone())]));
            context.PSSetShaderResources(COLUMNS_SRV, Some(&[Some(srv.clone())]));
            context.Draw(3, 0);
            context.PSSetShaderResources(COLUMNS_SRV, Some(&[None]));
        }
        Ok(())
    })?;
    Ok(Box::new(output))
}
