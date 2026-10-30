//! Experimental shader paints: one WGSL function and reflected f32 parameters.
//! Output is straight-alpha encoded-sRGB SDR, matching the existing paint path.
//! The source and output contracts remain unchanged by the purpose-based API names.
use crate::Paint;
use anyhow::{Context as _, Result, bail, ensure};
use gpui::{CustomContent, DevicePixels, PreparationRequest, Size, px, size};
use std::{collections::BTreeMap, sync::Arc};

#[cfg(target_os = "macos")]
mod metal;
mod validation;

/// Named scalar/vector values. Repeated setters replace the previous value.
/// Types, names and finite values are checked when creating a paint snapshot.
#[derive(Clone, Debug, Default)]
pub struct ShaderParameters(BTreeMap<String, Vec<f32>>);
impl ShaderParameters {
    /// Start an empty parameter set.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set an f32 field.
    pub fn float(mut self, name: impl Into<String>, value: f32) -> Self {
        self.0.insert(name.into(), vec![value]);
        self
    }
    /// Set a vec2<f32> field.
    pub fn vec2(mut self, name: impl Into<String>, value: [f32; 2]) -> Self {
        self.0.insert(name.into(), value.to_vec());
        self
    }
    /// Set a vec3<f32> field.
    pub fn vec3(mut self, name: impl Into<String>, value: [f32; 3]) -> Self {
        self.0.insert(name.into(), value.to_vec());
        self
    }
    /// Set a vec4<f32> field.
    pub fn vec4(mut self, name: impl Into<String>, value: [f32; 4]) -> Self {
        self.0.insert(name.into(), value.to_vec());
        self
    }
}

#[derive(Debug)]
struct Field {
    name: String,
    offset: usize,
    components: usize,
}

struct Program {
    #[cfg(any(feature = "wgpu", target_os = "windows"))]
    source: String,
    fields: Vec<Field>,
    uniform_size: usize,
    #[cfg(target_os = "macos")]
    msl: String,
    #[cfg(target_os = "macos")]
    metal_entries: [String; 2],
    #[cfg(target_os = "macos")]
    metal_pipeline: metal::PipelineCache,
    #[cfg(feature = "wgpu")]
    wgpu: crate::wgpu::PaintCache,
    #[cfg(target_os = "windows")]
    dx11: crate::dx11::PaintCache,
}

/// Validated portable source, reflection and shared backend pipelines.
#[derive(Clone)]
pub struct ShaderProgram(Arc<Program>);
impl std::fmt::Debug for ShaderProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShaderProgram")
            .field("contract", &1)
            .field("fields", &self.0.fields)
            .finish_non_exhaustive()
    }
}
impl ShaderProgram {
    /// Validate the experimental shader-paint subset and translate to MSL, HLSL and SPIR-V.
    /// User source declares `Parameters` and `fn paint(input: PaintInput) -> vec4<f32>`.
    /// The extension supplies `parameters`, resource bindings and entry points.
    pub fn new(source: &str) -> Result<Self> {
        Ok(Self(Arc::new(validation::compile(source)?)))
    }

