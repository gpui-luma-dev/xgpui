//! Bounded framebuffer-copy and separable-filter passes.
use anyhow::{Result, ensure};
use gpui::*;
use wgpu::*;
mod groups;

struct Slot {
    size: [u32; 2],
    textures: [Texture; 3],
    views: [TextureView; 3],
    groups: [BindGroup; 3],
}
pub(crate) struct Backdrops {
    layout: BindGroupLayout,
    uniforms: Buffer,
    filter: RenderPipeline,
    slots: Vec<Slot>,
    pub plans: Vec<BackdropPlan>,
    pub format: TextureFormat,
    program: u64,
    stride: u64,
    pub groups: groups::Groups,
    pub cache: gpui::BackdropCache,
}
impl Backdrops {
    pub fn new(device: &Device, format: TextureFormat, program: &SceneShaderProgram) -> Self {
        let stride = u64::from(device.limits().min_uniform_buffer_offset_alignment)
            .max(96)
            .next_power_of_two();
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("backdrop uniforms"),
            size: stride * 28,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("backdrop"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: BufferSize::new(96),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("backdrop"),
            source: ShaderSource::Wgsl(program.wgsl().into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("backdrop"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |blend| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("backdrop"),
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: &shader,
                    entry_point: Some("backdrop_vertex"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(FragmentState {
                    module: &shader,
                    entry_point: Some("backdrop_fragment"),
                    targets: &[Some(ColorTargetState {
                        format,
                        blend,
                        write_mask: ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let filter = pipeline(None);
        Self {
            layout,
            uniforms,
            filter,
            slots: Vec::new(),
            plans: Vec::new(),
            format,
            program: program.id(),
            stride,
            groups: Default::default(),
            cache: Default::default(),
        }
    }
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        scene: &Scene,
        size: Size<DevicePixels>,
        context: &[u64],
    ) -> Result<()> {
        ensure!(
            scene.backdrops.len() <= MAX_BACKDROPS,
            "at most four backdrop boundaries per frame"
        );
        anyhow::ensure!(
            scene
                .layer_boundaries
                .iter()
                .all(|b| b.capture.program.id() == self.program),
            "layer capture program mismatch"
        );
        self.plans = scene
            .backdrops
            .iter()
            .map(|d| {
                ensure!(
                    d.program.id() == self.program,
                    "backdrop programs must match"
                );
                d.plan(size)
            })
            .collect::<Result<_>>()?;
        for (i, plan) in self.plans.iter().enumerate() {
            if self.slots.get(i).is_none_or(|s| s.size != plan.size) {
                self.cache.invalidate(i);
                let textures = std::array::from_fn(|pass| {
                    let size = if pass == 0 { plan.size } else { plan.reduced };
                    device.create_texture(&TextureDescriptor {
                        label: Some("backdrop working texture"),
                        size: Extent3d {
                            width: size[0],
                            height: size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: self.format,
                        usage: TextureUsages::COPY_DST
                            | TextureUsages::TEXTURE_BINDING
                            | TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                });
                let views: [TextureView; 3] =
                    std::array::from_fn(|j| textures[j].create_view(&Default::default()));
                let groups = std::array::from_fn(|j| {
                    device.create_bind_group(&BindGroupDescriptor {
                        label: Some("backdrop"),
                        layout: &self.layout,
                        entries: &[
                            BindGroupEntry {
                                binding: 0,
                                resource: BindingResource::Buffer(BufferBinding {
                                    buffer: &self.uniforms,
                                    offset: 0,
                                    size: BufferSize::new(96),
                                }),
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource: BindingResource::TextureView(&views[j]),
                            },
                            BindGroupEntry {
                                binding: 3,
                                resource: BindingResource::TextureView(&views[0]),
                            },
                        ],
                    })
                });
                let slot = Slot {
                    size: plan.size,
                    textures,
                    views,
                    groups,
                };
                if i == self.slots.len() {
                    self.slots.push(slot)
                } else {
                    self.slots[i] = slot
                }
            }
            for j in 0..3 {
                queue.write_buffer(
                    &self.uniforms,
                    (i * 3 + j) as u64 * self.stride,
                    bytemuck::cast_slice(&plan.parameters[j]),
                );
            }
        }
        self.slots.truncate(self.plans.len());
        self.cache
            .prepare(scene.backdrop_cache_keys(size)?, context);
        self.groups.prepare(
            device,
            queue,
            &self.layout,
            &self.uniforms,
            self.stride,
            self.format,
            scene,
            size,
        )?;
        Ok(())
    }
    pub fn group_filter(&self, encoder: &mut CommandEncoder, slot: usize) {
        self.groups.filter(encoder, &self.filter, self.stride, slot);
    }
    pub fn group_draw(&self, pass: &mut RenderPass<'_>, slot: usize, begin: bool) {
        self.groups
            .draw(pass, &self.filter, self.stride, slot, begin)
    }
    pub fn filter(&mut self, encoder: &mut CommandEncoder, source: &Texture, index: usize) {
        self.cache.start(index);
        if self.cache.hit(index) {
            return;
        }
        let slot = &self.slots[index];
        let plan = &self.plans[index];
        encoder.copy_texture_to_texture(
            TexelCopyTextureInfo {
                texture: source,
                mip_level: 0,
                origin: Origin3d {
                    x: plan.origin[0],
                    y: plan.origin[1],
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            slot.textures[0].as_image_copy(),
            Extent3d {
                width: plan.size[0],
                height: plan.size[1],
                depth_or_array_layers: 1,
            },
        );
        for j in 0..2 {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("backdrop filter"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &slot.views[j + 1],
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.filter);
            pass.set_bind_group(
                0,
                &slot.groups[j],
                &[((index * 3 + j) as u64 * self.stride) as u32],
            );
            pass.draw(0..6, 0..1);
        }
    }
    pub fn composite(&self, pass: &mut RenderPass<'_>, index: usize) {
        pass.set_pipeline(&self.filter);
        pass.set_bind_group(
            0,
            &self.slots[index].groups[2],
            &[((index * 3 + 2) as u64 * self.stride) as u32],
        );
        pass.draw(0..6, 0..1);
    }
}
