//! DirectX 11 preparation for retained custom paints. Only the host executes work.
use anyhow::{Context as _, Result, ensure};
use gpui::{CustomDraw, DevicePixels, Size, size};
use std::{
    ffi::CString,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
};
use windows::{
    Win32::Graphics::{
        Direct3D::{Fxc::*, *},
        Direct3D11::*,
        Dxgi::Common::*,
    },
    core::{Interface, PCSTR},
};

const PENDING: u8 = 0;
const SUBMITTED: u8 = 1;
const ABANDONED: u8 = 2;
const STAGED: u8 = 3;

#[derive(Debug)]
struct Recording {
    state: AtomicU8,
    batch: Option<Arc<()>>,
}
impl Recording {
    fn new(batch: Option<Arc<()>>) -> Self {
        Self {
            state: AtomicU8::new(PENDING),
            batch,
        }
    }
    fn can_reuse(self: &Arc<Self>, output: &Arc<Self>) -> bool {
        match output.state.load(Ordering::Acquire) {
            SUBMITTED => true,
            PENDING => Arc::ptr_eq(output, self),
            STAGED => self
                .batch
                .as_ref()
                .zip(output.batch.as_ref())
                .is_some_and(|(a, b)| Arc::ptr_eq(a, b)),
            _ => false,
        }
    }
    fn abandon(&self, from: u8) {
        let _ = self
            .state
            .compare_exchange(from, ABANDONED, Ordering::AcqRel, Ordering::Acquire);
    }
}

struct DeferredState {
    context: Option<ID3D11DeviceContext>,
    finished: bool,
    failed: bool,
}

pub(crate) struct PreparationBatch {
    device: ID3D11Device,
    owner: ID3D11DeviceContext,
    id: Arc<()>,
    pending: Vec<(Option<ID3D11CommandList>, Arc<Recording>)>,
}
impl PreparationBatch {
    pub fn new(device: ID3D11Device, owner: ID3D11DeviceContext) -> Self {
        Self {
            device,
            owner,
            id: Arc::new(()),
            pending: Vec::new(),
        }
    }
    /// Run producer preparation and host output validation before staging its work.
    pub fn prepare<T>(
        &mut self,
        prepare: impl FnOnce(&Dx11CustomContext) -> Result<T>,
    ) -> Result<T> {
        let context = Dx11CustomContext::with_batch(
            self.device.clone(),
            self.owner.clone(),
            Some(self.id.clone()),
        )?;
        let output = prepare(&context)?;
        let command = context.finish()?;
        context.recording.state.store(STAGED, Ordering::Release);
        self.pending.push((command, context.recording.clone()));
        Ok(output)
    }
    pub fn submit(self) -> Result<()> {
        unsafe {
            self.device.GetDeviceRemovedReason()?;
        }
        for (command, recording) in &self.pending {
            if let Some(command) = command {
                unsafe {
                    self.owner.ExecuteCommandList(command, true);
                    self.device.GetDeviceRemovedReason()?;
                }
            }
            recording.state.store(SUBMITTED, Ordering::Release);
        }
        Ok(())
    }
}
impl Drop for PreparationBatch {
    fn drop(&mut self) {
        for (_, recording) in &self.pending {
            recording.abandon(STAGED);
        }
    }
}

