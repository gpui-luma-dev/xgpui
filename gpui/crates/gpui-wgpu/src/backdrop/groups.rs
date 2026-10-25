use super::*;
struct Slot {
    size: [u32; 2],
    // Saved destination, raw subtree, horizontal scratch, filtered subtree.
    textures: Vec<Texture>,
    views: Vec<TextureView>,
    bind_groups: Vec<BindGroup>,
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
    pub fn clear(&mut self) {
        self.slots.clear();
        self.plans.clear();
        self.hits.clear();
    }
    pub fn texture_count(&self) -> usize {
        self.slots.iter().map(|slot| slot.textures.len()).sum()
    }
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
                let slot = Slot {
                    size: plan.size,
                    textures: Vec::new(),
                    views: Vec::new(),
                    bind_groups: Vec::new(),
                    retained: None,
                };
                if i == self.slots.len() {
                    self.slots.push(slot)
                } else {
                    self.slots[i] = slot
                }
            }
            let slot = &mut self.slots[i];
            let count = if plan.blur_radius > 0. { 4 } else { 2 };
            if slot.textures.len() != count {
                // Allocation changes invalidate only filtered pixels. Keep the
                // immutable raw source and its retained command identities.
                if let Some(old) = &mut slot.retained {
                    old.blur_radius = 0.;
                }
            }
            slot.bind_groups.truncate(count);
            slot.views.truncate(count);
            slot.textures.truncate(count);
            while slot.textures.len() < count {
                let texture = device.create_texture(&TextureDescriptor {
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
                    usage: TextureUsages::COPY_DST
                        | TextureUsages::TEXTURE_BINDING
                        | TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                slot.views.push(texture.create_view(&Default::default()));
                slot.textures.push(texture);
                let j = slot.bind_groups.len();
                slot.bind_groups
                    .push(device.create_bind_group(&BindGroupDescriptor {
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
                                resource: BindingResource::TextureView(&slot.views[j]),
                            },
                            BindGroupEntry {
                                binding: 3,
                                resource: BindingResource::TextureView(&slot.views[0]),
                            },
                        ],
                    }));
            }
            self.hits.push(
                self.slots[i]
                    .retained
                    .as_ref()
                    .is_some_and(|old| plan.matches(old)),
            );
            for pass in 0..4 {
                queue.write_buffer(
                    uniforms,
                    (12 + i * 4 + pass) as u64 * stride,
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
            &self.slots[slot].bind_groups[if !begin && self.plans[slot].blur_radius > 0. {
                3
            } else {
                1
            }],
            &[((12 + slot * 4 + usize::from(!begin)) as u64 * stride) as u32],
        );
        pass.draw(0..6, 0..1);
    }
    pub fn filter(
        &self,
        encoder: &mut CommandEncoder,
        pipeline: &RenderPipeline,
        stride: u64,
        slot: usize,
    ) {
        let p = &self.plans[slot];
        if !p.needs_blur(self.slots[slot].retained.as_ref()) {
            return;
        }
        for axis in 0..2 {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("subtree Gaussian blur"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.slots[slot].views[axis + 2],
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(
                0,
                &self.slots[slot].bind_groups[axis + 1],
                &[((12 + slot * 4 + axis + 2) as u64 * stride) as u32],
            );
            pass.draw(0..6, 0..1);
        }
    }
    pub fn commit(&mut self, executed: &[usize]) {
        for &slot in executed {
            if self.hits[slot] {
                self.stats.hits += 1;
            } else {
                self.stats.captures += 1;
            }
            self.slots[slot].retained = Some(self.plans[slot].clone());
        }
    }
}