    /// Pack named fields using reflected WGSL offsets; return an immutable paint.
    pub fn paint(&self, parameters: ShaderParameters) -> Result<ShaderPaint> {
        for name in parameters.0.keys() {
            ensure!(
                self.0.fields.iter().any(|field| &field.name == name),
                "unknown shader parameter `{name}`"
            );
        }
        let mut bytes = vec![0_u8; self.0.uniform_size];
        for field in &self.0.fields {
            let values = parameters
                .0
                .get(&field.name)
                .with_context(|| format!("missing shader parameter `{}`", field.name))?;
            ensure!(
                values.len() == field.components,
                "wrong type for shader parameter `{}`: expected {} f32 components",
                field.name,
                field.components
            );
            ensure!(
                values.iter().all(|v| v.is_finite()),
                "shader parameter `{}` must be finite",
                field.name
            );
            for (i, value) in values.iter().enumerate() {
                bytes[field.offset + i * 4..field.offset + i * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        Ok(ShaderPaint(Arc::new(Snapshot {
            program: self.clone(),
            bytes,
            #[cfg(target_os = "macos")]
            metal: Default::default(),
            #[cfg(feature = "wgpu")]
            wgpu: self.0.wgpu.snapshot(),
            #[cfg(target_os = "windows")]
            dx11: self.0.dx11.snapshot(),
        })))
    }
}

struct Snapshot {
    program: ShaderProgram,
    bytes: Vec<u8>,
    #[cfg(target_os = "macos")]
    metal: metal::OutputCache,
    #[cfg(feature = "wgpu")]
    wgpu: crate::wgpu::PaintCache,
    #[cfg(target_os = "windows")]
    dx11: crate::dx11::PaintCache,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShaderPaint")
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

/// Cloneable immutable paint. Clones share preparation caches and retain source.
#[derive(Clone, Debug)]
pub struct ShaderPaint(Arc<Snapshot>);
impl From<ShaderPaint> for Paint {
    fn from(paint: ShaderPaint) -> Self {
        Paint::Custom(paint.0)
    }
}
impl CustomContent for ShaderPaint {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        extent: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        self.0.prepare(backend, extent)
    }
    fn prepare_with_request(
        &self,
        backend: &dyn std::any::Any,
        request: &PreparationRequest,
    ) -> Result<Box<dyn std::any::Any>> {
        self.0.prepare_with_request(backend, request)
    }
}
impl CustomContent for Snapshot {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        extent: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        let request = PreparationRequest::new(
            size(px(extent.width.0 as f32), px(extent.height.0 as f32)),
            extent,
            1.,
        )?;
        self.prepare_with_request(backend, &request)
    }
    fn prepare_with_request(
        &self,
        backend: &dyn std::any::Any,
        request: &PreparationRequest,
    ) -> Result<Box<dyn std::any::Any>> {
        #[cfg(target_os = "macos")]
        if let Some(context) = backend.downcast_ref::<gpui_apple::custom_draw::MetalCustomContext>()
        {
            return self.prepare_metal(context, request);
        }
        #[cfg(target_os = "windows")]
        if let Some(context) =
            backend.downcast_ref::<gpui_windows::custom_draw::Dx11CustomContext>()
        {
            let geometry = self.geometry(request);
            let bytes: Vec<_> = geometry.iter().flat_map(|v| v.to_le_bytes()).collect();
            let (output, _) = self.dx11.render_uniforms(
                context,
                request.raster_size(),
                &self.program.0.source,
                &[&bytes, &self.bytes],
                6,
                Some([geometry[0].to_bits(), geometry[1].to_bits()]),
            )?;
            return Ok(Box::new(output));
        }
        #[cfg(feature = "wgpu")]
        if let Some(context) = backend.downcast_ref::<gpui_wgpu::custom_draw::WgpuCustomContext>() {
            let geometry = self.geometry(request);
            let geometry_bytes: Vec<_> = geometry.iter().flat_map(|v| v.to_le_bytes()).collect();
            let uniforms: &[&[u8]] = &[&geometry_bytes, &self.bytes];
            let (output, _) = self.wgpu.render_draw(
                context,
                crate::wgpu::DrawRequest {
                    extent: request.raster_size(),
                    source: &self.program.0.source,
                    uniforms,
                    vertices: 6,
                    logical: Some([
                        f32::from(request.logical_size().width).to_bits(),
                        f32::from(request.logical_size().height).to_bits(),
                    ]),
                },
            )?;
            return Ok(Box::new(output));
        }
        bail!("portable shader requires an implemented Metal, DirectX 11 or wgpu adapter")
    }
}
impl Snapshot {
    fn geometry(&self, request: &PreparationRequest) -> [f32; 4] {
        [
            f32::from(request.logical_size().width),
            f32::from(request.logical_size().height),
            request.raster_size().width.0 as f32,
            request.raster_size().height.0 as f32,
        ]
    }
}

#[cfg(test)]
mod tests;
