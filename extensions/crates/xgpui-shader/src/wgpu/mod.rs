//! Portable WGSL effect implementations; GPUI supplies only recording and coverage.
use anyhow::Result;
use gpui::{DevicePixels, Size};
use gpui_wgpu::{
    custom_draw::{WgpuCustomContext, WgpuCustomOutput},
    wgpu,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use wgpu::util::DeviceExt;

struct CachedPipeline {
    device: Arc<wgpu::Device>,
    pipeline: wgpu::RenderPipeline,
}

#[derive(Hash, PartialEq, Eq)]
struct OutputKey {
    queue: usize,
    width: i32,
    height: i32,
    logical: Option<[u32; 2]>,
}

pub(crate) struct DrawRequest<'a> {
    pub extent: Size<DevicePixels>,
    pub source: &'a str,
    pub uniforms: &'a [&'a [u8]],
    pub vertices: u32,
    pub logical: Option<[u32; 2]>,
}

#[derive(Default)]
pub(crate) struct PaintCache {
    pipeline: Arc<Mutex<Option<CachedPipeline>>>,
    compilations: Arc<AtomicUsize>,
    outputs: Mutex<HashMap<OutputKey, WgpuCustomOutput>>,
}
impl PaintCache {
    pub fn snapshot(&self) -> Self {
        Self {
            pipeline: self.pipeline.clone(),
            compilations: self.compilations.clone(),
            outputs: Mutex::default(),
        }
    }
    pub fn compilations(&self) -> usize {
        self.compilations.load(Ordering::Relaxed)
    }
    fn cached(&self, ctx: &WgpuCustomContext, key: &OutputKey) -> Result<Option<WgpuCustomOutput>> {
        Ok(self
            .outputs
            .lock()
            .map_err(|_| anyhow::anyhow!("wgpu output cache poisoned"))?
            .get(key)
            .filter(|o| ctx.can_reuse(o))
            .cloned())
    }
    fn store(&self, key: OutputKey, output: &WgpuCustomOutput) -> Result<()> {
        let mut outputs = self
            .outputs
            .lock()
            .map_err(|_| anyhow::anyhow!("wgpu output cache poisoned"))?;
        let bytes: u64 = outputs
            .values()
            .map(|o| o.texture().width() as u64 * o.texture().height() as u64 * 4)
            .sum();
        let added = output.texture().width() as u64 * output.texture().height() as u64 * 4;
        if bytes + added > 64 * 1024 * 1024 {
            outputs.clear();
        }
        outputs.insert(key, output.clone());
        Ok(())
    }
    pub fn clear(
        &self,
        ctx: &WgpuCustomContext,
        size: Size<DevicePixels>,
        rgba: [f32; 4],
    ) -> Result<(WgpuCustomOutput, bool)> {
        let key = OutputKey {
            queue: ctx.cache_key(),
            width: size.width.0,
            height: size.height.0,
            logical: None,
        };
        if let Some(output) = self.cached(ctx, &key)? {
            return Ok((output, false));
        }
        let output = ctx.create_output(size)?;
        let view = output.texture().create_view(&Default::default());
        ctx.record(|encoder| {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("solid_paint"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: rgba[0] as f64,
                            g: rgba[1] as f64,
                            b: rgba[2] as f64,
                            a: rgba[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            Ok(())
        })?;
        self.store(key, &output)?;
        Ok((output, true))
    }
    fn render(
        &self,
        ctx: &WgpuCustomContext,
        size: Size<DevicePixels>,
        shader: &str,
        params: &[[f32; 4]],
        vertices: u32,
    ) -> Result<(WgpuCustomOutput, bool)> {
        self.render_draw(
            ctx,
            DrawRequest {
                extent: size,
                source: shader,
                uniforms: &[bytemuck::cast_slice(params)],
                vertices,
                logical: None,
            },
        )
    }
    pub(crate) fn render_draw(
        &self,
        ctx: &WgpuCustomContext,
        request: DrawRequest<'_>,
    ) -> Result<(WgpuCustomOutput, bool)> {
        let DrawRequest {
            extent: size,
            source: shader,
            uniforms,
            vertices,
            logical,
        } = request;
        let key = OutputKey {
            queue: ctx.cache_key(),
            width: size.width.0,
            height: size.height.0,
            logical,
        };
        if let Some(output) = self.cached(ctx, &key)? {
            return Ok((output, false));
        }
        let pipeline = {
            let mut cached = self
                .pipeline
                .lock()
                .map_err(|_| anyhow::anyhow!("wgpu pipeline cache poisoned"))?;
            if cached
                .as_ref()
                .is_none_or(|cached| !Arc::ptr_eq(&cached.device, &ctx.device))
            {
                // Browser error-scope futures require returning to the JS event loop.
                // Keep native synchronous validation; on Wasm, use the host device's
                // uncaptured-error handler rather than blocking or swallowing errors.
                #[cfg(not(target_family = "wasm"))]
                let error_scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
                let module = ctx
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("extension_paint"),
                        source: wgpu::ShaderSource::Wgsl(shader.into()),
                    });
                let entries: Vec<_> = uniforms
                    .iter()
                    .enumerate()
                    .map(|(index, _)| wgpu::BindGroupLayoutEntry {
                        binding: index as u32,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    })
                    .collect();
                let layout =
                    ctx.device
                        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                            label: Some("paint_parameters"),
                            entries: &entries,
                        });
                let layout = ctx
                    .device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("extension_paint"),
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: 0,
                    });
                let pipeline = ctx
                    .device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("extension_paint"),
                        layout: Some(&layout),
                        vertex: wgpu::VertexState {
                            module: &module,
                            entry_point: Some("paint_vertex"),
                            buffers: &[],
                            compilation_options: Default::default(),
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some("paint_fragment"),
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
                #[cfg(not(target_family = "wasm"))]
                if let Some(error) = gpui::block_on(error_scope.pop()) {
                    anyhow::bail!("wgpu paint pipeline: {error}");
                }
                *cached = Some(CachedPipeline {
                    device: ctx.device.clone(),
                    pipeline,
                });
                self.compilations.fetch_add(1, Ordering::Relaxed);
            }
            cached.as_ref().unwrap().pipeline.clone()
        };
        let buffers: Vec<_> = uniforms
            .iter()
            .map(|bytes| {
                ctx.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("paint_parameters"),
                        contents: bytes,
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect();
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(index, buffer)| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let bind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("paint_parameters"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let output = ctx.create_output(size)?;
        let view = output.texture().create_view(&Default::default());
        ctx.record(|encoder| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("extension_paint"),
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
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..vertices, 0..1);
            Ok(())
        })?;
        self.store(key, &output)?;
        Ok((output, true))
    }
}

pub(crate) fn gradient(
    paint: &crate::Gradient,
    ctx: &WgpuCustomContext,
    extent: Size<DevicePixels>,
) -> Result<Box<dyn std::any::Any>> {
    let params = crate::effect_params::gradient(paint, extent);
    let (output, generated) =
        paint
            .wgpu
            .render(ctx, extent, include_str!("gradient.wgsl"), &params, 6)?;
    if generated {
        paint.preparations.fetch_add(1, Ordering::Relaxed);
    }
    Ok(Box::new(output))
}

pub(crate) fn mesh(
    paint: &crate::MeshGradient,
    ctx: &WgpuCustomContext,
    extent: Size<DevicePixels>,
) -> Result<Box<dyn std::any::Any>> {
    let params = crate::effect_params::mesh(paint);
    let (output, generated) = paint.wgpu.render(
        ctx,
        extent,
        include_str!("mesh.wgsl"),
        &params,
        paint.stats().triangles * 3,
    )?;
    if generated {
        paint.preparations.fetch_add(1, Ordering::Relaxed);
    }
    Ok(Box::new(output))
}

#[cfg(test)]
mod tests;
