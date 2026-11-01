//! CPU-only procedural WGSL paints built on the portable content API.
use gpui::content::*;
use std::sync::Arc;
mod validation;
type Result<T> = std::result::Result<T, ContentError>;
fn interpretation() -> ImageInterpretation {
    ImageInterpretation {
        color: ColorEncoding::Srgb,
        alpha: AlphaMode::Straight,
    }
}
#[derive(Debug)]
struct Program {
    portable: PortableProgram,
}
/// Validated procedural source and reflected parameters. Native compilation,
/// allocation, scheduling and caching belong to the portable content host.
#[derive(Clone, Debug)]
pub struct ShaderProgram(Arc<Program>);
impl ShaderProgram {
    /// Declare `Parameters` and `fn paint(input: PaintInput) -> vec4<f32>`.
    /// PaintInput contains normalized UV, logical pixel position and logical size.
    /// Output is clamped straight-alpha encoded-sRGB SDR. Resources/entry points
    /// are supplied by this wrapper; use ContentSource for multi-resource graphs.
    pub fn new(source: &str) -> Result<Self> {
        validation::compile(source)
            .map(|program| Self(Arc::new(program)))
            .map_err(|e| ContentError::new(ContentErrorKind::ShaderValidation, format!("{e:#}")))
    }
    /// Declared parameter fields, types and packing layout, including unused fields.
    pub fn parameter_layout(&self) -> &UniformLayout {
        self.0
            .portable
            .uniform_layout("parameters")
            .expect("validated parameter declaration")
    }
    /// Validate typed values using the core reflected layout, including unused
    /// fields, then create a fresh immutable content snapshot.
    pub fn paint(&self, parameters: UniformValues) -> Result<ShaderPaint> {
        self.parameter_layout().pack(&parameters)?;
        let source = Snapshot {
            program: self.clone(),
            parameters,
        };
        let content = Content::new(source.clone());
        Ok(ShaderPaint { source, content })
    }
}
#[derive(Clone, Debug)]
struct Snapshot {
    program: ShaderProgram,
    parameters: UniformValues,
}
/// Immutable portable producer. Clones retain identity; each paint() creates a
/// new snapshot. Retain a destination presentation slot across parameter edits.
#[derive(Clone, Debug)]
pub struct ShaderPaint {
    source: Snapshot,
    content: Content,
}
impl ShaderPaint {
    pub fn content(&self) -> Content {
        self.content.clone()
    }
}
impl ContentSource for Snapshot {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest) -> Result<ImageRef> {
        let program = &self.program.0.portable;
        let mut pass = RenderPass::fullscreen(
            program,
            ImageDescriptor::new(request.extent(), PixelFormat::Rgba8, interpretation(), 1)?,
        );
        if program.binding("parameters").is_some() {
            pass = pass.uniform("parameters", self.parameters.clone());
        }
        if program.binding("xgpui_geometry").is_some() {
            let logical = request.logical_size();
            pass = pass.uniform(
                "xgpui_geometry",
                UniformValues::new().vec4(
                    "dimensions",
                    [
                        f32::from(logical.width),
                        f32::from(logical.height),
                        request.extent().width() as f32,
                        request.extent().height() as f32,
                    ],
                ),
            );
        }
        cx.render(pass)
    }
}
impl ContentSource for ShaderPaint {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest) -> Result<ImageRef> {
        self.source.describe(cx, request)
    }
}
impl From<ShaderPaint> for Content {
    fn from(paint: ShaderPaint) -> Self {
        paint.content
    }
}
impl From<ShaderPaint> for crate::Paint {
    fn from(paint: ShaderPaint) -> Self {
        paint.content.into()
    }
}
#[cfg(test)]
mod tests;