/// Host-owned deferred context; app authors normally use xgpui-shader instead.
pub struct Dx11CustomContext {
    /// Device used for all preparation resources.
    pub device: ID3D11Device,
    owner: ID3D11DeviceContext,
    deferred: Mutex<DeferredState>,
    recording: Arc<Recording>,
}
impl Dx11CustomContext {
    #[cfg(test)]
    pub(crate) fn new(device: ID3D11Device, owner: ID3D11DeviceContext) -> Result<Self> {
        Self::with_batch(device, owner, None)
    }
    fn with_batch(
        device: ID3D11Device,
        owner: ID3D11DeviceContext,
        batch: Option<Arc<()>>,
    ) -> Result<Self> {
        ensure!(
            unsafe { device.GetFeatureLevel() }.0 >= D3D_FEATURE_LEVEL_11_0.0,
            "custom paints require Direct3D feature level 11.0"
        );
        Ok(Self {
            device,
            owner,
            deferred: Mutex::new(DeferredState {
                context: None,
                finished: false,
                failed: false,
            }),
            recording: Arc::new(Recording::new(batch)),
        })
    }
    /// Immediate-context identity, retained by outputs to prevent address reuse.
    pub fn cache_key(&self) -> usize {
        self.owner.as_raw() as usize
    }
    /// Allocate an immutable-after-submission straight-alpha BGRA8Unorm source.
    pub fn create_output(&self, extent: Size<DevicePixels>) -> Result<Dx11CustomOutput> {
        ensure!(
            self.recording.state.load(Ordering::Acquire) == PENDING,
            "DirectX preparation is no longer writable"
        );
        Ok(Dx11CustomOutput {
            bundle: allocate_texture(&self.device, extent)?,
            owner: self.owner.clone(),
            recording: self.recording.clone(),
        })
    }
    /// Whether output is submitted, local to this recording, or staged earlier in this host batch.
    pub fn can_reuse(&self, output: &Dx11CustomOutput) -> bool {
        self.owner.as_raw() == output.owner.as_raw() && self.recording.can_reuse(&output.recording)
    }
    /// Serialize encoding into the host's deferred context. Do not execute work.
    /// An encoding error invalidates this transaction, including commands recorded earlier.
    pub fn record<T>(&self, encode: impl FnOnce(&ID3D11DeviceContext) -> Result<T>) -> Result<T> {
        let mut state = self
            .deferred
            .lock()
            .map_err(|_| anyhow::anyhow!("DirectX preparation poisoned"))?;
        ensure!(
            !state.finished && !state.failed,
            "DirectX preparation already finished or failed"
        );
        let result = (|| {
            if state.context.is_none() {
                unsafe {
                    self.device
                        .CreateDeferredContext(0, Some(&mut state.context))?;
                }
            }
            encode(
                state
                    .context
                    .as_ref()
                    .context("missing DirectX deferred context")?,
            )
        })();
        if result.is_err() {
            state.failed = true;
            state.context.take();
            self.recording.abandon(PENDING);
        }
        result
    }
    #[cfg(test)]
    fn clear(&self, output: &Dx11CustomOutput, color: [f32; 4]) -> Result<()> {
        self.ensure_writable(output)?;
        self.record(|context| {
            unsafe { context.ClearRenderTargetView(&output.bundle.rtv, &color) };
            Ok(())
        })
    }
    #[cfg(test)]
    fn ensure_writable(&self, output: &Dx11CustomOutput) -> Result<()> {
        ensure!(
            Arc::ptr_eq(&self.recording, &output.recording)
                && self.recording.state.load(Ordering::Acquire) == PENDING,
            "cannot mutate submitted or foreign DirectX output"
        );
        Ok(())
    }
    fn finish(&self) -> Result<Option<ID3D11CommandList>> {
        let mut state = self
            .deferred
            .lock()
            .map_err(|_| anyhow::anyhow!("DirectX preparation poisoned"))?;
        ensure!(
            !state.finished && !state.failed,
            "DirectX preparation already finished or failed"
        );
        state.finished = true;
        let Some(context) = state.context.take() else {
            return Ok(None);
        };
        let mut commands = None;
        unsafe {
            context.FinishCommandList(false, Some(&mut commands))?;
        }
        Ok(Some(commands.context("missing DirectX preparation list")?))
    }
    #[cfg(test)]
    pub(crate) fn submit(&self) -> Result<()> {
        if let Some(command) = self.finish()? {
            unsafe {
                self.owner.ExecuteCommandList(&command, true);
                self.device.GetDeviceRemovedReason()?;
            }
        }
        self.recording.state.store(SUBMITTED, Ordering::Release);
        Ok(())
    }
}
impl Drop for Dx11CustomContext {
    fn drop(&mut self) {
        self.recording.abandon(PENDING);
    }
}

