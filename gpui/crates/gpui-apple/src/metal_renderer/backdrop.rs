use super::*;
use crate::metal::*;
mod groups;
struct Slot {
    size: [u32; 2],
    textures: [Texture; 3],
}
pub(super) struct Backdrops {
    filter: RenderPipelineState,
    format: MTLPixelFormat,
    slots: Vec<Slot>,
    pub plans: Vec<gpui::BackdropPlan>,
    program: u64,
    pub groups: groups::Groups,
    pub cache: gpui::BackdropCache,
}
impl Backdrops {
    pub fn new(device: &DeviceRef, program: &gpui::SceneShaderProgram) -> Result<Self> {
        Self::with_format(device, program, MTLPixelFormat::BGRA8Unorm)
    }
    pub fn with_format(
        device: &DeviceRef,
        program: &gpui::SceneShaderProgram,
        format: MTLPixelFormat,
    ) -> Result<Self> {
        let library = device
            .newLibraryWithSource_options_error(
                &objc2_foundation::NSString::from_str(&program.metal().0),
                Some(&CompileOptions::new()),
            )
            .map_err(anyhow::Error::msg)?;
        let vertex = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str(&program.metal().1))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str(&program.metal().2))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        let pipeline = |blend| -> Result<RenderPipelineState> {
            let desc = RenderPipelineDescriptor::new();
            desc.setVertexFunction(Some(&vertex));
            desc.setFragmentFunction(Some(&fragment));
            let color = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
            color.setPixelFormat(format);
            color.setBlendingEnabled(blend);
            color.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
            color.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            color.setSourceAlphaBlendFactor(MTLBlendFactor::One);
            color.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            device
                .newRenderPipelineStateWithDescriptor_error(&desc)
                .map_err(anyhow::Error::msg)
        };
        Ok(Self {
            filter: pipeline(false)?,
            format,
            slots: Vec::new(),
            plans: Vec::new(),
            program: program.id(),
            groups: Default::default(),
            cache: Default::default(),
        })
    }
    pub fn format(&self) -> MTLPixelFormat {
        self.format
    }
    /// Release frame-owned captures while preserving pipelines and lifetime statistics.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.plans.clear();
        self.groups.clear();
        self.cache.prepare(Vec::new(), &[]);
    }
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn texture_count(&self) -> usize {
        self.slots.len() * 3 + self.groups.texture_count()
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
                    desc.setTextureType(MTLTextureType::Type2D);
                    desc.setPixelFormat(self.format);
                    unsafe { desc.setWidth(size[0] as usize) };
                    unsafe { desc.setHeight(size[1] as usize) };
                    desc.setStorageMode(MTLStorageMode::Private);
                    desc.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
                    device
                        .newTextureWithDescriptor(&desc)
                        .expect("Metal texture allocation failed")
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
        self.groups.prepare(device, scene, size, self.format)?;
        Ok(())
    }
    pub fn group_filter(&self, cb: &CommandBufferRef, slot: usize) {
        self.groups.filter(cb, &self.filter, slot);
    }
    pub fn group_draw(&self, e: &RenderCommandEncoderRef, slot: usize, begin: bool) {
        self.groups.draw(e, &self.filter, slot, begin)
    }
    fn draw(&self, e: &RenderCommandEncoderRef, index: usize, pass: usize) {
        let params = &self.plans[index].parameters[pass];
        e.setRenderPipelineState(&self.filter);
        unsafe {
            e.setVertexBytes_length_atIndex(
                std::ptr::NonNull::new(params.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                    .expect("non-null bytes"),
                96,
                1,
            )
        };
        unsafe {
            e.setFragmentBytes_length_atIndex(
                std::ptr::NonNull::new(params.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                    .expect("non-null bytes"),
                96,
                1,
            )
        };
        unsafe { e.setFragmentTexture_atIndex(Some(&self.slots[index].textures[pass]), 0) };
        unsafe { e.setFragmentTexture_atIndex(Some(&self.slots[index].textures[0]), 1) };
        unsafe { e.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6) };
    }
    pub fn filter(&mut self, cb: &CommandBufferRef, source: &TextureRef, index: usize) {
        self.cache.start(index);
        if self.cache.hit(index) {
            return;
        }
        let plan = &self.plans[index];
        let textures = &self.slots[index].textures;
        let blit = cb
            .blitCommandEncoder()
            .expect("Metal blit encoder allocation failed");
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(source, 0, 0, MTLOrigin {
                x: plan.origin[0] as usize,
                y: plan.origin[1] as usize,
                z: 0,
            }, MTLSize {
                width: plan.size[0] as usize,
                height: plan.size[1] as usize,
                depth: 1,
            }, &textures[0], 0, 0, MTLOrigin { x: 0, y: 0, z: 0 })
        };
        blit.endEncoding();
        for pass in 0..2 {
            let e = new_command_encoder_for_texture(
                cb,
                &textures[pass + 1],
                size(
                    DevicePixels(plan.reduced[0] as i32),
                    DevicePixels(plan.reduced[1] as i32),
                ),
                Some(MTLClearColor {
                    red: 0.,
                    green: 0.,
                    blue: 0.,
                    alpha: 0.,
                }),
            );
            self.draw(&e, index, pass);
            e.endEncoding();
        }
    }
    pub fn composite(&self, e: &RenderCommandEncoderRef, index: usize) {
        self.draw(e, index, 2)
    }
}
