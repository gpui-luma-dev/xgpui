//! Private WARP/headless checkpoint. Deliberately compiled only for backend tests.
//! Ordered native scene runs and explicit P3 textures; no public producer or window API.
use super::*;
use anyhow::ensure;

mod reference;
mod tests;

/// The caller partitions at wide surfaces, preserving scene order. Native scenes
/// retain their own GPUI batch ordering. This does not yet partition a mixed Scene.
enum Layer<'a> {
    Ui(&'a Scene),
    P3 {
        source: &'a Target,
        bounds: [f32; 4],
        opacity: f32,
    },
}

struct Target {
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
            texture: texture.unwrap(),
            srv: srv.unwrap(),
            rtv: rtv.unwrap(),
        })
    }
}

fn validate_extent(extent: Size<DevicePixels>) -> Result<()> {
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

struct Compositor {
    target: Target,
    extent: Size<DevicePixels>,
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    uniform: ID3D11Buffer,
    blend: ID3D11BlendState,
    gray_fragment: ID3D11PixelShader,
    gray_blend: ID3D11BlendState,
}
impl Compositor {
    fn new(device: &ID3D11Device, extent: Size<DevicePixels>) -> Result<Self> {
        let target = Target::new(device, extent)?;
        let source = include_str!("wide_sdr.hlsl");
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
        Ok(Self {
            target,
            extent,
            vertex,
            fragment,
            uniform: uniform.unwrap(),
            blend,
            gray_fragment,
            gray_blend,
        })
    }

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
        // Validate every explicit surface before clearing/encoding the frame.
        for layer in layers {
            if let Layer::P3 {
                source,
                bounds,
                opacity,
            } = layer
            {
                ensure!(
                    bounds.iter().all(|v| v.is_finite()) && bounds[2] > 0. && bounds[3] > 0.,
                    "invalid surface bounds"
                );
                ensure!(
                    opacity.is_finite() && (0. ..=1.).contains(opacity),
                    "invalid surface opacity"
                );
                ensure!(
                    unsafe { source.texture.GetDevice()? }.as_raw() == devices.device.as_raw(),
                    "foreign wide texture"
                );
                ensure!(
                    source.texture.as_raw() != self.target.texture.as_raw(),
                    "wide target cannot sample itself"
                );
            }
        }
        renderer.resize(self.extent)?;
        unsafe {
            devices
                .device_context
                .ClearRenderTargetView(&self.target.rtv, &[0.; 4]);
        }
        for layer in layers {
            match layer {
                Layer::Ui(scene) => {
                    // Scoped substitution handles cached SubpixelSprite batches without
                    // changing the scene, atlas format, default renderer or public API.
                    set_rasterizer_state(&devices.device, &devices.device_context)?;
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
                    source,
                    bounds,
                    opacity,
                } => self.draw(&devices, &source.srv, *bounds, *opacity, true)?,
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
        set_rasterizer_state(&devices.device, context)?;
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
