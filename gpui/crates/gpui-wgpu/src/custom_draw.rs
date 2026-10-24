//! Backend-owned preparation and composition of independent custom paint sources.
mod coverage_cache;
use anyhow::{Result, ensure};
pub(crate) use coverage_cache::{CoverageCache, CoverageKey};
use gpui::{CustomDraw, DevicePixels, Size, size};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
#[cfg(test)]
use wgpu::util::DeviceExt;

const PENDING: u8 = 0;
const SUBMITTED: u8 = 1;
const ABANDONED: u8 = 2;
const STAGED: u8 = 3;

#[derive(Debug)]
struct Recording {
    state: AtomicU8,
    batch: Option<Arc<()>>,
}
impl Recording {
    fn new(batch: Option<Arc<()>>) -> Self {
        Self {
            state: AtomicU8::new(PENDING),
            batch,
        }
    }
    fn can_reuse(self: &Arc<Self>, output: &Arc<Self>) -> bool {
        match output.state.load(Ordering::Acquire) {
            SUBMITTED => true,
            PENDING => Arc::ptr_eq(output, self),
            STAGED => self
                .batch
                .as_ref()
                .zip(output.batch.as_ref())
                .is_some_and(|(a, b)| Arc::ptr_eq(a, b)),
            _ => false,
        }
    }
    fn abandon(&self, from: u8) {
        let _ = self
            .state
            .compare_exchange(from, ABANDONED, Ordering::AcqRel, Ordering::Acquire);
    }
}

struct EncoderState {
    encoder: Option<wgpu::CommandEncoder>,
    finished: bool,
    failed: bool,
}

/// A batch owns accepted recordings until all paint preparation succeeds.
/// Dropping an unfinished batch invalidates its staged outputs without submitting.
pub(crate) struct PreparationBatch {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    id: Arc<()>,
    pending: Vec<(Option<wgpu::CommandBuffer>, Arc<Recording>)>,
}
impl PreparationBatch {
    pub fn new(device: Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> Self {
        Self {
            device,
            queue: queue.clone(),
            id: Arc::new(()),
            pending: Vec::new(),
        }
    }
    /// Run producer preparation and host output validation before staging its work.
    pub fn prepare<T>(
        &mut self,
        prepare: impl FnOnce(&WgpuCustomContext) -> Result<T>,
    ) -> Result<T> {
        let context =
            WgpuCustomContext::with_batch(self.device.clone(), &self.queue, Some(self.id.clone()));
        let output = prepare(&context)?;
        let command = context.finish()?;
        context.recording.state.store(STAGED, Ordering::Release);
        self.pending.push((command, context.recording.clone()));
        Ok(output)
    }
    pub fn submit(mut self) -> Result<()> {
        if self.pending.iter().any(|(command, _)| command.is_some()) {
            self.queue.submit(
                self.pending
                    .iter_mut()
                    .filter_map(|(command, _)| command.take()),
            );
        }
        for (_, recording) in &self.pending {
            recording.state.store(SUBMITTED, Ordering::Release);
        }
        Ok(())
    }
}
impl Drop for PreparationBatch {
    fn drop(&mut self) {
        for (_, recording) in &self.pending {
            recording.abandon(STAGED);
        }
    }
}

/// Host-owned recording context. Producers record work; only the renderer submits.
pub struct WgpuCustomContext {
    /// Device on which producer pipelines and buffers must be created.
    pub device: Arc<wgpu::Device>,
    key: usize,
    queue: Arc<wgpu::Queue>,
    encoder: Mutex<EncoderState>,
    recording: Arc<Recording>,
}

impl WgpuCustomContext {
    #[cfg(test)]
    pub(crate) fn new(device: Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> Self {
        Self::with_batch(device, queue, None)
    }
    fn with_batch(
        device: Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        batch: Option<Arc<()>>,
    ) -> Self {
        Self {
            device,
            key: Arc::as_ptr(queue) as usize,
            queue: queue.clone(),
            encoder: Mutex::new(EncoderState {
                encoder: None,
                finished: false,
                failed: false,
            }),
            recording: Arc::new(Recording::new(batch)),
        }
    }
    /// Queue-scoped cache identity; cached output retains the device and queue.
    pub fn cache_key(&self) -> usize {
        self.key
    }
    /// Allocate a host-device output with straight-alpha encoded-sRGB RGBA8 channels.
    pub fn create_output(&self, extent: Size<DevicePixels>) -> Result<WgpuCustomOutput> {
        ensure!(
            self.recording.state.load(Ordering::Acquire) == PENDING,
            "custom preparation is no longer writable"
        );
        let texture = new_texture(&self.device, extent, wgpu::TextureFormat::Rgba8Unorm)?;
        Ok(WgpuCustomOutput {
            texture,
            key: self.key,
            _queue: self.queue.clone(),
            recording: self.recording.clone(),
        })
    }
    /// Record preparation commands. Do not submit work or wait on the GPU here.
    /// An encoding error invalidates this transaction, including commands recorded earlier.
    pub fn record<T>(
        &self,
        encode: impl FnOnce(&mut wgpu::CommandEncoder) -> Result<T>,
    ) -> Result<T> {
        let mut state = self
            .encoder
            .lock()
            .map_err(|_| anyhow::anyhow!("custom encoder poisoned"))?;
        ensure!(
            !state.finished && !state.failed,
            "custom preparation already finished or failed"
        );
        let encoder = state.encoder.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("custom_preparation"),
                })
        });
        let result = encode(encoder);
        if result.is_err() {
            state.failed = true;
            state.encoder.take();
            self.recording.abandon(PENDING);
        }
        result
    }
    /// Whether output is submitted, local to this recording, or staged earlier in this host batch.
    pub fn can_reuse(&self, output: &WgpuCustomOutput) -> bool {
        output.key == self.key && self.recording.can_reuse(&output.recording)
    }
    fn finish(&self) -> Result<Option<wgpu::CommandBuffer>> {
        let mut state = self
            .encoder
            .lock()
            .map_err(|_| anyhow::anyhow!("custom encoder poisoned"))?;
        ensure!(
            !state.finished && !state.failed,
            "custom preparation already finished or failed"
        );
        state.finished = true;
        Ok(state.encoder.take().map(|encoder| encoder.finish()))
    }
    #[cfg(test)]
    pub(crate) fn submit(&self) -> Result<()> {
        if let Some(command) = self.finish()? {
            self.queue.submit([command]);
        }
        self.recording.state.store(SUBMITTED, Ordering::Release);
        Ok(())
    }
}