/// Cacheable producer output; construction binds device, context and format.
#[derive(Clone, Debug)]
pub struct Dx11CustomOutput {
    bundle: TextureBundle,
    owner: ID3D11DeviceContext,
    recording: Arc<Recording>,
}
impl Dx11CustomOutput {
    /// Immutable sampleable texture after host submission.
    pub fn texture(&self) -> &ID3D11Texture2D {
        &self.bundle.texture
    }
    pub(crate) fn validate(
        self,
        context: &Dx11CustomContext,
        extent: Size<DevicePixels>,
    ) -> Result<TextureBundle> {
        ensure!(
            context.can_reuse(&self),
            "foreign or abandoned DirectX output"
        );
        ensure!(
            self.bundle.extent == extent,
            "invalid DirectX output extent"
        );
        ensure!(
            unsafe { self.bundle.texture.GetDevice()? }.as_raw() == context.device.as_raw(),
            "DirectX output belongs to another device"
        );
        Ok(self.bundle)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextureBundle {
    pub texture: ID3D11Texture2D,
    pub srv: ID3D11ShaderResourceView,
    pub rtv: ID3D11RenderTargetView,
    pub extent: Size<DevicePixels>,
}
pub(crate) fn allocate_texture(
    device: &ID3D11Device,
    extent: Size<DevicePixels>,
) -> Result<TextureBundle> {
    ensure!(
        extent.width.0 > 0
            && extent.height.0 > 0
            && extent.width.0 <= 16384
            && extent.height.0 <= 16384
            && extent.width.0 as u64 * extent.height.0 as u64 * 4 <= 64 * 1024 * 1024,
        "invalid DirectX custom texture extent"
    );
    let desc = D3D11_TEXTURE2D_DESC {
        Width: extent.width.0 as u32,
        Height: extent.height.0 as u32,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        ..Default::default()
    };
    let (mut texture, mut srv, mut rtv) = (None, None, None);
    unsafe {
        device.CreateTexture2D(&desc, None, Some(&mut texture))?;
        let texture = texture.as_ref().context("missing custom texture")?;
        device.CreateShaderResourceView(texture, None, Some(&mut srv))?;
        device.CreateRenderTargetView(texture, None, Some(&mut rtv))?;
    }
    Ok(TextureBundle {
        texture: texture.unwrap(),
        srv: srv.unwrap(),
        rtv: rtv.unwrap(),
        extent,
    })
}
pub(crate) fn viewport(extent: Size<DevicePixels>) -> D3D11_VIEWPORT {
    D3D11_VIEWPORT {
        Width: extent.width.0 as f32,
        Height: extent.height.0 as f32,
        MinDepth: 0.,
        MaxDepth: 1.,
        ..Default::default()
    }
}

/// Backend pipeline compiled from extension-generated HLSL shader model 5.0.
struct CompositorPipeline {
    device: ID3D11Device,
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    raster: ID3D11RasterizerState,
    depth: ID3D11DepthStencilState,
}
impl CompositorPipeline {
    /// Compile shaders; entry names come from the extension's translation metadata.
    pub fn new(
        device: &ID3D11Device,
        source: &str,
        vertex_entry: &str,
        fragment_entry: &str,
    ) -> Result<Self> {
        let vertex_bytes = compile(source, vertex_entry, "vs_5_0")?;
        let fragment_bytes = compile(source, fragment_entry, "ps_5_0")?;
        let (mut vertex, mut fragment, mut raster, mut depth) = (None, None, None, None);
        unsafe {
            device.CreateVertexShader(&vertex_bytes, None, Some(&mut vertex))?;
            device.CreatePixelShader(&fragment_bytes, None, Some(&mut fragment))?;
            device.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    DepthClipEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut raster),
            )?;
            device.CreateDepthStencilState(
                &D3D11_DEPTH_STENCIL_DESC {
                    DepthEnable: false.into(),
                    DepthFunc: D3D11_COMPARISON_ALWAYS,
                    DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                    StencilEnable: false.into(),
                    FrontFace: D3D11_DEPTH_STENCILOP_DESC {
                        StencilFailOp: D3D11_STENCIL_OP_KEEP,
                        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
                        StencilPassOp: D3D11_STENCIL_OP_KEEP,
                        StencilFunc: D3D11_COMPARISON_ALWAYS,
                    },
                    BackFace: D3D11_DEPTH_STENCILOP_DESC {
                        StencilFailOp: D3D11_STENCIL_OP_KEEP,
                        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
                        StencilPassOp: D3D11_STENCIL_OP_KEEP,
                        StencilFunc: D3D11_COMPARISON_ALWAYS,
                    },
                    ..Default::default()
                },
                Some(&mut depth),
            )?;
        }
        Ok(Self {
            device: device.clone(),
            vertex: vertex.unwrap(),
            fragment: fragment.unwrap(),
            raster: raster.unwrap(),
            depth: depth.unwrap(),
        })
    }
    fn bind(&self, context: &ID3D11DeviceContext) {
        unsafe {
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.fragment, None);
            context.RSSetState(&self.raster);
            context.OMSetDepthStencilState(&self.depth, 0);
            context.OMSetBlendState(None, None, u32::MAX);
        }
    }
}
fn compile(source: &str, entry: &str, target: &str) -> Result<Vec<u8>> {
    let entry = CString::new(entry)?;
    let target = CString::new(target)?;
    let (mut code, mut errors) = (None, None);
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR(c"xgpui-custom.hlsl".as_ptr().cast()),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        let message = errors
            .map(|blob| unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(
                    blob.GetBufferPointer().cast(),
                    blob.GetBufferSize(),
                ))
                .into_owned()
            })
            .unwrap_or_default();
        anyhow::bail!("DirectX custom shader compilation: {error}: {message}");
    }
    let code = code.context("missing compiled DirectX shader")?;
    Ok(unsafe {
        std::slice::from_raw_parts(code.GetBufferPointer().cast(), code.GetBufferSize()).to_vec()
    })
}
fn constant_buffer(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11Buffer> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= 65536,
        "invalid DirectX uniform size"
    );
    let mut padded = bytes.to_vec();
    padded.resize(bytes.len().next_multiple_of(16), 0);
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: padded.len() as u32,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: padded.as_ptr().cast(),
        ..Default::default()
    };
    let mut buffer = None;
    unsafe {
        device.CreateBuffer(&desc, Some(&data), Some(&mut buffer))?;
    }
    buffer.context("missing uniform buffer")
}

