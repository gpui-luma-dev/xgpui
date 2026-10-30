//! Native wide SDR composition; canonical linear scRGB, ordered sRGB UI runs.
use super::*;
use anyhow::ensure;

#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;

/// The caller partitions at wide surfaces, preserving scene order. Native scenes
/// retain their own GPUI batch ordering. This does not yet partition a mixed Scene.
#[cfg(test)]
enum Layer<'a> {
    Ui(&'a Scene),
    P3 {
        source: &'a dyn CustomContent,
        extent: Size<DevicePixels>,
        bounds: [f32; 4],
        opacity: f32,
    },
}

struct Target {
    #[cfg(test)]
    texture: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    rtv: ID3D11RenderTargetView,
}
impl Target {
    fn new(device: &ID3D11Device, extent: Size<DevicePixels>) -> Result<Self> {
        validate_extent(extent)?;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: extent.width.0 as u32,
            Height: extent.height.0 as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
            ..Default::default()
        };
        let (mut texture, mut srv, mut rtv) = (None, None, None);
        unsafe {
            device.CreateTexture2D(&desc, None, Some(&mut texture))?;
            let texture = texture.as_ref().context("wide texture missing")?;
            device.CreateShaderResourceView(texture, None, Some(&mut srv))?;
            device.CreateRenderTargetView(texture, None, Some(&mut rtv))?;
        }
        Ok(Self {
            #[cfg(test)]
            texture: texture.unwrap(),
            srv: srv.unwrap(),
            rtv: rtv.unwrap(),
        })
    }
}

pub(super) fn validate_extent(extent: Size<DevicePixels>) -> Result<()> {
    let (w, h) = (extent.width.0, extent.height.0);
    ensure!(
        (1..=16384).contains(&w) && (1..=16384).contains(&h),
        "invalid wide viewport"
    );
    // Float16 target plus BGRA scratch; excludes existing path intermediates.
    ensure!(
        w as u64 * h as u64 * 12 <= 256 * 1024 * 1024,
        "wide viewport exceeds 256 MiB"
    );
    Ok(())
}

