//! Experimental instanced shaders: ordered, main-pass fragment shaders with typed instances.
//! Transport is limited to 128 bytes of f32 scalars/vectors for WebGL2 parity.
use anyhow::{Result, ensure};
use gpui::{Bounds, Pixels, SceneShaderProgram, ShaderOutputEncoding, Window};
use std::{marker::PhantomData, sync::Arc};
mod border;
mod compiler;
pub mod presets;
mod shadow;
pub use border::{AnimatedBorder, AnimatedBorderLayer, AnimatedBorderStyle};
pub use shadow::{ShadowLayer, ShadowPainter, ShadowStop, ShadowStyle};

/// A field in a Rust instance, used to verify the reflected WGSL ABI.
#[derive(Clone, Copy, Debug)]
pub struct InstanceField {
    /// Exact WGSL member name.
    pub name: &'static str,
    /// Byte offset (use `std::mem::offset_of!`).
    pub offset: usize,
    /// One for f32; two, three or four for f32 vectors.
    pub components: usize,
}
/// A POD parameter record with explicit, reflected field descriptions.
/// The compiler checks names, offsets, types, total size and array stride.
pub trait ShaderInstance: bytemuck::Pod {
    /// All WGSL members, including explicit padding, in declaration order.
    const FIELDS: &'static [InstanceField];
}
/// Validated portable program; clones share identity and backend pipelines.
pub struct ShaderProgram<T> {
    program: Arc<SceneShaderProgram>,
    pub(crate) source: Arc<str>,
    _instance: PhantomData<fn(T)>,
}
impl<T> Clone for ShaderProgram<T> {
    fn clone(&self) -> Self {
        Self {
            program: self.program.clone(),
            source: self.source.clone(),
            _instance: PhantomData,
        }
    }
}
impl<T: ShaderInstance> ShaderProgram<T> {
    /// Source declares `Parameters` and `fn shade(ShaderInput, Parameters) -> vec4<f32>`.
    /// Output is straight-alpha encoded-sRGB SDR. Host applies clipping/opacity.
    pub fn new(source: &str) -> Result<Self> {
        Self::with_encoding(source, ShaderOutputEncoding::EncodedSrgb)
    }
    /// Bounded straight-alpha linear Display P3/D65 output. The wrapper clamps
    /// producer channels to [0,1]; map gamut explicitly before returning them.
    /// Requires an active native WideGamutSdr window. Host associates alpha and
    /// converts to the backend composition space after producer clamping.
    pub fn new_linear_p3(source: &str) -> Result<Self> {
        Self::with_encoding(source, ShaderOutputEncoding::LinearDisplayP3)
    }
    fn with_encoding(source: &str, encoding: ShaderOutputEncoding) -> Result<Self> {
        Ok(Self {
            program: Arc::new(compiler::compile::<T>(source, encoding)?),
            source: Arc::from(source),
            _instance: PhantomData,
        })
    }
    pub(crate) fn glyph_program(source: &str) -> Result<Self> {
        Ok(Self {
            program: Arc::new(compiler::compile_glyph::<T>(source)?),
            source: Arc::from(source),
            _instance: PhantomData,
        })
    }
    pub(crate) fn path_program(source: &str) -> Result<Self> {
        Ok(Self {
            program: Arc::new(compiler::compile_path::<T>(source)?),
            source: Arc::from(source),
            _instance: PhantomData,
        })
    }
    /// Retain the compiled program for low-level GPUI scene recording.
    /// Parameter slots must match this program's validated `ShaderInstance` ABI.
    pub fn scene_program(&self) -> Arc<SceneShaderProgram> {
        self.program.clone()
    }
    /// Stable program identity, useful when examining batch diagnostics.
    pub fn id(&self) -> u64 {
        self.program.id()
    }
    pub(crate) fn pack(&self, instance: &T) -> Result<[[f32; 4]; gpui::SHADER_DATA_SLOTS]> {
        let mut slots = [[0.; 4]; gpui::SHADER_DATA_SLOTS];
        for (index, bytes) in bytemuck::bytes_of(instance).chunks_exact(4).enumerate() {
            let value = f32::from_ne_bytes(bytes.try_into().unwrap());
            ensure!(
                value.is_finite(),
                "shader instance contains non-finite data at byte {}",
                index * 4
            );
            slots[index / 4][index % 4] = value;
        }
        Ok(slots)
    }
}
/// Typed recording API. Shader instances follow GPUI draw ordering and paint replay.
pub trait WindowShaderExt {
    /// Paint the complete effect rectangle in logical pixels.
    fn paint_custom_shader<T: ShaderInstance>(
        &mut self,
        bounds: Bounds<Pixels>,
        program: &ShaderProgram<T>,
        instance: T,
    ) -> Result<()>;
}
impl WindowShaderExt for Window {
    fn paint_custom_shader<T: ShaderInstance>(
        &mut self,
        bounds: Bounds<Pixels>,
        program: &ShaderProgram<T>,
        instance: T,
    ) -> Result<()> {
        ensure!(
            [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height)
            ]
            .iter()
            .all(|v| v.is_finite()),
            "non-finite shader bounds"
        );
        if program.program.output_encoding() == ShaderOutputEncoding::LinearDisplayP3 {
            ensure!(
                self.color_status().effective() == gpui::WindowColorMode::WideGamutSdr,
                "linear P3 shaders require an active native WideGamutSdr window"
            );
        }
        let data = program.pack(&instance)?;
        self.paint_shader(bounds, program.program.clone(), data);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