pub(crate) struct PreparedDraw {
    pub source: TextureBundle,
    pub coverage: Option<TextureBundle>,
    pub native: bool,
}
pub(crate) struct CustomCompositor {
    pipeline: CompositorPipeline,
    blend: ID3D11BlendState,
    sampler: ID3D11SamplerState,
    pub masks: std::collections::HashMap<Vec<u32>, TextureBundle>,
    pub mask_bytes: usize,
}
impl CustomCompositor {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let pipeline = CompositorPipeline::new(
            device,
            include_str!("custom_draw.hlsl"),
            "custom_vertex",
            "custom_fragment",
        )?;
        let mut desc = D3D11_BLEND_DESC::default();
        let target = &mut desc.RenderTarget[0];
        target.BlendEnable = true.into();
        target.SrcBlend = D3D11_BLEND_SRC_ALPHA;
        target.DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
        target.BlendOp = D3D11_BLEND_OP_ADD;
        target.SrcBlendAlpha = D3D11_BLEND_ONE;
        target.DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
        target.BlendOpAlpha = D3D11_BLEND_OP_ADD;
        target.RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
        let (mut blend, mut sampler) = (None, None);
        unsafe {
            device.CreateBlendState(&desc, Some(&mut blend))?;
            device.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;
        }
        Ok(Self {
            pipeline,
            blend: blend.unwrap(),
            sampler: sampler.unwrap(),
            masks: Default::default(),
            mask_bytes: 0,
        })
    }
    pub fn draw(
        &self,
        context: &ID3D11DeviceContext,
        target: &ID3D11RenderTargetView,
        viewport_size: Size<DevicePixels>,
        draws: &[CustomDraw],
        outputs: &[PreparedDraw],
    ) -> Result<()> {
        let mut deferred = None;
        unsafe {
            self.pipeline
                .device
                .CreateDeferredContext(0, Some(&mut deferred))?;
        }
        let deferred = deferred.context("missing compositor context")?;
        self.pipeline.bind(&deferred);
        unsafe {
            deferred.OMSetBlendState(&self.blend, None, u32::MAX);
            deferred.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
            deferred.RSSetViewports(Some(&[viewport(viewport_size)]));
            deferred.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
        }
        for (draw, output) in draws.iter().zip(outputs) {
            let params: [[f32; 4]; 4] = [
                [
                    draw.bounds.origin.x.0,
                    draw.bounds.origin.y.0,
                    draw.bounds.size.width.0,
                    draw.bounds.size.height.0,
                ],
                [
                    draw.content_mask.bounds.origin.x.0,
                    draw.content_mask.bounds.origin.y.0,
                    draw.content_mask.bounds.size.width.0,
                    draw.content_mask.bounds.size.height.0,
                ],
                [
                    draw.corner_radii.top_left.0,
                    draw.corner_radii.top_right.0,
                    draw.corner_radii.bottom_right.0,
                    draw.corner_radii.bottom_left.0,
                ],
                [
                    viewport_size.width.0 as f32,
                    viewport_size.height.0 as f32,
                    draw.opacity,
                    u32::from(output.coverage.is_some()) as f32
                        + if output.native { 2. } else { 0. },
                ],
            ];
            let bytes: Vec<_> = params
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            let uniform = constant_buffer(&self.pipeline.device, &bytes)?;
            let mask = output.coverage.as_ref().unwrap_or(&output.source);
            unsafe {
                deferred.VSSetConstantBuffers(0, Some(&[Some(uniform.clone())]));
                deferred.PSSetConstantBuffers(0, Some(&[Some(uniform)]));
                deferred.PSSetShaderResources(
                    0,
                    Some(&[Some(output.source.srv.clone()), Some(mask.srv.clone())]),
                );
                deferred.Draw(6, 0);
            }
        }
        let mut commands = None;
        unsafe {
            deferred.FinishCommandList(false, Some(&mut commands))?;
            context.ExecuteCommandList(commands.as_ref().context("missing compositor list")?, true);
            self.pipeline.device.GetDeviceRemovedReason()?;
        }
        Ok(())
    }
}

