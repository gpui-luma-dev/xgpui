use super::*;
use gpui::{LayerCacheStats, LayerPlan};
struct Slot {
    size: [u32; 2],
    blur_size: [u32; 2],
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
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn texture_count(&self) -> usize {
        self.slots.iter().map(|slot| slot.textures.len()).sum()
    }
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn texture_bytes(&self) -> u64 {
        self.slots
            .iter()
            .flat_map(|slot| &slot.textures)
            .map(|texture| {
                (texture.width()
                    * texture.height()
                    * if texture.pixelFormat() == MTLPixelFormat::RGBA16Float {
                        8
                    } else {
                        4
                    }) as u64
            })
            .sum()
    }
    pub fn prepare(
        &mut self,
        device: &DeviceRef,
        scene: &Scene,
        size: Size<DevicePixels>,
        format: MTLPixelFormat,
    ) -> Result<()> {
        self.plans = scene.layer_plans(size)?;
        // Four bounded 1024-square groups, with up to four FP16 textures each.
        let bytes: u64 = self
            .plans
            .iter()
            .map(|p| {
                let pixels = 2 * p.size[0] as u64 * p.size[1] as u64
                    + if p.blur_radius > 0. {
                        2 * p.blur_size[0] as u64 * p.blur_size[1] as u64
                    } else {
                        0
                    };
                pixels
                    * if format == MTLPixelFormat::RGBA16Float {
                        8
                    } else {
                        4
                    }
            })
            .sum();
        anyhow::ensure!(
            bytes <= 128 * 1024 * 1024,
            "layer capture targets exceed 128 MiB"
        );
        if format == MTLPixelFormat::RGBA16Float {
            for p in &mut self.plans {
                for params in &mut p.parameters {
                    params[0][3] = 1.;
                }
            }
        }
        self.hits.clear();
        for (i, p) in self.plans.iter().enumerate() {
            if self.slots.get(i).is_none_or(|s| s.size != p.size) {
                let slot = Slot {
                    size: p.size,
                    blur_size: p.blur_size,
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
            if slot.blur_size != p.blur_size {
                // Preserve destination/raw textures while resizing only the filters.
                slot.textures.truncate(2);
                slot.blur_size = p.blur_size;
                if let Some(old) = &mut slot.retained {
                    old.blur_radius = 0.;
                }
            }
            let count = if p.blur_radius > 0. { 4 } else { 2 };
            if slot.textures.len() != count {
                // Keep raw pixels; recreated blur targets need fresh filtering.
                if let Some(old) = &mut slot.retained {
                    old.blur_radius = 0.;
                }
            }
            slot.textures.truncate(count);
            while slot.textures.len() < count {
                let extent = if slot.textures.len() < 2 {
                    p.size
                } else {
                    p.blur_size
                };
                let d = TextureDescriptor::new();
                d.setTextureType(MTLTextureType::Type2D);
                d.setPixelFormat(format);
                unsafe { d.setWidth(extent[0] as usize) };
                unsafe { d.setHeight(extent[1] as usize) };
                d.setStorageMode(MTLStorageMode::Private);
                d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
                slot.textures.push(
                    device
                        .newTextureWithDescriptor(&d)
                        .expect("Metal texture allocation failed"),
                );
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
        let blit = cb
            .blitCommandEncoder()
            .expect("Metal blit encoder allocation failed");
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(source, 0, 0, MTLOrigin {
                x: p.origin[0] as usize,
                y: p.origin[1] as usize,
                z: 0,
            }, MTLSize {
                width: p.size[0] as usize,
                height: p.size[1] as usize,
                depth: 1,
            }, &self.slots[slot].textures[usize::from(!begin)], 0, 0, MTLOrigin { x: 0, y: 0, z: 0 })
        };
        blit.endEncoding();
    }
    pub fn draw(
        &self,
        e: &RenderCommandEncoderRef,
        pipeline: &RenderPipelineStateRef,
        slot: usize,
        begin: bool,
    ) {
        let params = &self.plans[slot].parameters[usize::from(!begin)];
        e.setRenderPipelineState(pipeline);
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
        unsafe {
            e.setFragmentTexture_atIndex(
                Some(
                    &self.slots[slot].textures[if !begin && self.plans[slot].blur_radius > 0. {
                        3
                    } else {
                        1
                    }],
                ),
                0,
            )
        };
        unsafe { e.setFragmentTexture_atIndex(Some(&self.slots[slot].textures[0]), 1) };
        unsafe { e.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6) };
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
                    DevicePixels(p.blur_size[0] as i32),
                    DevicePixels(p.blur_size[1] as i32),
                ),
                Some(MTLClearColor {
                    red: 0.,
                    green: 0.,
                    blue: 0.,
                    alpha: 0.,
                }),
            );
            let params = &p.parameters[axis + 2];
            e.setRenderPipelineState(pipeline);
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
            unsafe { e.setFragmentTexture_atIndex(Some(&self.slots[slot].textures[axis + 1]), 0) };
            unsafe { e.setFragmentTexture_atIndex(Some(&self.slots[slot].textures[0]), 1) };
            unsafe { e.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6) };
            e.endEncoding();
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
