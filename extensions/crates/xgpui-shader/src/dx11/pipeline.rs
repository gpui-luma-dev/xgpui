//! Producer pipeline compilation and drawing are extension-owned, not GPUI API.
use anyhow::{Context as _, Result, ensure};
use gpui_windows::custom_draw::{Dx11CustomContext, Dx11CustomOutput};
use std::ffi::CString;
use windows::{
    Win32::Graphics::{
        Direct3D::{Fxc::*, *},
        Direct3D11::*,
    },
    core::{Interface, PCSTR},
};
/// Extension pipeline compiled from generated HLSL shader model 5.0.
pub(super) struct ShaderPipeline {
    device: ID3D11Device,
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    raster: ID3D11RasterizerState,
    depth: ID3D11DepthStencilState,
}
impl ShaderPipeline {
    /// Compile shaders; entry names come from the extension's translation metadata.
    pub fn new(
        device: &ID3D11Device,
        source: &str,
        vertex_entry: &str,
        fragment_entry: &str,
    ) -> Result<Self> {
        let vertex_bytes = compile(source, vertex_entry, "vs_5_0")?;
        let fragment_bytes = compile(source, fragment_entry, "ps_5_0")?;
        let (mut vertex, mut fragment, mut raster, mut depth) = (None, None, None, None);
        unsafe {
            device.CreateVertexShader(&vertex_bytes, None, Some(&mut vertex))?;
            device.CreatePixelShader(&fragment_bytes, None, Some(&mut fragment))?;
            device.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    DepthClipEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut raster),
            )?;
            device.CreateDepthStencilState(
                &D3D11_DEPTH_STENCIL_DESC {
                    DepthEnable: false.into(),
                    DepthFunc: D3D11_COMPARISON_ALWAYS,
                    DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                    StencilEnable: false.into(),
                    FrontFace: D3D11_DEPTH_STENCILOP_DESC {
                        StencilFailOp: D3D11_STENCIL_OP_KEEP,
                        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
                        StencilPassOp: D3D11_STENCIL_OP_KEEP,
                        StencilFunc: D3D11_COMPARISON_ALWAYS,
                    },
                    BackFace: D3D11_DEPTH_STENCILOP_DESC {
                        StencilFailOp: D3D11_STENCIL_OP_KEEP,
                        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
                        StencilPassOp: D3D11_STENCIL_OP_KEEP,
                        StencilFunc: D3D11_COMPARISON_ALWAYS,
                    },
                    ..Default::default()
                },
                Some(&mut depth),
            )?;
        }
        Ok(Self {
            device: device.clone(),
            vertex: vertex.unwrap(),
            fragment: fragment.unwrap(),
            raster: raster.unwrap(),
            depth: depth.unwrap(),
        })
    }
    /// Record an extension pipeline with immutable uniform buffers and no blending.
    pub(super) fn render(
        &self,
        context: &Dx11CustomContext,
        output: &Dx11CustomOutput,
        uniforms: &[&[u8]],
        vertices: u32,
    ) -> Result<()> {
        ensure!(
            uniforms.len() <= D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT as usize,
            "too many DirectX uniform buffers"
        );
        ensure!(
            self.device.as_raw() == context.device.as_raw(),
            "DirectX pipeline belongs to another device"
        );
        let buffers: Vec<_> = uniforms
            .iter()
            .map(|bytes| constant_buffer(&context.device, bytes).map(Some))
            .collect::<Result<_>>()?;
        let mut rtv = None;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            output.texture().GetDesc(&mut desc);
            context
                .device
                .CreateRenderTargetView(output.texture(), None, Some(&mut rtv))?;
        }
        let rtv = rtv.context("missing extension render target")?;
        context.record(|context| {
            self.bind(context);
            unsafe {
                context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
                context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                    Width: desc.Width as f32,
                    Height: desc.Height as f32,
                    MinDepth: 0.,
                    MaxDepth: 1.,
                    ..Default::default()
                }]));
                context.VSSetConstantBuffers(0, Some(&buffers));
                context.PSSetConstantBuffers(0, Some(&buffers));
                context.Draw(vertices, 0);
            }
            Ok(())
        })
    }
    fn bind(&self, context: &ID3D11DeviceContext) {
        unsafe {
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.fragment, None);
            context.RSSetState(&self.raster);
            context.OMSetDepthStencilState(&self.depth, 0);
            context.OMSetBlendState(None, None, u32::MAX);
        }
    }
}
fn compile(source: &str, entry: &str, target: &str) -> Result<Vec<u8>> {
    let entry = CString::new(entry)?;
    let target = CString::new(target)?;
    let (mut code, mut errors) = (None, None);
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR(c"xgpui-custom.hlsl".as_ptr().cast()),
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
        anyhow::bail!("DirectX custom shader compilation: {error}: {message}");
    }
    let code = code.context("missing compiled DirectX shader")?;
    Ok(unsafe {
        std::slice::from_raw_parts(code.GetBufferPointer().cast(), code.GetBufferSize()).to_vec()
    })
}
fn constant_buffer(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11Buffer> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= 65536,
        "invalid DirectX uniform size"
    );
    let mut padded = bytes.to_vec();
    padded.resize(bytes.len().next_multiple_of(16), 0);
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: padded.len() as u32,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: padded.as_ptr().cast(),
        ..Default::default()
    };
    let mut buffer = None;
    unsafe {
        device.CreateBuffer(&desc, Some(&data), Some(&mut buffer))?;
    }
    buffer.context("missing uniform buffer")
}

pub(super) fn clear(
    context: &Dx11CustomContext,
    output: &Dx11CustomOutput,
    rgba: [f32; 4],
) -> Result<()> {
    let mut rtv = None;
    unsafe {
        context
            .device
            .CreateRenderTargetView(output.texture(), None, Some(&mut rtv))?;
    }
    let rtv = rtv.context("missing extension render target")?;
    context.record(|deferred| {
        unsafe {
            deferred.ClearRenderTargetView(&rtv, &rgba);
        }
        Ok(())
    })
}