pub(crate) fn extent(draw: &CustomDraw) -> Result<Size<DevicePixels>> {
    ensure!(
        draw.opacity.is_finite() && (0.0..=1.0).contains(&draw.opacity),
        "invalid custom opacity"
    );
    ensure!(
        [
            draw.bounds.origin.x.0,
            draw.bounds.origin.y.0,
            draw.bounds.size.width.0,
            draw.bounds.size.height.0,
            draw.content_mask.bounds.origin.x.0,
            draw.content_mask.bounds.origin.y.0,
            draw.content_mask.bounds.size.width.0,
            draw.content_mask.bounds.size.height.0,
            draw.corner_radii.top_left.0,
            draw.corner_radii.top_right.0,
            draw.corner_radii.bottom_right.0,
            draw.corner_radii.bottom_left.0
        ]
        .iter()
        .all(|v| v.is_finite()),
        "non-finite custom geometry"
    );
    ensure!(
        draw.bounds.size.width.0 > 0.
            && draw.bounds.size.height.0 > 0.
            && draw.bounds.size.width.0 <= 16384.
            && draw.bounds.size.height.0 <= 16384.,
        "invalid custom extent"
    );
    Ok(size(
        DevicePixels(draw.bounds.size.width.0.ceil() as i32),
        DevicePixels(draw.bounds.size.height.0.ceil() as i32),
    ))
}

