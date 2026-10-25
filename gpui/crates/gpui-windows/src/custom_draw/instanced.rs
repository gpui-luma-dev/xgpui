//! Main-target instanced draws. Deferred lists restore the surrounding native state.
use super::*;
use gpui::{Scene, SceneShaderProgram};
use std::{collections::HashMap, ops::Range};

struct Pipeline {
    shader: CompositorPipeline,
    layout: ID3D11InputLayout,
}
impl Pipeline {
    fn new(device: &ID3D11Device, program: &SceneShaderProgram) -> Result<Self> {
        let (source, vertex, fragment) = program.hlsl();
        let bytes = compile(source, vertex, "vs_5_0")?;
        let elements: Vec<_> = (0..11)
            .map(|i| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(c"LOC".as_ptr().cast()),
                SemanticIndex: i,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: i * 16,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            })
            .collect();
        let mut layout = None;
        unsafe {
            device.CreateInputLayout(&elements, &bytes, Some(&mut layout))?;
        }
        Ok(Self {
            shader: CompositorPipeline::new(device, source, vertex, fragment)?,
            layout: layout.context("shader input layout")?,
        })
    }
}

pub(crate) struct ShaderRenderer {
    pipelines: HashMap<u64, Pipeline>,
    instances: ID3D11Buffer,
    capacity: usize,
    globals: ID3D11Buffer,
    blend: ID3D11BlendState,
    deferred: ID3D11DeviceContext,
}
impl ShaderRenderer {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let mut desc = D3D11_BLEND_DESC::default();
        desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_SRC_ALPHA,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let (mut blend, mut deferred) = (None, None);
        unsafe {
            device.CreateBlendState(&desc, Some(&mut blend))?;
            device.CreateDeferredContext(0, Some(&mut deferred))?;
        }
        Ok(Self {
            pipelines: HashMap::new(),
            instances: instance_buffer(device, 176)?,
            capacity: 176,
            globals: dynamic_uniform_buffer(device)?,
            blend: blend.context("shader blend")?,
            deferred: deferred.context("shader context")?,
        })
    }
    pub fn prepare(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        scene: &Scene,
    ) -> Result<()> {
        if scene.shader_draws.is_empty() {
            return Ok(());
        }
        self.pipelines
            .retain(|id, _| scene.shader_draws.iter().any(|d| d.program.id() == *id));
        let bytes = scene
            .shader_draws
            .len()
            .checked_mul(176)
            .context("shader size overflow")?;
        ensure!(
            bytes <= 64 * 1024 * 1024,
            "shader instance data exceeds 64 MiB"
        );
        if bytes == 0 {
            return Ok(());
        }
        for draw in &scene.shader_draws {
            if !self.pipelines.contains_key(&draw.program.id()) {
                self.pipelines
                    .insert(draw.program.id(), Pipeline::new(device, &draw.program)?);
            }
        }
        if bytes > self.capacity {
            self.capacity = bytes.next_power_of_two();
            self.instances = instance_buffer(device, self.capacity)?;
        }
        let records: Vec<_> = scene
            .shader_draws
            .iter()
            .map(|d| d.instance_data())
            .collect();
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context.Map(
                &self.instances,
                0,
                D3D11_MAP_WRITE_DISCARD,
                0,
                Some(&mut mapped),
            )?;
            std::ptr::copy_nonoverlapping(
                records.as_ptr().cast::<u8>(),
                mapped.pData.cast(),
                bytes,
            );
            context.Unmap(&self.instances, 0);
        }
        Ok(())
    }
    pub fn draw(
        &self,
        context: &ID3D11DeviceContext,
        target: &ID3D11RenderTargetView,
        extent: Size<DevicePixels>,
        program: u64,
        range: Range<usize>,
    ) -> Result<()> {
        let pipeline = &self.pipelines[&program];
        let dc = &self.deferred;
        pipeline.shader.bind(dc);
        let globals = [extent.width.0 as f32, extent.height.0 as f32, 0., 0.];
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            dc.Map(
                &self.globals,
                0,
                D3D11_MAP_WRITE_DISCARD,
                0,
                Some(&mut mapped),
            )?;
            std::ptr::copy_nonoverlapping(globals.as_ptr().cast::<u8>(), mapped.pData.cast(), 16);
            dc.Unmap(&self.globals, 0);
            dc.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
            dc.RSSetViewports(Some(&[viewport(extent)]));
            dc.OMSetBlendState(&self.blend, None, u32::MAX);
            dc.IASetInputLayout(&pipeline.layout);
            dc.IASetVertexBuffers(
                0,
                1,
                Some([Some(self.instances.clone())].as_ptr()),
                Some(&176),
                Some(&((range.start * 176) as u32)),
            );
            dc.VSSetConstantBuffers(0, Some(&[Some(self.globals.clone())]));
            dc.PSSetConstantBuffers(0, Some(&[Some(self.globals.clone())]));
            dc.DrawInstanced(6, range.len() as u32, 0, 0);
            let mut commands = None;
            dc.FinishCommandList(false, Some(&mut commands))?;
            context.ExecuteCommandList(commands.as_ref().context("shader command list")?, true);
            pipeline.shader.device.GetDeviceRemovedReason()?;
        }
        Ok(())
    }
}
fn instance_buffer(device: &ID3D11Device, size: usize) -> Result<ID3D11Buffer> {
    let mut buffer = None;
    unsafe {
        device.CreateBuffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: size as u32,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                ..Default::default()
            },
            None,
            Some(&mut buffer),
        )?;
    }
    buffer.context("shader instance buffer")
}
