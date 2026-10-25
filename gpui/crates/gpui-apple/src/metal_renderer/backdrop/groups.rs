use super::*;
use gpui::{LayerCacheStats, LayerPlan};
struct Slot {
    size: [u32; 2],
    textures: [Texture; 2],
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
        device: &DeviceRef,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<()> {
        self.plans = scene.layer_plans(size)?;
        self.hits.clear();
        for (i, p) in self.plans.iter().enumerate() {
            if self.slots.get(i).is_none_or(|s| s.size != p.size) {
                let textures = std::array::from_fn(|_| {
                    let d = TextureDescriptor::new();
                    d.set_texture_type(MTLTextureType::D2);
                    d.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                    d.set_width(p.size[0] as u64);
                    d.set_height(p.size[1] as u64);
                    d.set_storage_mode(MTLStorageMode::Private);
                    d.set_usage(MTLTextureUsage::ShaderRead);
                    device.new_texture(&d)
                });
                let slot = Slot {
                    size: p.size,
                    textures,
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
        e.set_fragment_texture(0, Some(&self.slots[slot].textures[1]));
        e.set_fragment_texture(1, Some(&self.slots[slot].textures[0]));
        e.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
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
