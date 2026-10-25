use super::*;
use gpui::{LayerCacheStats, LayerPlan};
struct Slot {
    size: [u32; 2],
    // Saved destination, raw subtree, horizontal scratch, filtered subtree.
    textures: Vec<Texture>,
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
        device: &DeviceRef,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<()> {
        self.plans = scene.layer_plans(size)?;
        self.hits.clear();
        for (i, p) in self.plans.iter().enumerate() {
            if self.slots.get(i).is_none_or(|s| s.size != p.size) {
                let slot = Slot {
                    size: p.size,
                    textures: Vec::new(),
                    retained: None,
                };
                if i == self.slots.len() {
                    self.slots.push(slot)
                } else {
                    self.slots[i] = slot
                }
            }
            let slot = &mut self.slots[i];
            let count = if p.blur_radius > 0. { 4 } else { 2 };
            if slot.textures.len() != count {
                // Keep raw pixels; recreated blur targets need fresh filtering.
                if let Some(old) = &mut slot.retained {
                    old.blur_radius = 0.;
                }
            }
            slot.textures.truncate(count);
            while slot.textures.len() < count {
                let d = TextureDescriptor::new();
                d.set_texture_type(MTLTextureType::D2);
                d.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                d.set_width(p.size[0] as u64);
                d.set_height(p.size[1] as u64);
                d.set_storage_mode(MTLStorageMode::Private);
                d.set_usage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
                slot.textures.push(device.new_texture(&d));
            }
            self.hits.push(
                self.slots[i]
                    .retained
                    .as_ref()
                    .is_some_and(|old| p.matches(old)),
            );
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
    pub fn copy(&self, cb: &CommandBufferRef, source: &TextureRef, slot: usize, begin: bool) {
        let p = &self.plans[slot];
        let blit = cb.new_blit_command_encoder();
        blit.copy_from_texture(
            source,
            0,
            0,
            MTLOrigin {
                x: p.origin[0] as u64,
                y: p.origin[1] as u64,
                z: 0,
            },
            MTLSize {
                width: p.size[0] as u64,
                height: p.size[1] as u64,
                depth: 1,
            },
            &self.slots[slot].textures[usize::from(!begin)],
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
        );
        blit.end_encoding();
    }
    pub fn draw(
        &self,
        e: &RenderCommandEncoderRef,
        pipeline: &RenderPipelineStateRef,
        slot: usize,
        begin: bool,
    ) {
        let params = &self.plans[slot].parameters[usize::from(!begin)];
        e.set_render_pipeline_state(pipeline);
        e.set_vertex_bytes(1, 96, params.as_ptr().cast());
        e.set_fragment_bytes(1, 96, params.as_ptr().cast());
        e.set_fragment_texture(
            0,
            Some(
                &self.slots[slot].textures[if !begin && self.plans[slot].blur_radius > 0. {
                    3
                } else {
                    1
                }],
            ),
        );
        e.set_fragment_texture(1, Some(&self.slots[slot].textures[0]));
        e.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
    }
    pub fn filter(&self, cb: &CommandBufferRef, pipeline: &RenderPipelineStateRef, slot: usize) {
        let p = &self.plans[slot];
        if !p.needs_blur(self.slots[slot].retained.as_ref()) {
            return;
        }
        for axis in 0..2 {
            let e = new_command_encoder_for_texture(
                cb,
                &self.slots[slot].textures[axis + 2],
                size(
                    DevicePixels(p.size[0] as i32),
                    DevicePixels(p.size[1] as i32),
                ),
                Some(MTLClearColor::new(0., 0., 0., 0.)),
            );
            let params = &p.parameters[axis + 2];
            e.set_render_pipeline_state(pipeline);
            e.set_vertex_bytes(1, 96, params.as_ptr().cast());
            e.set_fragment_bytes(1, 96, params.as_ptr().cast());
            e.set_fragment_texture(0, Some(&self.slots[slot].textures[axis + 1]));
            e.set_fragment_texture(1, Some(&self.slots[slot].textures[0]));
            e.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
            e.end_encoding();
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