/// Exact normalized geometry key: clipping, screen origin and producer are excluded.
pub(crate) fn coverage_key(draw: &CustomDraw) -> Vec<u32> {
    let mut key = vec![
        draw.bounds.size.width.0.to_bits(),
        draw.bounds.size.height.0.to_bits(),
    ];
    for r in [
        draw.corner_radii.top_left,
        draw.corner_radii.top_right,
        draw.corner_radii.bottom_right,
        draw.corner_radii.bottom_left,
    ] {
        key.push(r.0.to_bits());
    }
    match draw.coverage.as_ref().expect("coverage") {
        gpui::CustomCoverage::Border { widths, style } => {
            key.extend([0, *style as u32]);
            for w in [widths.top, widths.right, widths.bottom, widths.left] {
                key.push(w.0.to_bits());
            }
        }
        gpui::CustomCoverage::Path(path) => {
            key.push(1);
            for v in &path.vertices {
                key.extend([
                    (v.xy_position.x.0 - draw.bounds.origin.x.0).to_bits(),
                    (v.xy_position.y.0 - draw.bounds.origin.y.0).to_bits(),
                    v.st_position.x.to_bits(),
                    v.st_position.y.to_bits(),
                ]);
            }
        }
    }
    key
}

pub(crate) fn coverage_scene(draw: &CustomDraw, extent: Size<DevicePixels>) -> Result<gpui::Scene> {
    use gpui::*;
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(
            ScaledPixels(extent.width.0 as f32),
            ScaledPixels(extent.height.0 as f32),
        ),
    );
    let mask = ContentMask { bounds };
    match draw.coverage.as_ref().expect("coverage") {
        CustomCoverage::Border { widths, style } => {
            ensure!(
                [widths.top.0, widths.right.0, widths.bottom.0, widths.left.0]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.),
                "invalid border widths"
            );
            scene.insert_primitive(Quad {
                bounds,
                content_mask: mask,
                background: transparent_black().into(),
                border_color: white(),
                border_widths: *widths,
                corner_radii: draw.corner_radii,
                border_style: *style,
                ..Default::default()
            });
        }
        CustomCoverage::Path(path) => {
            ensure!(path.vertices.len() <= 1_000_000, "coverage path too large");
            let mut path = path.as_ref().clone();
            let sx = bounds.size.width.0 / draw.bounds.size.width.0;
            let sy = bounds.size.height.0 / draw.bounds.size.height.0;
            for vertex in &mut path.vertices {
                vertex.xy_position.x =
                    ScaledPixels((vertex.xy_position.x.0 - draw.bounds.origin.x.0) * sx);
                vertex.xy_position.y =
                    ScaledPixels((vertex.xy_position.y.0 - draw.bounds.origin.y.0) * sy);
                ensure!(
                    [
                        vertex.xy_position.x.0,
                        vertex.xy_position.y.0,
                        vertex.st_position.x,
                        vertex.st_position.y
                    ]
                    .iter()
                    .all(|v| v.is_finite()),
                    "non-finite coverage path"
                );
                vertex.content_mask = mask;
            }
            path.bounds = bounds;
            path.content_mask = mask;
            path.color = white().into();
            scene.insert_primitive(path);
        }
    }
    scene.finish();
    Ok(scene)
}

#[cfg(test)]
mod tests;
