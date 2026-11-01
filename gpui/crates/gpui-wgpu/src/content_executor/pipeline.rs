use super::*;
use std::num::NonZeroU64;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct PipelineKey {
    program: c::PortableProgram,
    target: Option<(c::PixelFormat, c::PassBlend)>,
    vertices: Vec<c::VertexLayout>,
}
impl PipelineKey {
    pub fn of(node: &c::ContentNode) -> Option<Self> {
        match node.operation() {
            c::ContentOperation::Render(p) => {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                Some(Self {
                    program: p.program().clone(),
                    target: Some((d.format(), p.blend())),
                    vertices: match p.geometry() {
                        c::ResolvedGeometry::Mesh { vertices, .. } => {
                            vertices.iter().map(|v| v.layout.clone()).collect()
                        }
                        _ => Vec::new(),
                    },
                })
            }
            c::ContentOperation::Compute(p) => Some(Self {
                program: p.program().clone(),
                target: None,
                vertices: Vec::new(),
            }),
            _ => None,
        }
    }
}
pub(super) enum PipelineState {
    Render(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
}
pub(super) struct Pipeline {
    pub state: PipelineState,
    pub layouts: Vec<wgpu::BindGroupLayout>,
    pub program: c::PortableProgram,
}
pub(super) fn format(format: c::PixelFormat) -> wgpu::TextureFormat {
    match format {
        c::PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
        c::PixelFormat::Bgra8 => wgpu::TextureFormat::Bgra8Unorm,
        c::PixelFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        c::PixelFormat::R8 => wgpu::TextureFormat::R8Unorm,
    }
}
fn visibility(stages: c::ProgramStages) -> wgpu::ShaderStages {
    let mut result = wgpu::ShaderStages::empty();
    if stages.contains(c::ProgramStages::VERTEX) {
        result |= wgpu::ShaderStages::VERTEX;
    }
    if stages.contains(c::ProgramStages::FRAGMENT) {
        result |= wgpu::ShaderStages::FRAGMENT;
    }
    if stages.contains(c::ProgramStages::COMPUTE) {
        result |= wgpu::ShaderStages::COMPUTE;
    }
    result
}
fn vertex_format(ty: c::ShaderValueType) -> wgpu::VertexFormat {
    match ty {
        c::ShaderValueType::Float(1) => wgpu::VertexFormat::Float32,
        c::ShaderValueType::Float(2) => wgpu::VertexFormat::Float32x2,
        c::ShaderValueType::Float(3) => wgpu::VertexFormat::Float32x3,
        c::ShaderValueType::Float(4) => wgpu::VertexFormat::Float32x4,
        c::ShaderValueType::Sint(1) => wgpu::VertexFormat::Sint32,
        c::ShaderValueType::Sint(2) => wgpu::VertexFormat::Sint32x2,
        c::ShaderValueType::Sint(3) => wgpu::VertexFormat::Sint32x3,
        c::ShaderValueType::Sint(4) => wgpu::VertexFormat::Sint32x4,
        c::ShaderValueType::Uint(1) => wgpu::VertexFormat::Uint32,
        c::ShaderValueType::Uint(2) => wgpu::VertexFormat::Uint32x2,
        c::ShaderValueType::Uint(3) => wgpu::VertexFormat::Uint32x3,
        c::ShaderValueType::Uint(4) => wgpu::VertexFormat::Uint32x4,
        _ => unreachable!(),
    }
}
impl Worker {
    pub(super) fn pipeline(
        &mut self,
        key: PipelineKey,
        stats: &mut WgpuContentStats,
    ) -> Result<Arc<Pipeline>> {
        if let Some(pipeline) = self.pipelines.get(&key) {
            return Ok(pipeline.clone());
        }
        let program = &key.program;
        let count = program
            .bindings()
            .iter()
            .map(|b| b.address().group + 1)
            .max()
            .unwrap_or(0);
        let mut layouts = Vec::new();
        for group in 0..count {
            let mut entries = Vec::new();
            for binding in program
                .bindings()
                .iter()
                .filter(|b| b.address().group == group)
            {
                let ty = match binding.kind() {
                    c::ProgramBindingKind::Uniform(layout) => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(u64::from(layout.byte_len())),
                    },
                    c::ProgramBindingKind::StorageBuffer { layout, access } => {
                        wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage {
                                read_only: *access == c::StorageAccess::ReadOnly,
                            },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(u64::from(layout.minimum_bytes())),
                        }
                    }
                    c::ProgramBindingKind::SampledImage(_) => wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    c::ProgramBindingKind::Sampler => {
                        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
                    }
                    c::ProgramBindingKind::StorageImage {
                        format: pixel,
                        access,
                        ..
                    } => {
                        if *access != c::StorageAccess::WriteOnly
                            && !self
                                .device
                                .features()
                                .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
                        {
                            return Err(unsupported(
                                "readonly/read-write storage textures require an enabled wgpu device capability",
                            ));
                        }
                        wgpu::BindingType::StorageTexture {
                            access: match access {
                                c::StorageAccess::ReadOnly => wgpu::StorageTextureAccess::ReadOnly,
                                c::StorageAccess::WriteOnly => {
                                    wgpu::StorageTextureAccess::WriteOnly
                                }
                                c::StorageAccess::ReadWrite => {
                                    wgpu::StorageTextureAccess::ReadWrite
                                }
                            },
                            format: format(*pixel),
                            view_dimension: wgpu::TextureViewDimension::D2,
                        }
                    }
                };
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: binding.address().binding,
                    visibility: visibility(binding.stages()),
                    ty,
                    count: None,
                });
            }
            layouts.push(
                self.device
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("portable content bindings"),
                        entries: &entries,
                    }),
            );
        }
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("portable content pipeline layout"),
                bind_group_layouts: &layouts.iter().map(Some).collect::<Vec<_>>(),
                immediate_size: 0,
            });
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("portable content WGSL"),
                source: wgpu::ShaderSource::Wgsl(program.wgsl().into()),
            });
        let state = if let Some((pixel, blend)) = key.target {
            let (vertex, fragment) = program.render_entries().unwrap();
            let attributes: Vec<Vec<_>> = key
                .vertices
                .iter()
                .map(|v| {
                    v.attributes
                        .iter()
                        .map(|a| wgpu::VertexAttribute {
                            format: vertex_format(a.ty),
                            offset: u64::from(a.offset),
                            shader_location: a.location,
                        })
                        .collect()
                })
                .collect();
            let buffers: Vec<_> = key
                .vertices
                .iter()
                .zip(&attributes)
                .map(|(v, attrs)| {
                    Some(wgpu::VertexBufferLayout {
                        array_stride: u64::from(v.stride),
                        step_mode: match v.step {
                            c::VertexStep::Vertex => wgpu::VertexStepMode::Vertex,
                            c::VertexStep::Instance => wgpu::VertexStepMode::Instance,
                        },
                        attributes: attrs,
                    })
                })
                .collect();
            PipelineState::Render(self.device.create_render_pipeline(
                &wgpu::RenderPipelineDescriptor {
                    label: Some("portable content render"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some(vertex),
                        buffers: &buffers,
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(fragment),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: format(pixel),
                            blend: match blend {
                                c::PassBlend::Replace => None,
                                c::PassBlend::SourceOver => {
                                    Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)
                                }
                            },
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                },
            ))
        } else {
            PipelineState::Compute(self.device.create_compute_pipeline(
                &wgpu::ComputePipelineDescriptor {
                    label: Some("portable content compute"),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: program.compute_entry(),
                    compilation_options: Default::default(),
                    cache: None,
                },
            ))
        };
        stats.pipeline_compilations += 1;
        if self.pipelines.len() >= self.options.pipeline_cache_entries {
            self.pipelines.clear();
        }
        let pipeline = Arc::new(Pipeline {
            state,
            layouts,
            program: key.program.clone(),
        });
        self.pipelines.insert(key, pipeline.clone());
        Ok(pipeline)
    }
}
