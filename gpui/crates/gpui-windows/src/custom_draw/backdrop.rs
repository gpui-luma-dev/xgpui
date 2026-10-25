use super::*;
mod groups;
use gpui::{BackdropPlan, MAX_BACKDROPS, Scene, SceneShaderProgram};
struct Slot {
    size: [u32; 2],
    textures: [TextureBundle; 3],
}
pub(crate) struct Backdrops {
    pipeline: CompositorPipeline,
    uniform: ID3D11Buffer,
    deferred: ID3D11DeviceContext,
    slots: Vec<Slot>,
    plans: Vec<BackdropPlan>,
    program: u64,
    pub groups: groups::Groups,
    pub cache: gpui::BackdropCache,
}
impl Backdrops {
    pub fn new(device: &ID3D11Device, program: &SceneShaderProgram) -> Result<Self> {
        let (source, vertex, fragment) = program.hlsl();
        let pipeline = CompositorPipeline::new(device, source, vertex, fragment)?;
        let (mut uniform, mut deferred) = (None, None);
        unsafe {
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 96,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut uniform),
            )?;
            device.CreateDeferredContext(0, Some(&mut deferred))?;
        }
        Ok(Self {
            pipeline,
            uniform: uniform.context("backdrop uniform")?,
            deferred: deferred.context("backdrop context")?,
            slots: Vec::new(),
            plans: Vec::new(),
            program: program.id(),
            groups: Default::default(),
            cache: Default::default(),
        })
    }
    pub fn prepare(
        &mut self,
        device: &ID3D11Device,
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
        for (i, p) in self.plans.iter().enumerate() {
            if self.slots.get(i).is_none_or(|s| s.size != p.size) {
                self.cache.invalidate(i);
                let make = |sz: [u32; 2]| {
                    allocate_texture_limited(
                        device,
                        gpui::size(DevicePixels(sz[0] as i32), DevicePixels(sz[1] as i32)),
                        DXGI_FORMAT_B8G8R8A8_UNORM,
                        4,
                        4 * 1024 * 1024,
                    )
                };
                let slot = Slot {
                    size: p.size,
                    textures: [make(p.size)?, make(p.reduced)?, make(p.reduced)?],
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
    pub fn group_boundary(
        &self,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        target: &ID3D11RenderTargetView,
        size: Size<DevicePixels>,
        index: usize,
    ) -> Result<()> {
        let (slot, begin) = self.groups.boundary(index);
        self.groups.draw(
            &self.pipeline,
            &self.uniform,
            &self.deferred,
            context,
            source,
            target,
            size,
            slot,
            begin,
        )
    }
    pub fn draw(
        &mut self,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        target: &ID3D11RenderTargetView,
        size: Size<DevicePixels>,
        index: usize,
    ) -> Result<()> {
        self.cache.start(index);
        let hit = self.cache.hit(index);
        let dc = &self.deferred;
        let plan = &self.plans[index];
        let textures = &self.slots[index].textures;
        self.pipeline.bind(dc);
        unsafe {
            dc.OMSetRenderTargets(None, None);
            if !hit {
                dc.CopySubresourceRegion(
                    &textures[0].texture,
                    0,
                    0,
                    0,
                    0,
                    source,
                    0,
                    Some(&D3D11_BOX {
                        left: plan.origin[0],
                        top: plan.origin[1],
                        front: 0,
                        right: plan.origin[0] + plan.size[0],
                        bottom: plan.origin[1] + plan.size[1],
                        back: 1,
                    }),
                );
            }
            dc.VSSetConstantBuffers(0, Some(&[Some(self.uniform.clone())]));
            dc.PSSetConstantBuffers(0, Some(&[Some(self.uniform.clone())]));
            for pass in (if hit { 2 } else { 0 })..3 {
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                dc.Map(
                    &self.uniform,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )?;
                std::ptr::copy_nonoverlapping(
                    plan.parameters[pass].as_ptr().cast::<u8>(),
                    mapped.pData.cast(),
                    96,
                );
                dc.Unmap(&self.uniform, 0);
                let destination = if pass == 2 {
                    target
                } else {
                    &textures[pass + 1].rtv
                };
                dc.OMSetRenderTargets(Some(&[Some(destination.clone())]), None);
                dc.RSSetViewports(Some(&[viewport(if pass == 2 {
                    size
                } else {
                    textures[pass + 1].extent
                })]));
                dc.OMSetBlendState(None, None, u32::MAX);
                dc.PSSetShaderResources(
                    0,
                    Some(&[
                        Some(textures[pass].srv.clone()),
                        Some(textures[0].srv.clone()),
                    ]),
                );
                dc.Draw(6, 0);
                dc.PSSetShaderResources(0, Some(&[None, None]));
            }
            dc.OMSetRenderTargets(None, None);
            let mut commands = None;
            dc.FinishCommandList(false, Some(&mut commands))?;
            context.ExecuteCommandList(commands.as_ref().context("backdrop commands")?, true);
            self.pipeline.device.GetDeviceRemovedReason()?;
        }
        Ok(())
    }
}
