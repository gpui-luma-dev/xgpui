//! Native D3D11 effects compiled from the same WGSL as the portable backend.
use anyhow::Result;
use gpui::{DevicePixels, Size};
use gpui_windows::custom_draw::{Dx11CustomContext, Dx11CustomOutput};
mod pipeline;
use pipeline::ShaderPipeline;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

struct CachedPipeline {
    device: ID3D11Device,
    pipeline: Arc<ShaderPipeline>,
}
#[derive(Hash, PartialEq, Eq)]
struct OutputKey {
    context: usize,
    width: i32,
    height: i32,
    logical: Option<[u32; 2]>,
}
#[derive(Default)]
pub(crate) struct PaintCache {
    pipeline: Arc<Mutex<Option<CachedPipeline>>>,
    compilations: Arc<AtomicUsize>,
    outputs: Mutex<HashMap<OutputKey, Dx11CustomOutput>>,
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
    pub fn texture_info(&self) -> Option<(String, u64)> {
        let outputs = self.outputs.lock().ok()?;
        if outputs.is_empty() {
            return None;
        }
        Some((
            "DirectX 11".into(),
            outputs
                .keys()
                .map(|key| key.width as u64 * key.height as u64 * 4)
                .sum(),
        ))
    }
    fn output(
        &self,
        ctx: &Dx11CustomContext,
        extent: Size<DevicePixels>,
        logical: Option<[u32; 2]>,
        draw: impl FnOnce(&Dx11CustomOutput) -> Result<()>,
    ) -> Result<(Dx11CustomOutput, bool)> {
        let mut outputs = self
            .outputs
            .lock()
            .map_err(|_| anyhow::anyhow!("D3D11 output cache poisoned"))?;
        let key = OutputKey {
            context: ctx.cache_key(),
            width: extent.width.0,
            height: extent.height.0,
            logical,
        };
        if let Some(output) = outputs.get(&key).filter(|o| ctx.can_reuse(o)) {
            return Ok((output.clone(), false));
        }
        let output = ctx.create_output(extent)?;
        draw(&output)?;
        let bytes: u64 = outputs
            .keys()
            .map(|key| key.width as u64 * key.height as u64 * 4)
            .sum();
        if bytes + key.width as u64 * key.height as u64 * 4 > 64 * 1024 * 1024 {
            outputs.clear();
        }
        outputs.insert(key, output.clone());
        Ok((output, true))
    }
    pub fn clear(
        &self,
        ctx: &Dx11CustomContext,
        extent: Size<DevicePixels>,
        rgba: [f32; 4],
    ) -> Result<(Dx11CustomOutput, bool)> {
        self.output(ctx, extent, None, |o| pipeline::clear(ctx, o, rgba))
    }
    fn render(
        &self,
        ctx: &Dx11CustomContext,
        extent: Size<DevicePixels>,
        source: &str,
        params: &[[f32; 4]],
        vertices: u32,
    ) -> Result<(Dx11CustomOutput, bool)> {
        self.render_uniforms(
            ctx,
            extent,
            source,
            &[bytemuck::cast_slice(params)],
            vertices,
            None,
        )
    }
    pub(crate) fn render_uniforms(
        &self,
        ctx: &Dx11CustomContext,
        extent: Size<DevicePixels>,
        source: &str,
        uniforms: &[&[u8]],
        vertices: u32,
        logical: Option<[u32; 2]>,
    ) -> Result<(Dx11CustomOutput, bool)> {
        self.output(ctx, extent, logical, |output| {
            let mut cached = self
                .pipeline
                .lock()
                .map_err(|_| anyhow::anyhow!("D3D11 pipeline cache poisoned"))?;
            if cached.as_ref().is_none_or(|p| p.device != ctx.device) {
                let (source, vertex, fragment) = crate::hlsl::compile_wgsl(source)?;
                let pipeline = ShaderPipeline::new(&ctx.device, &source, &vertex, &fragment)?;
                *cached = Some(CachedPipeline {
                    device: ctx.device.clone(),
                    pipeline: Arc::new(pipeline),
                });
                self.compilations.fetch_add(1, Ordering::Relaxed);
            }
            pipeline::clear(ctx, output, [0.; 4])?;
            cached
                .as_ref()
                .unwrap()
                .pipeline
                .render(ctx, output, uniforms, vertices)
        })
    }
}

pub(crate) fn gradient(
    paint: &crate::Gradient,
    ctx: &Dx11CustomContext,
    extent: Size<DevicePixels>,
) -> Result<Box<dyn std::any::Any>> {
    let params = crate::effect_params::gradient(paint, extent);
    let (output, generated) =
        paint
            .dx11
            .render(ctx, extent, include_str!("wgpu/gradient.wgsl"), &params, 6)?;
    if generated {
        paint.preparations.fetch_add(1, Ordering::Relaxed);
    }
    Ok(Box::new(output))
}

pub(crate) fn mesh(
    paint: &crate::MeshGradient,
    ctx: &Dx11CustomContext,
    extent: Size<DevicePixels>,
) -> Result<Box<dyn std::any::Any>> {
    let params = crate::effect_params::mesh(paint);
    let (output, generated) = paint.dx11.render(
        ctx,
        extent,
        include_str!("wgpu/mesh.wgsl"),
        &params,
        paint.stats().triangles * 3,
    )?;
    if generated {
        paint.preparations.fetch_add(1, Ordering::Relaxed);
    }
    Ok(Box::new(output))
}