impl Drop for WgpuCustomContext {
    fn drop(&mut self) {
        self.recording.abandon(PENDING);
    }
}

/// Cacheable immutable producer output. Construct through the host context.
#[derive(Clone, Debug)]
pub struct WgpuCustomOutput {
    texture: wgpu::Texture,
    key: usize,
    _queue: Arc<wgpu::Queue>,
    recording: Arc<Recording>,
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
    pub uniform_offset: u32,
}

const UNIFORM_BYTES: u64 = 64;
/// Renderer-private arena. Reuse only after previous render commands are submitted;
/// queue-ordered uploads then preserve reads by still-running previous frames.
struct UniformArena {
    buffer: Option<wgpu::Buffer>,
    bytes: Vec<u8>,
    stride: u64,
    capacity: u64,
    cursor: usize,
    #[cfg(test)]
    creations: usize,
}
impl UniformArena {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: None,
            bytes: Vec::new(),
            stride: UNIFORM_BYTES
                .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as u64),
            capacity: 0,
            cursor: 0,
            #[cfg(test)]
            creations: 0,
        }
    }
    fn begin(&mut self, device: &wgpu::Device, count: usize) -> Result<()> {
        let required = (count as u64)
            .checked_mul(self.stride)
            .ok_or_else(|| anyhow::anyhow!("custom uniform size overflow"))?;
        let limit = device.limits().max_buffer_size.min(u32::MAX as u64);
        ensure!(
            required <= limit,
            "custom uniforms exceed buffer/offset limits"
        );
        if required > self.capacity {
            let capacity = required
                .checked_next_power_of_two()
                .unwrap_or(required)
                .min(limit);
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("custom_params_arena"),
                size: capacity,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = capacity;
            #[cfg(test)]
            {
                self.creations += 1;
            }
        }
        self.bytes.resize(required as usize, 0);
        self.cursor = 0;
        Ok(())
    }
    fn write(&mut self, params: &[[f32; 4]; 4]) -> u32 {
        let offset = self.cursor * self.stride as usize;
        self.cursor += 1;
        self.bytes[offset..offset + UNIFORM_BYTES as usize]
            .copy_from_slice(bytemuck::cast_slice(params));
        offset as u32
    }
    fn upload(&self, queue: &wgpu::Queue) {
        debug_assert_eq!(self.cursor as u64 * self.stride, self.bytes.len() as u64);
        if !self.bytes.is_empty() {
            queue.write_buffer(self.buffer.as_ref().expect("uniform arena"), 0, &self.bytes);
        }
    }
}
pub(crate) struct CustomCompositor {
    pub pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: UniformArena,
    pub masks: CoverageCache<wgpu::Texture>,
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
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
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
            uniforms: UniformArena::new(device),
            masks: Default::default(),
            mask_bytes: 0,
        }
    }
    pub(crate) fn begin_uniforms(&mut self, device: &wgpu::Device, count: usize) -> Result<()> {
        self.uniforms.begin(device, count)
    }
    pub(crate) fn upload_uniforms(&self, queue: &wgpu::Queue) {
        self.uniforms.upload(queue);
    }
    pub fn prepare(
        &mut self,
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
        let uniform_offset = self.uniforms.write(&params);
        let uniform = self.uniforms.buffer.as_ref().expect("uniform arena");
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
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniform,
                        offset: 0,
                        size: wgpu::BufferSize::new(UNIFORM_BYTES),
                    }),
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
        PreparedDraw {
            bind_group,
            uniform_offset,
        }
    }
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
