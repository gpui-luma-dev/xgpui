use super::*;
use metal::*;
mod groups;
struct Slot {
    size: [u32; 2],
    textures: [Texture; 3],
}
pub(super) struct Backdrops {
    filter: RenderPipelineState,
    slots: Vec<Slot>,
    pub plans: Vec<gpui::BackdropPlan>,
    program: u64,
    pub groups: groups::Groups,
    pub cache: gpui::BackdropCache,
}
impl Backdrops {
    pub fn new(device: &DeviceRef, program: &gpui::SceneShaderProgram) -> Result<Self> {
        let library = device
            .new_library_with_source(&program.metal().0, &CompileOptions::new())
            .map_err(anyhow::Error::msg)?;
        let vertex = library
            .get_function(&program.metal().1, None)
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .get_function(&program.metal().2, None)
            .map_err(anyhow::Error::msg)?;
        let pipeline = |blend| -> Result<RenderPipelineState> {
            let desc = RenderPipelineDescriptor::new();
            desc.set_vertex_function(Some(&vertex));
            desc.set_fragment_function(Some(&fragment));
            let color = desc.color_attachments().object_at(0).unwrap();
            color.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            color.set_blending_enabled(blend);
            color.set_source_rgb_blend_factor(MTLBlendFactor::SourceAlpha);
            color.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
            color.set_source_alpha_blend_factor(MTLBlendFactor::One);
            color.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
            device
                .new_render_pipeline_state(&desc)
                .map_err(anyhow::Error::msg)
        };
        Ok(Self {
            filter: pipeline(false)?,
            slots: Vec::new(),
            plans: Vec::new(),
            program: program.id(),
            groups: Default::default(),
            cache: Default::default(),
        })
    }
    pub fn prepare(
        &mut self,
        device: &DeviceRef,
        scene: &Scene,
        size: Size<DevicePixels>,
        context: &[u64],
    ) -> Result<()> {
        anyhow::ensure!(
            scene.backdrops.len() <= gpui::MAX_BACKDROPS,
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
                anyhow::ensure!(
                    d.program.id() == self.program,
                    "backdrop programs must match"
                );
                d.plan(size)
            })
            .collect::<Result<_>>()?;
        for (i, plan) in self.plans.iter().enumerate() {
            if self.slots.get(i).is_none_or(|s| s.size != plan.size) {
                self.cache.invalidate(i);
                let textures = std::array::from_fn(|j| {
                    let size = if j == 0 { plan.size } else { plan.reduced };
                    let desc = TextureDescriptor::new();
                    desc.set_texture_type(MTLTextureType::D2);
                    desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                    desc.set_width(size[0] as u64);
                    desc.set_height(size[1] as u64);
                    desc.set_storage_mode(MTLStorageMode::Private);
                    desc.set_usage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
                    device.new_texture(&desc)
                });
                let slot = Slot {
                    size: plan.size,
                    textures,
                };
                if i == self.slots.len() {
                    self.slots.push(slot)
                } else {
                    self.slots[i] = slot
                }
            }
        }
        self.slots.truncate(self.plans.len());
        self.cache
            .prepare(scene.backdrop_cache_keys(size)?, context);
        self.groups.prepare(device, scene, size)?;
        Ok(())
    }
    pub fn group_draw(&self, e: &RenderCommandEncoderRef, slot: usize, begin: bool) {
        self.groups.draw(e, &self.filter, slot, begin)
    }
    fn draw(&self, e: &RenderCommandEncoderRef, index: usize, pass: usize) {
        let params = &self.plans[index].parameters[pass];
        e.set_render_pipeline_state(&self.filter);
        e.set_vertex_bytes(1, 96, params.as_ptr().cast());
        e.set_fragment_bytes(1, 96, params.as_ptr().cast());
        e.set_fragment_texture(0, Some(&self.slots[index].textures[pass]));
        e.set_fragment_texture(1, Some(&self.slots[index].textures[0]));
        e.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
    }
    pub fn filter(&mut self, cb: &CommandBufferRef, source: &TextureRef, index: usize) {
        self.cache.start(index);
        if self.cache.hit(index) {
            return;
        }
        let plan = &self.plans[index];
        let textures = &self.slots[index].textures;
        let blit = cb.new_blit_command_encoder();
        blit.copy_from_texture(
            source,
            0,
            0,
            MTLOrigin {
                x: plan.origin[0] as u64,
                y: plan.origin[1] as u64,
                z: 0,
            },
            MTLSize {
                width: plan.size[0] as u64,
                height: plan.size[1] as u64,
                depth: 1,
            },
            &textures[0],
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
        );
        blit.end_encoding();
        for pass in 0..2 {
            let e = new_command_encoder_for_texture(
                cb,
                &textures[pass + 1],
                size(
                    DevicePixels(plan.reduced[0] as i32),
                    DevicePixels(plan.reduced[1] as i32),
                ),
                Some(MTLClearColor::new(0., 0., 0., 0.)),
            );
            self.draw(e, index, pass);
            e.end_encoding();
        }
    }
    pub fn composite(&self, e: &RenderCommandEncoderRef, index: usize) {
        self.draw(e, index, 2)
    }
}
