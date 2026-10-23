//! Backend-owned preparation and composition of independent custom paint sources.
use anyhow::{Result, ensure};
use gpui::{CustomDraw, DevicePixels, Size, size};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use wgpu::util::DeviceExt;

/// Host-owned recording context. Producers record work; only the renderer submits.
pub struct WgpuCustomContext {
    /// Device on which producer pipelines and buffers must be created.
    pub device: Arc<wgpu::Device>,
    key: usize,
    queue: Arc<wgpu::Queue>,
    encoder: Mutex<Option<wgpu::CommandEncoder>>,
    recording: Arc<AtomicU8>,
}

impl WgpuCustomContext {
    pub(crate) fn new(device: Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> Self {
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("custom_preparation"),
        });
        Self {
            device,
            key: Arc::as_ptr(queue) as usize,
            queue: queue.clone(),
            encoder: Mutex::new(Some(encoder)),
            recording: Arc::new(AtomicU8::new(0)),
        }
    }
    /// Queue-scoped cache identity; cached output retains the device and queue.
    pub fn cache_key(&self) -> usize {
        self.key
    }
    /// Allocate a host-device output with straight-alpha encoded-sRGB RGBA8 channels.
    pub fn create_output(&self, extent: Size<DevicePixels>) -> Result<WgpuCustomOutput> {
        let texture = new_texture(&self.device, extent, wgpu::TextureFormat::Rgba8Unorm)?;
        Ok(WgpuCustomOutput {
            texture,
            key: self.key,
            _queue: self.queue.clone(),
            recording: self.recording.clone(),
        })
    }
    /// Record preparation commands. Do not submit work or wait on the GPU here.
    pub fn record<T>(
        &self,
        encode: impl FnOnce(&mut wgpu::CommandEncoder) -> Result<T>,
    ) -> Result<T> {
        let mut encoder = self
            .encoder
            .lock()
            .map_err(|_| anyhow::anyhow!("custom encoder poisoned"))?;
        encode(
            encoder
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("custom preparation already finished"))?,
        )
    }
    /// Whether this output is ready on this queue or pending in this recording.
    pub fn can_reuse(&self, output: &WgpuCustomOutput) -> bool {
        output.key == self.key
            && (output.recording.load(Ordering::Acquire) == 1
                || (Arc::ptr_eq(&output.recording, &self.recording)
                    && output.recording.load(Ordering::Acquire) == 0))
    }
    pub(crate) fn submit(&self) -> Result<()> {
        let command = self
            .encoder
            .lock()
            .map_err(|_| anyhow::anyhow!("custom encoder poisoned"))?
            .take()
            .ok_or_else(|| anyhow::anyhow!("custom preparation already finished"))?
            .finish();
        self.queue.submit([command]);
        self.recording.store(1, Ordering::Release);
        Ok(())
    }
}

impl Drop for WgpuCustomContext {
    fn drop(&mut self) {
        let _ = self
            .recording
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// Cacheable immutable producer output. Construct through the host context.
#[derive(Clone, Debug)]
pub struct WgpuCustomOutput {
    texture: wgpu::Texture,
    key: usize,
    _queue: Arc<wgpu::Queue>,
    recording: Arc<AtomicU8>,
}
impl WgpuCustomOutput {
    /// Texture to render into during preparation; immutable after host submission.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub(crate) fn validate(
        self,
        context: &WgpuCustomContext,
        extent: Size<DevicePixels>,
    ) -> Result<Self> {
        ensure!(
            context.can_reuse(&self),
            "custom output belongs to another queue or abandoned recording"
        );
        ensure!(
            self.texture.width() == extent.width.0 as u32
                && self.texture.height() == extent.height.0 as u32,
            "invalid custom texture extent"
        );
        Ok(self)
    }
}

pub(crate) fn new_texture(
    device: &wgpu::Device,
    extent: Size<DevicePixels>,
    format: wgpu::TextureFormat,
) -> Result<wgpu::Texture> {
    ensure!(
        extent.width.0 > 0
            && extent.height.0 > 0
            && extent.width.0 as u32 <= device.limits().max_texture_dimension_2d
            && extent.height.0 as u32 <= device.limits().max_texture_dimension_2d
            && extent.width.0 as u64 * extent.height.0 as u64 * 4 <= 64 * 1024 * 1024,
        "invalid custom texture extent"
    );
    Ok(device.create_texture(&wgpu::TextureDescriptor {
        label: Some("custom_paint"),
        size: wgpu::Extent3d {
            width: extent.width.0 as u32,
            height: extent.height.0 as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    }))
}

pub(crate) fn extent(draw: &CustomDraw) -> Result<Size<DevicePixels>> {
    ensure!(
        draw.opacity.is_finite() && (0.0..=1.0).contains(&draw.opacity),
        "invalid custom opacity"
    );
    ensure!(
        [
            draw.bounds.origin.x.0,
            draw.bounds.origin.y.0,
            draw.bounds.size.width.0,
            draw.bounds.size.height.0,
            draw.content_mask.bounds.origin.x.0,
            draw.content_mask.bounds.origin.y.0,
            draw.content_mask.bounds.size.width.0,
            draw.content_mask.bounds.size.height.0,
            draw.corner_radii.top_left.0,
            draw.corner_radii.top_right.0,
            draw.corner_radii.bottom_right.0,
            draw.corner_radii.bottom_left.0
        ]
        .iter()
        .all(|v| v.is_finite()),
        "non-finite custom geometry"
    );
    ensure!(
        draw.bounds.size.width.0 > 0.
            && draw.bounds.size.height.0 > 0.
            && draw.bounds.size.width.0 <= 16384.
            && draw.bounds.size.height.0 <= 16384.,
        "invalid custom extent"
    );
    Ok(size(
        DevicePixels(draw.bounds.size.width.0.ceil() as i32),
        DevicePixels(draw.bounds.size.height.0.ceil() as i32),
    ))
}

pub(crate) struct PreparedDraw {
    pub bind_group: wgpu::BindGroup,
}
pub(crate) struct CustomCompositor {
    pub pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pub masks: std::collections::HashMap<Vec<u32>, wgpu::Texture>,
    pub mask_bytes: usize,
}
impl CustomCompositor {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("custom_compositor"),
            source: wgpu::ShaderSource::Wgsl(include_str!("custom_draw.wgsl").into()),
        });
        let entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ];
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("custom_paint"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("custom_paint"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("custom_paint"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("custom_vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("custom_fragment"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("custom_paint"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            masks: Default::default(),
            mask_bytes: 0,
        }
    }
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        draw: &CustomDraw,
        source: &wgpu::Texture,
        mask: Option<&wgpu::Texture>,
        native: bool,
        viewport: Size<DevicePixels>,
    ) -> PreparedDraw {
        let params: [[f32; 4]; 4] = [
            [
                draw.bounds.origin.x.0,
                draw.bounds.origin.y.0,
                draw.bounds.size.width.0,
                draw.bounds.size.height.0,
            ],
            [
                draw.content_mask.bounds.origin.x.0,
                draw.content_mask.bounds.origin.y.0,
                draw.content_mask.bounds.size.width.0,
                draw.content_mask.bounds.size.height.0,
            ],
            [
                draw.corner_radii.top_left.0,
                draw.corner_radii.top_right.0,
                draw.corner_radii.bottom_right.0,
                draw.corner_radii.bottom_left.0,
            ],
            [
                viewport.width.0 as f32,
                viewport.height.0 as f32,
                draw.opacity,
                u32::from(mask.is_some()) as f32 + if native { 2. } else { 0. },
            ],
        ];
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("custom_params"),
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let source = source.create_view(&Default::default());
        let mask = mask
            .map(|t| t.create_view(&Default::default()))
            .unwrap_or_else(|| source.clone());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("custom_paint"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&source),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&mask),
                },
            ],
        });
        PreparedDraw { bind_group }
    }
}

