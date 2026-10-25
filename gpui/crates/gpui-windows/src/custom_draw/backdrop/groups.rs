use super::*;
use gpui::{LayerCacheStats, LayerPlan};
struct Slot {
    size: [u32; 2],
    textures: [TextureBundle; 2],
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
        device: &ID3D11Device,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<()> {
        self.plans = scene.layer_plans(size)?;
        self.hits.clear();
        for (i, p) in self.plans.iter().enumerate() {
            if self.slots.get(i).is_none_or(|s| s.size != p.size) {
                let make = || {
                    allocate_texture_limited(
                        device,
                        gpui::size(
                            DevicePixels(p.size[0] as i32),
                            DevicePixels(p.size[1] as i32),
                        ),
                        DXGI_FORMAT_B8G8R8A8_UNORM,
                        4,
                        4 * 1024 * 1024,
                    )
                };
                let slot = Slot {
                    size: p.size,
                    textures: [make()?, make()?],
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
    pub(super) fn draw(
        &self,
        pipeline: &CompositorPipeline,
        uniform: &ID3D11Buffer,
        dc: &ID3D11DeviceContext,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        target: &ID3D11RenderTargetView,
        size: Size<DevicePixels>,
        slot: usize,
        begin: bool,
    ) -> Result<()> {
        let p = &self.plans[slot];
        let hit = self.hits[slot];
        let textures = &self.slots[slot].textures;
        pipeline.bind(dc);
        unsafe {
            dc.OMSetRenderTargets(None, None);
            if begin || !hit {
                dc.CopySubresourceRegion(
                    &textures[usize::from(!begin)].texture,
                    0,
                    0,
                    0,
                    0,
                    source,
                    0,
                    Some(&D3D11_BOX {
                        left: p.origin[0],
                        top: p.origin[1],
                        front: 0,
                        right: p.origin[0] + p.size[0],
                        bottom: p.origin[1] + p.size[1],
                        back: 1,
                    }),
                );
            }
            if !begin || !hit {
                let params = &p.parameters[usize::from(!begin)];
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                dc.Map(uniform, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
                std::ptr::copy_nonoverlapping(
                    params.as_ptr().cast::<u8>(),
                    mapped.pData.cast(),
                    96,
                );
                dc.Unmap(uniform, 0);
                dc.VSSetConstantBuffers(0, Some(&[Some(uniform.clone())]));
                dc.PSSetConstantBuffers(0, Some(&[Some(uniform.clone())]));
                dc.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
                dc.RSSetViewports(Some(&[viewport(size)]));
                dc.PSSetShaderResources(
                    0,
                    Some(&[Some(textures[1].srv.clone()), Some(textures[0].srv.clone())]),
                );
                dc.Draw(6, 0);
                dc.PSSetShaderResources(0, Some(&[None, None]));
                dc.OMSetRenderTargets(None, None);
            }
            let mut commands = None;
            dc.FinishCommandList(false, Some(&mut commands))?;
            context.ExecuteCommandList(commands.as_ref().context("group boundary commands")?, true);
            pipeline.device.GetDeviceRemovedReason()?;
        }
        Ok(())
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
