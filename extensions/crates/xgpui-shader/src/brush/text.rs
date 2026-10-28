//! Glyph coverage reuses the shape-only slots of the same 128-byte brush payload.
use super::*;
use std::sync::{Arc, Mutex};

/// A retained text brush snapshot, compiled for atlas coverage rather than shape SDF.
#[derive(Clone)]
pub(crate) struct TextBrush {
    brush: Brush,
    pub(super) program: ShaderProgram<BrushInstance>,
}
impl std::fmt::Debug for TextBrush {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextBrush")
            .field("brush", &self.brush)
            .finish()
    }
}
impl TextBrush {
    pub(crate) fn new(brush: Brush) -> Result<Self> {
        static PROGRAMS: OnceLock<Mutex<ProgramCache<u64>>> = OnceLock::new();
        let mut programs = PROGRAMS.get_or_init(Default::default).lock().unwrap();
        let program = if let Some(program) = programs.get(&brush.program.id()) {
            program.clone()
        } else {
            // Strip only analytic shape coverage; retain the validated gradient
            // and procedural material helpers from this immutable brush variant.
            let source = brush.program.source.as_ref();
            let start = source.find("fn shade(").expect("brush shape entry");
            let next = source[start..].find("\n}").expect("brush shape end") + start + 2;
            let source = format!(
                "{}\n{}\n{}",
                &source[..start],
                &source[next..],
                include_str!("text.wgsl")
            );
            let program = ShaderProgram::glyph_program(&source)?;
            programs.insert(brush.program.id(), program.clone());
            program
        };
        Ok(Self { brush, program })
    }
    pub(super) fn parameters(
        &self,
        glyph: &gpui::MonochromeSprite,
        bounds: Bounds<Pixels>,
        scale: f32,
    ) -> Result<BrushInstance> {
        ensure!(scale.is_finite() && scale > 0., "invalid glyph scale");
        let mut parameters = self.brush.instance(bounds, Corners::default())?;
        ensure!(!bounds.is_empty(), "text brush bounds must be nonempty");
        parameters.radii = [
            glyph.bounds.origin.x.0 / scale - f32::from(bounds.origin.x),
            glyph.bounds.origin.y.0 / scale - f32::from(bounds.origin.y),
            bounds.size.width.into(),
            bounds.size.height.into(),
        ];
        parameters.widths = [
            glyph.tile.bounds.origin.x.0 as f32,
            glyph.tile.bounds.origin.y.0 as f32,
            glyph.tile.bounds.size.width.0 as f32,
            glyph.tile.bounds.size.height.0 as f32,
        ];
        Ok(parameters)
    }
}
impl gpui::GlyphBrush for TextBrush {
    fn cache_key(&self) -> Arc<()> {
        self.brush.identity.clone()
    }
    fn paint_glyph(
        &self,
        window: &mut Window,
        glyph: gpui::MonochromeSprite,
        brush_bounds: Bounds<Pixels>,
        opacity: f32,
    ) -> Result<()> {
        if brush_bounds.is_empty() || opacity <= 0. {
            return Ok(());
        }
        let parameters = self.parameters(&glyph, brush_bounds, window.scale_factor())?;
        window.paint_glyph_shader(
            glyph,
            self.program.scene_program(),
            self.program.pack(&parameters)?,
            opacity,
        );
        Ok(())
    }
}
impl Brush {
    pub(crate) fn text_brush(self) -> Result<Arc<dyn gpui::GlyphBrush>> {
        Ok(Arc::new(TextBrush::new(self)?))
    }
}