/// Exact normalized geometry key: clipping, screen origin and producer are excluded.
pub(crate) fn coverage_key(draw: &CustomDraw) -> Vec<u32> {
    let mut key = vec![
        draw.bounds.size.width.0.to_bits(),
        draw.bounds.size.height.0.to_bits(),
    ];
    for r in [
        draw.corner_radii.top_left,
        draw.corner_radii.top_right,
        draw.corner_radii.bottom_right,
        draw.corner_radii.bottom_left,
    ] {
        key.push(r.0.to_bits());
    }
    match draw.coverage.as_ref().expect("coverage") {
        gpui::CustomCoverage::Border { widths, style } => {
            key.extend([0, *style as u32]);
            for w in [widths.top, widths.right, widths.bottom, widths.left] {
                key.push(w.0.to_bits());
            }
        }
        gpui::CustomCoverage::Path(path) => {
            key.push(1);
            for v in &path.vertices {
                key.extend([
                    (v.xy_position.x.0 - draw.bounds.origin.x.0).to_bits(),
                    (v.xy_position.y.0 - draw.bounds.origin.y.0).to_bits(),
                    v.st_position.x.to_bits(),
                    v.st_position.y.to_bits(),
                ]);
            }
        }
    }
    key
}

pub(crate) fn coverage_scene(draw: &CustomDraw, extent: Size<DevicePixels>) -> Result<gpui::Scene> {
    use gpui::*;
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(
            ScaledPixels(extent.width.0 as f32),
            ScaledPixels(extent.height.0 as f32),
        ),
    );
    let mask = ContentMask { bounds };
    match draw.coverage.as_ref().expect("coverage") {
        CustomCoverage::Border { widths, style } => {
            ensure!(
                [widths.top.0, widths.right.0, widths.bottom.0, widths.left.0]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.),
                "invalid border widths"
            );
            scene.insert_primitive(Quad {
                bounds,
                content_mask: mask,
                background: transparent_black().into(),
                border_color: white(),
                border_widths: *widths,
                corner_radii: draw.corner_radii,
                border_style: *style,
                ..Default::default()
            });
        }
        CustomCoverage::Path(path) => {
            ensure!(path.vertices.len() <= 1_000_000, "coverage path too large");
            let mut path = path.as_ref().clone();
            let sx = bounds.size.width.0 / draw.bounds.size.width.0;
            let sy = bounds.size.height.0 / draw.bounds.size.height.0;
            for vertex in &mut path.vertices {
                vertex.xy_position.x =
                    ScaledPixels((vertex.xy_position.x.0 - draw.bounds.origin.x.0) * sx);
                vertex.xy_position.y =
                    ScaledPixels((vertex.xy_position.y.0 - draw.bounds.origin.y.0) * sy);
                ensure!(
                    [
                        vertex.xy_position.x.0,
                        vertex.xy_position.y.0,
                        vertex.st_position.x,
                        vertex.st_position.y
                    ]
                    .iter()
                    .all(|v| v.is_finite()),
                    "non-finite coverage path"
                );
                vertex.content_mask = mask;
            }
            path.bounds = bounds;
            path.content_mask = mask;
            path.color = white().into();
            scene.insert_primitive(path);
        }
    }
    scene.finish();
    Ok(scene)
}

#[cfg(test)]
mod tests;
