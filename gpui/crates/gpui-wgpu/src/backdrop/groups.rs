use super::*;
struct Slot {
    size: [u32; 2],
    textures: [Texture; 2],
    bind_group: BindGroup,
    retained: Option<LayerPlan>,
}
#[derive(Default)]
pub(crate) struct Groups {
    slots: Vec<Slot>,
    plans: Vec<LayerPlan>,
    hits: Vec<bool>,
    pub stats: LayerCacheStats,
}
impl Groups {
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        layout: &BindGroupLayout,
        uniforms: &Buffer,
        stride: u64,
        format: TextureFormat,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<()> {
        self.plans = scene.layer_plans(size)?;
        self.hits.clear();
        for (i, plan) in self.plans.iter().enumerate() {
            if self.slots.get(i).is_none_or(|s| s.size != plan.size) {
                let textures = std::array::from_fn(|_| {
                    device.create_texture(&TextureDescriptor {
                        label: Some("captured layer"),
                        size: Extent3d {
                            width: plan.size[0],
                            height: plan.size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format,
                        usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                });
                let views: [TextureView; 2] =
                    std::array::from_fn(|j| textures[j].create_view(&Default::default()));
                let bind_group = device.create_bind_group(&BindGroupDescriptor {
                    label: Some("captured layer"),
                    layout,
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::Buffer(BufferBinding {
                                buffer: uniforms,
                                offset: 0,
                                size: BufferSize::new(96),
                            }),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(&views[1]),
                        },
                        BindGroupEntry {
                            binding: 3,
                            resource: BindingResource::TextureView(&views[0]),
                        },
                    ],
                });
                let slot = Slot {
                    size: plan.size,
                    textures,
                    bind_group,
                    retained: None,
                };
                if i == self.slots.len() {
                    self.slots.push(slot)
                } else {
                    self.slots[i] = slot
                }
            }
            self.hits.push(
                self.slots[i]
                    .retained
                    .as_ref()
                    .is_some_and(|old| plan.matches(old)),
            );
            for pass in 0..2 {
                queue.write_buffer(
                    uniforms,
                    (12 + i * 2 + pass) as u64 * stride,
                    bytemuck::cast_slice(&plan.parameters[pass]),
                );
            }
        }
        self.slots.truncate(self.plans.len());
        Ok(())
    }
    pub fn boundary(&self, index: usize) -> (usize, bool) {
        self.plans
            .iter()
            .enumerate()
            .find_map(|(slot, p)| {
                if p.begin_index == index {
                    Some((slot, true))
                } else if p.end_index == index {
                    Some((slot, false))
                } else {
                    None
                }
            })
            .expect("validated layer boundary")
    }
    pub fn start(&mut self, slot: usize) {
        if !self.hits[slot] {
            self.slots[slot].retained = None;
        }
    }
    pub fn hit(&self, slot: usize) -> bool {
        self.hits[slot]
    }
    pub fn end(&self, slot: usize) -> usize {
        self.plans[slot].end_index
    }
    pub fn copy(&self, encoder: &mut CommandEncoder, source: &Texture, slot: usize, begin: bool) {
        let p = &self.plans[slot];
        encoder.copy_texture_to_texture(
            TexelCopyTextureInfo {
                texture: source,
                mip_level: 0,
                origin: Origin3d {
                    x: p.origin[0],
                    y: p.origin[1],
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            self.slots[slot].textures[usize::from(!begin)].as_image_copy(),
            Extent3d {
                width: p.size[0],
                height: p.size[1],
                depth_or_array_layers: 1,
            },
        );
    }
    pub fn draw(
        &self,
        pass: &mut RenderPass<'_>,
        pipeline: &RenderPipeline,
        stride: u64,
        slot: usize,
        begin: bool,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(
            0,
            &self.slots[slot].bind_group,
            &[((12 + slot * 2 + usize::from(!begin)) as u64 * stride) as u32],
        );
        pass.draw(0..6, 0..1);
    }
    pub fn commit(&mut self, executed: &[usize]) {
        for &slot in executed {
            if self.hits[slot] {
                self.stats.hits += 1;
            } else {
                self.stats.captures += 1;
                self.slots[slot].retained = Some(self.plans[slot].clone());
            }
        }
    }
}
