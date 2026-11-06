//! Native wgpu producer for pastel waves (the Linux renderer; also reachable
//! through the headless wgpu renderer elsewhere). Runs producer.wgsl: a compute
//! pass writes per-column curves, then one fragment pass shades the target.
use super::native::PastelWavesNative;
use anyhow::Result;
use gpui::{DevicePixels, Size};
use gpui_wgpu::{custom_draw::WgpuCustomContext, wgpu};
use std::{
    any::Any,
    sync::{Arc, Mutex},
};
use wgpu::util::DeviceExt;

struct Pipelines {
    device: Arc<wgpu::Device>,
    columns: wgpu::ComputePipeline,
    shade: wgpu::RenderPipeline,
}
/// Compile once per host device.
static PIPELINES: Mutex<Option<Arc<Pipelines>>> = Mutex::new(None);

/// Matches `@workgroup_size` of `pastel_columns`.
const WORKGROUP: u32 = 64;

fn pipelines(ctx: &WgpuCustomContext) -> Result<Arc<Pipelines>> {
    let mut cached = PIPELINES
        .lock()
        .map_err(|_| anyhow::anyhow!("pastel waves pipeline cache poisoned"))?;
    if let Some(entry) = cached.as_ref()
        && Arc::ptr_eq(&entry.device, &ctx.device)
    {
        return Ok(entry.clone());
    }
    let device = &ctx.device;
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("pastel_waves"),
        source: wgpu::ShaderSource::Wgsl(super::producer_source().into()),
    });
    // Automatic layouts: the compute entry binds 0 and 1, the render entries 0 and 2.
    let columns = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("pastel_columns"),
        layout: None,
        module: &module,
        entry_point: Some("pastel_columns"),
        compilation_options: Default::default(),
        cache: None,
    });
    let shade = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("pastel_shade"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("pastel_vertex"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("pastel_fragment"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    if let Some(error) = gpui::block_on(error_scope.pop()) {
        anyhow::bail!("pastel waves wgpu pipelines: {error}");
    }
    let entry = Arc::new(Pipelines {
        device: device.clone(),
        columns,
        shade,
    });
    *cached = Some(entry.clone());
    Ok(entry)
}

/// Record both passes into the host batch and return the output texture.
pub(super) fn prepare(
    waves: &PastelWavesNative,
    ctx: &WgpuCustomContext,
    extent: Size<DevicePixels>,
) -> Result<Box<dyn Any>> {
    let pipelines = pipelines(ctx)?;
    let device = &ctx.device;
    let width = extent.width.0.max(1) as usize;
    let height = extent.height.0.max(1) as usize;
    let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pastel_uniforms"),
        contents: waves.uniforms(width, height).as_bytes(),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    // 9 ribbons x 2 vec4 per column, written and read only on the GPU.
    let columns = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pastel_columns"),
        size: (width * 9 * 2 * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let compute_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("pastel_columns"),
        layout: &pipelines.columns.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: columns.as_entire_binding(),
            },
        ],
    });
    let shade_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("pastel_shade"),
        layout: &pipelines.shade.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: columns.as_entire_binding(),
            },
        ],
    });
    let output = ctx.create_output(extent)?;
    let view = output.texture().create_view(&Default::default());
    ctx.record(|encoder| {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("pastel_columns"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines.columns);
            pass.set_bind_group(0, &compute_bind, &[]);
            pass.dispatch_workgroups((width as u32).div_ceil(WORKGROUP), 1, 1);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pastel_shade"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipelines.shade);
        pass.set_bind_group(0, &shade_bind, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    })?;
    Ok(Box::new(output))
}
