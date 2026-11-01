//! Application-side portable paint example. No OS or backend conditionals.
use anyhow::Result;
use gpui::content::UniformValues;
use xgpui_shader::shader_paint::{ShaderPaint, ShaderProgram};

pub fn program() -> Result<ShaderProgram> {
    Ok(ShaderProgram::new(include_str!(
        "shaders/portable-wash.wgsl"
    ))?)
}
pub fn paint(program: &ShaderProgram, progress: f32, alpha: f32) -> Result<ShaderPaint> {
    Ok(program.paint(
        UniformValues::new()
            .float("progress", progress)
            .vec3("tint", [0.95, 0.15, 0.35])
            .float("alpha", alpha),
    )?)
}

/// Retained procedural state for interactive parameter changes.
pub struct LivePaint {
    program: ShaderProgram,
    pub paint: ShaderPaint,
    values: [f32; 5],
}
impl LivePaint {
    pub fn new() -> Self {
        let program = program().unwrap();
        let paint = paint(&program, 0., 1.).unwrap();
        Self {
            program,
            paint,
            values: [f32::NAN; 5],
        }
    }
    pub fn update(&mut self, progress: f32, color: [f32; 4]) {
        let values = [progress, color[0], color[1], color[2], color[3]];
        if self.values != values {
            self.paint = self
                .program
                .paint(
                    UniformValues::new()
                        .float("progress", progress)
                        .vec3("tint", [color[0], color[1], color[2]])
                        .float("alpha", color[3]),
                )
                .unwrap();
            self.values = values;
        }
    }
}
