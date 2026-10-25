//! Native path coverage with a main-target brush evaluation.
use super::*;
use gpui::{ContentMask, Path, ShaderDraw};
use std::sync::{Arc, Mutex};

impl Brush {
    fn path_program(&self) -> Result<ShaderProgram<BrushInstance>> {
        static PROGRAMS: OnceLock<Mutex<ProgramCache<u64>>> = OnceLock::new();
        let mut programs = PROGRAMS.get_or_init(Default::default).lock().unwrap();
        if let Some(program) = programs.get(&self.program.id()) {
            return Ok(program.clone());
        }
        let source = self.program.source.as_ref();
        let start = source.find("fn shade(").expect("brush shape entry");
        let end = source[start..].find("\n}").expect("brush shape end") + start + 2;
        let source = format!(
            "{}\n{}\nfn shade(input:ShaderInput,p:Parameters)->vec4<f32>{{return brush_color(input,p);}}",
            &source[..start],
            &source[end..]
        );
        let program = ShaderProgram::path_program(&source)?;
        programs.insert(self.program.id(), program.clone());
        Ok(program)
    }
    pub(crate) fn path_draw(
        &self,
        path: Path<Pixels>,
        clip: ContentMask<Pixels>,
        opacity: f32,
        scale: f32,
    ) -> Result<ShaderDraw> {
        ensure!(scale.is_finite() && scale > 0., "invalid path brush scale");
        ensure!(
            opacity.is_finite() && (0. ..=1.).contains(&opacity),
            "invalid path brush opacity"
        );
        ensure!(
            !path.vertices.is_empty()
                && path.vertices.len() <= 65_536
                && path.vertices.len() % 3 == 0,
            "path brush requires 1..65536 triangle vertices"
        );
        ensure!(
            path.vertices.iter().all(|v| [
                f32::from(v.xy_position.x),
                f32::from(v.xy_position.y),
                v.st_position.x,
                v.st_position.y
            ]
            .iter()
            .all(|v| v.is_finite())),
            "non-finite path brush vertex"
        );
        let parameters = self.instance(path.bounds, Corners::default())?;
        let mut path = path.scale(scale);
        path.content_mask = clip.scale(scale);
        path.color = gpui::white().into();
        let program = self.path_program()?;
        Ok(ShaderDraw {
            order: 0,
            bounds: path.bounds,
            content_mask: path.content_mask,
            opacity,
            scale_factor: scale,
            program: program.scene_program(),
            glyph_mask: None,
            path_mask: Some(Arc::new(path)),
            data: program.pack(&parameters)?,
        })
    }
    /// Apply this field to a tessellated native fill or stroke. Coordinates span
    /// its full, unclipped tessellated bounds. Reuses native path coverage and
    /// evaluates color in the main target; one coverage batch per brushed path.
    pub fn paint_path(&self, window: &mut Window, path: Path<Pixels>) -> Result<()> {
        if path.vertices.is_empty() {
            return Ok(());
        }
        // The host supplies inherited opacity; the native coverage remains white.
        let draw = self.path_draw(path, window.content_mask(), 1., window.scale_factor())?;
        window.paint_brushed_path(draw);
        Ok(())
    }
}