pub(super) struct Compositor {
    target: Target,
    scratch: crate::custom_draw::TextureBundle,
    present_fragment: ID3D11PixelShader,
    #[cfg(any(test, feature = "test-support"))]
    preview_fragment: ID3D11PixelShader,
    extent: Size<DevicePixels>,
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    uniform: ID3D11Buffer,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    gray_fragment: ID3D11PixelShader,
    gray_blend: ID3D11BlendState,
}
impl Compositor {
    pub(super) fn new(device: &ID3D11Device, extent: Size<DevicePixels>) -> Result<Self> {
        let target = Target::new(device, extent)?;
        let scratch = crate::custom_draw::allocate_frame_texture(device, extent)?;
        let source = include_str!("wide_sdr.hlsl");
        let present_fragment = create_fragment_shader(
            device,
            &crate::custom_draw::compile(source, "present_main", "ps_5_0")?,
        )?;
        #[cfg(any(test, feature = "test-support"))]
        let preview_fragment = create_fragment_shader(
            device,
            &crate::custom_draw::compile(source, "preview_main", "ps_5_0")?,
        )?;
        let vertex = create_vertex_shader(
            device,
            &crate::custom_draw::compile(source, "vertex_main", "vs_5_0")?,
        )?;
        let fragment = create_fragment_shader(
            device,
            &crate::custom_draw::compile(source, "fragment_main", "ps_5_0")?,
        )?;
        let text_source = include_str!("../shaders.hlsl").replace(
            "#include \"alpha_correction.hlsl\"",
            include_str!("../alpha_correction.hlsl"),
        );
        let text_source = text_source + include_str!("wide_sdr/grayscale.hlsl");
        let gray_fragment = create_fragment_shader(
            device,
            &crate::custom_draw::compile(&text_source, "wide_grayscale_fragment", "ps_5_0")?,
        )?;
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: 32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut uniform = None;
        unsafe {
            device.CreateBuffer(&desc, None, Some(&mut uniform))?;
        }
        // Same associated source-over factors as path rasterization, including alpha.
        let blend = create_blend_state_for_path_rasterization(device)?;
        let gray_blend = create_blend_state(device)?;
        let mut raster = None;
        unsafe {
            device.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    DepthClipEnable: true.into(),
                    MultisampleEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut raster),
            )?;
        }
        Ok(Self {
            target,
            scratch,
            present_fragment,
            #[cfg(any(test, feature = "test-support"))]
            preview_fragment,
            extent,
            vertex,
            fragment,
            uniform: uniform.unwrap(),
            blend,
            gray_fragment,
            gray_blend,
            raster: raster.unwrap(),
        })
    }

    #[cfg(test)]
    fn render(&self, renderer: &mut DirectXRenderer, layers: &[Layer<'_>]) -> Result<()> {
        validate_extent(self.extent)?;
        let devices = renderer
            .devices
            .as_ref()
            .context("devices missing")?
            .clone();
        ensure!(
            unsafe { self.target.texture.GetDevice()? }.as_raw() == devices.device.as_raw(),
            "compositor belongs to another device"
        );
        ensure!(
            renderer
                .resources
                .as_ref()
                .context("resources missing")?
                .swap_chain
                .is_none(),
            "private compositor requires a headless target"
        );
        // Prepare/validate every wide output before submitting any recording.
        // Failure drops this batch and abandons all staged output identities.
        let mut batch = crate::custom_draw::PreparationBatch::new(
            devices.device.clone(),
            devices.device_context.clone(),
        );
        let mut sources = Vec::with_capacity(layers.len());
        for layer in layers {
            let source = match layer {
                Layer::Ui(_) => None,
                Layer::P3 {
                    source,
                    extent,
                    bounds,
                    opacity,
                } => {
                    ensure!(
                        bounds.iter().all(|v| v.is_finite()) && bounds[2] > 0. && bounds[3] > 0.,
                        "invalid surface bounds"
                    );
                    ensure!(
                        opacity.is_finite() && (0. ..=1.).contains(opacity),
                        "invalid surface opacity"
                    );
                    Some(batch.prepare(|context| {
                        let request = PreparationRequest::new(
                            size(px(bounds[2]), px(bounds[3])),
                            *extent,
                            1.,
                        )?;
                        let output = source.prepare_with_request(context, &request)?;
                        let output = output
                            .downcast::<crate::custom_draw::Dx11WideSdrOutput>()
                            .map_err(|_| {
                                anyhow::anyhow!("wide layer requires typed Dx11WideSdrOutput")
                            })?
                            .validate(context, *extent)?;
                        ensure!(
                            output.texture.as_raw() != self.target.texture.as_raw(),
                            "wide target cannot sample itself"
                        );
                        Ok(output)
                    })?)
                }
            };
            sources.push(source);
        }
        renderer.resize(self.extent)?;
        batch.submit()?;
        unsafe {
            devices
                .device_context
                .ClearRenderTargetView(&self.target.rtv, &[0.; 4]);
        }
        for (layer, source) in layers.iter().zip(&sources) {
            match layer {
                Layer::Ui(scene) => {
                    // Scoped substitution handles cached SubpixelSprite batches without
                    // changing the scene, atlas format, default renderer or public API.
                    unsafe {
                        devices.device_context.RSSetState(&self.raster);
                    }
                    let old_fragment = std::mem::replace(
                        &mut renderer.pipelines.subpixel_sprites.fragment,
                        self.gray_fragment.clone(),
                    );
                    let old_blend = std::mem::replace(
                        &mut renderer.pipelines.subpixel_sprites.blend_state,
                        self.gray_blend.clone(),
                    );
                    let result = renderer.render(scene, WindowBackgroundAppearance::Transparent);
                    renderer.pipelines.subpixel_sprites.fragment = old_fragment;
                    renderer.pipelines.subpixel_sprites.blend_state = old_blend;
                    result?;
                    let texture = renderer
                        .resources
                        .as_ref()
                        .context("resources missing")?
                        .render_target
                        .as_ref()
                        .context("scratch missing")?;
                    let mut srv = None;
                    unsafe {
                        devices
                            .device
                            .CreateShaderResourceView(texture, None, Some(&mut srv))?;
                    }
                    self.draw(
                        &devices,
                        srv.as_ref().unwrap(),
                        [
                            0.,
                            0.,
                            self.extent.width.0 as f32,
                            self.extent.height.0 as f32,
                        ],
                        1.,
                        false,
                    )?;
                }
                Layer::P3 {
                    bounds, opacity, ..
                } => self.draw(
                    &devices,
                    &source.as_ref().context("wide source missing")?.srv,
                    *bounds,
                    *opacity,
                    true,
                )?,
            }
        }
        // Do not leave the wide RTV installed in the legacy renderer.
        unsafe {
            devices.device_context.OMSetRenderTargets(None, None);
            devices
                .device_context
                .PSSetShaderResources(0, Some(&[None]));
        }
        Ok(())
    }

    fn draw(
        &self,
        devices: &DirectXRendererDevices,
        source: &ID3D11ShaderResourceView,
        bounds: [f32; 4],
        opacity: f32,
        p3: bool,
    ) -> Result<()> {
        let context = &devices.device_context;
        let params = [
            bounds,
            [
                self.extent.width.0 as f32,
                self.extent.height.0 as f32,
                opacity,
                if p3 { 1. } else { 0. },
            ],
        ];
        update_buffer(context, &self.uniform, &params)?;
        unsafe {
            context.RSSetState(&self.raster);
        }
        unsafe {
            // Unbind scratch RTV before binding it as a sampled resource.
            context.OMSetRenderTargets(Some(&[Some(self.target.rtv.clone())]), None);
            context.RSSetViewports(Some(&[crate::custom_draw::viewport(self.extent)]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.fragment, None);
            context.VSSetConstantBuffers(0, Some(&[Some(self.uniform.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.uniform.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(source.clone())]));
            context.OMSetBlendState(&self.blend, None, u32::MAX);
            context.OMSetDepthStencilState(None, 0);
            context.Draw(6, 0);
            // Next UI run may render into this source; remove the SRV hazard first.
            context.PSSetShaderResources(0, Some(&[None]));
        }
        Ok(())
    }
}

impl Compositor {
    pub(super) fn resize(
        &mut self,
        device: &ID3D11Device,
        extent: Size<DevicePixels>,
    ) -> Result<()> {
        if self.extent != extent {
            let target = Target::new(device, extent)?;
            let scratch = crate::custom_draw::allocate_frame_texture(device, extent)?;
            self.target = target;
            self.scratch = scratch;
            self.extent = extent;
        }
        Ok(())
    }
    pub(super) fn render_scene(
        &mut self,
        renderer: &mut DirectXRenderer,
        scene: &Scene,
        outputs: &[crate::custom_draw::PreparedDraw],
        background: WindowBackgroundAppearance,
    ) -> Result<()> {
        let devices = renderer
            .devices
            .as_ref()
            .context("devices missing")?
            .clone();
        self.resize(
            &devices.device,
            size(
                DevicePixels(renderer.width as i32),
                DevicePixels(renderer.height as i32),
            ),
        )?;
        let resources = renderer.resources.as_mut().context("resources missing")?;
        let old_texture = resources
            .render_target
            .replace(self.scratch.texture.clone());
        let old_view = resources
            .render_target_view
            .replace(self.scratch.rtv.clone());
        let old_fragment = std::mem::replace(
            &mut renderer.pipelines.subpixel_sprites.fragment,
            self.gray_fragment.clone(),
        );
        let old_blend = std::mem::replace(
            &mut renderer.pipelines.subpixel_sprites.blend_state,
            self.gray_blend.clone(),
        );
        let result = (|| {
            unsafe {
                devices
                    .device_context
                    .ClearRenderTargetView(&self.target.rtv, &[0.; 4]);
            }
            unsafe {
                devices.device_context.RSSetState(&self.raster);
            }
            renderer.pre_draw(&if background == WindowBackgroundAppearance::Opaque {
                [1.; 4]
            } else {
                [0.; 4]
            })?;
            let mut dirty = true; // includes the initial window background
            for batch in scene.batches() {
                if let PrimitiveBatch::Shaders {
                    program_id,
                    ref range,
                } = batch
                {
                    if scene.shader_draws[range.start].program.output_encoding()
                        != gpui::ShaderOutputEncoding::EncodedSrgb
                    {
                        if dirty {
                            self.flush_ui(&devices)?;
                        }
                        renderer
                            .shaders
                            .as_ref()
                            .context("shader renderer missing")?
                            .draw(
                                &devices.device_context,
                                &self.target.rtv,
                                self.extent,
                                program_id,
                                range.clone(),
                                scene.shader_draws[range.start].glyph_mask.and_then(|tile| {
                                    renderer.atlas.get_texture_view(tile.texture_id)
                                }),
                            )?;
                        renderer.pre_draw(&[0.; 4])?;
                        dirty = false;
                        continue;
                    }
                }
                if let PrimitiveBatch::Custom(range) = batch {
                    for index in range {
                        if outputs[index].wide {
                            if dirty {
                                self.flush_ui(&devices)?;
                            }
                            renderer
                                .custom_compositor
                                .as_ref()
                                .context("custom compositor missing")?
                                .draw(
                                    &devices.device_context,
                                    &self.target.rtv,
                                    self.extent,
                                    &scene.custom_draws[index..index + 1],
                                    &outputs[index..index + 1],
                                )?;
                            // Restore native globals and consume scratch before clearing it.
                            renderer.pre_draw(&[0.; 4])?;
                            dirty = false;
                        } else {
                            renderer.render_batch(
                                scene,
                                outputs,
                                PrimitiveBatch::Custom(index..index + 1),
                            )?;
                            dirty = true;
                        }
                    }
                } else {
                    renderer.render_batch(scene, outputs, batch)?;
                    dirty = true;
                }
            }
            if dirty {
                self.flush_ui(&devices)?;
            }
            Ok(())
        })();
        renderer.pipelines.subpixel_sprites.fragment = old_fragment;
        renderer.pipelines.subpixel_sprites.blend_state = old_blend;
        let resources = renderer.resources.as_mut().unwrap();
        resources.render_target = old_texture;
        resources.render_target_view = old_view;
        unsafe {
            devices.device_context.OMSetRenderTargets(None, None);
            devices
                .device_context
                .PSSetShaderResources(0, Some(&[None, None]));
        }
        result
    }
    fn flush_ui(&self, devices: &DirectXRendererDevices) -> Result<()> {
        self.draw(
            devices,
            &self.scratch.srv,
            [
                0.,
                0.,
                self.extent.width.0 as f32,
                self.extent.height.0 as f32,
            ],
            1.,
            false,
        )
    }
    fn blit(
        &self,
        devices: &DirectXRendererDevices,
        target: &ID3D11RenderTargetView,
        fragment: &ID3D11PixelShader,
        scale: f32,
    ) -> Result<()> {
        let params = [
            [
                0.,
                0.,
                self.extent.width.0 as f32,
                self.extent.height.0 as f32,
            ],
            [
                self.extent.width.0 as f32,
                self.extent.height.0 as f32,
                scale,
                0.,
            ],
        ];
        let context = &devices.device_context;
        update_buffer(context, &self.uniform, &params)?;
        unsafe {
            context.RSSetState(&self.raster);
        }
        unsafe {
            context.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
            context.RSSetViewports(Some(&[crate::custom_draw::viewport(self.extent)]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(fragment, None);
            context.VSSetConstantBuffers(0, Some(&[Some(self.uniform.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.uniform.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(self.target.srv.clone())]));
            context.OMSetBlendState(None, None, u32::MAX);
            context.OMSetDepthStencilState(None, 0);
            context.Draw(6, 0);
            context.PSSetShaderResources(0, Some(&[None]));
            context.OMSetRenderTargets(None, None);
        }
        Ok(())
    }
    pub(super) fn present(
        &self,
        devices: &DirectXRendererDevices,
        target: &ID3D11RenderTargetView,
        scale: f32,
    ) -> Result<()> {
        ensure!(scale.is_finite() && scale > 0., "invalid SDR white scale");
        self.blit(devices, target, &self.present_fragment, scale)
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn preview(&self, devices: &DirectXRendererDevices) -> Result<ID3D11Texture2D> {
        self.blit(devices, &self.scratch.rtv, &self.preview_fragment, 1.)?;
        Ok(self.scratch.texture.clone())
    }
}
#[cfg(test)]
impl Compositor {
    pub(super) fn fragment_identity(&self) -> usize {
        self.fragment.as_raw() as usize
    }
}
