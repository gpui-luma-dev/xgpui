use anyhow::Result;
use gpui::{
    Bounds, ContentMask, Corners, CustomDraw, CustomGeometry, Pixels, PrimitiveBatch, Quad, Scene,
    ShaderDraw, Size, content::*, point, px, rgb, size,
};
use xgpui_shader::instanced::{ShaderProgram, presets::EffectInstance};

pub fn program() -> Result<ShaderProgram<EffectInstance>> {
    ShaderProgram::new(include_str!("rectangles.wgsl"))
}
pub fn parameters(color: [f32; 4]) -> EffectInstance {
    EffectInstance {
        color,
        effect: [4., 0., 0., 0.],
    }
}
// Shared by the UI and both headless rendering paths. Same 32x16 grid, 2px gaps,
// 4px corners, colors and single-cell size as the unchanged portable-content app.
pub fn cells(viewport: Size<Pixels>, dense: bool, cool: bool) -> Vec<(Bounds<Pixels>, [f32; 4])> {
    let (columns, rows, count) = if dense { (32, 16, 512) } else { (1, 1, 1) };
    let width = viewport.width / columns as f32;
    let height = viewport.height / rows as f32;
    (0..count)
        .map(|index| {
            let v = (index + 1) as f32 / 512.;
            (
                Bounds::new(
                    point(
                        width * (index % columns) as f32,
                        height * (index / columns) as f32,
                    ),
                    size(width - px(2.), height - px(2.)),
                ),
                if cool {
                    [0.1, v, 1., 1.]
                } else {
                    [1., v, 0.1, 1.]
                },
            )
        })
        .collect()
}
// Match the portable group's local device-pixel snapping (half ties toward zero).
pub fn snapped(bounds: Bounds<Pixels>, scale: f32) -> Bounds<Pixels> {
    let round = |v: Pixels| {
        let v = f32::from(v) * scale;
        px((v.abs() - 0.5).ceil().copysign(v) / scale)
    };
    Bounds::from_corners(
        point(round(bounds.left()), round(bounds.top())),
        point(round(bounds.right()), round(bounds.bottom())),
    )
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Path {
    PortableGroup,
    Instanced,
}
#[derive(Debug)]
struct CellSource {
    color: [f32; 4],
}
impl ContentSource for CellSource {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> std::result::Result<ImageRef, ContentError> {
        let image = cx.solid(request.extent(), ColorValue::srgb(self.color)?)?;
        cx.copy_image(image)
    }
}
pub struct Workload {
    pub program: ShaderProgram<EffectInstance>,
    group: ContentGroupId,
    // Stable immutable producers for both warm/cool palettes. Replays do not
    // regenerate source identities, while palette changes still produce new work.
    sources: [Vec<Content>; 2],
}
impl Workload {
    pub fn new() -> Result<Self> {
        Ok(Self {
            program: program()?,
            group: ContentGroupId::new(),
            sources: [false, true].map(|cool| {
                cells(size(px(1024.), px(512.)), true, cool)
                    .into_iter()
                    .map(|(_, color)| Content::new(CellSource { color }))
                    .collect()
            }),
        })
    }
    pub fn scene(&self, path: Path, width: u32, height: u32, dense: bool, cool: bool) -> Scene {
        let viewport = size(px(width as f32), px(height as f32));
        let bounds = Bounds::new(point(px(0.), px(0.)), viewport).scale(1.);
        let mask = ContentMask { bounds };
        let mut scene = Scene::default();
        scene.insert_primitive(Quad {
            bounds,
            content_mask: mask,
            background: rgb(0x101820).into(),
            ..Default::default()
        });
        let mut group = ContentGroup::new(self.group);
        for (index, (rect, color)) in cells(viewport, dense, cool).into_iter().enumerate() {
            match path {
                Path::PortableGroup => group.push(
                    CustomGeometry::RoundedRect {
                        bounds: rect,
                        corner_radii: Corners::all(px(4.)),
                    },
                    self.sources[usize::from(cool)][index].clone(),
                ),
                Path::Instanced => {
                    let p = parameters(color);
                    let mut data = [[0.; 4]; gpui::SHADER_DATA_SLOTS];
                    data[0] = p.color;
                    data[1] = p.effect;
                    scene.insert_primitive(gpui::Primitive::Shader(ShaderDraw {
                        order: 0,
                        bounds: snapped(rect, 1.).scale(1.),
                        content_mask: mask,
                        opacity: 1.,
                        scale_factor: 1.,
                        program: self.program.scene_program(),
                        glyph_mask: None,
                        path_mask: None,
                        data,
                    }));
                }
            }
        }
        if matches!(path, Path::PortableGroup) {
            scene.insert_primitive(CustomDraw {
                order: 0,
                bounds,
                content_mask: mask,
                corner_radii: Corners::default(),
                opacity: 1.,
                content: group.into_scene_content(),
                coverage: None,
            });
        }
        scene.finish();
        scene
    }
}
pub fn shader_batches(scene: &Scene) -> usize {
    scene
        .batches()
        .filter(|b| matches!(b, PrimitiveBatch::Shaders { .. }))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dense_and_single_scenes_are_one_instanced_batch() {
        let workload = Workload::new().unwrap();
        for (dense, count) in [(false, 1), (true, 512)] {
            let scene = workload.scene(Path::Instanced, 1024, 512, dense, false);
            assert_eq!(scene.shader_draws.len(), count);
            assert_eq!(shader_batches(&scene), 1);
            assert!(scene.custom_draws.is_empty());
            assert_eq!(
                scene.batches().count(),
                2,
                "one background batch plus one shader batch"
            );
        }
    }
    #[test]
    fn companion_preserves_original_layout_palette_and_gaps() {
        let grid = cells(size(px(1024.), px(512.)), true, false);
        assert_eq!(grid.len(), 512);
        assert_eq!(grid[0].0.size, size(px(30.), px(30.)));
        assert_eq!(grid[511].0.origin, point(px(992.), px(480.)));
        assert_eq!(grid[0].1, [1., 1. / 512., 0.1, 1.]);
        let single = cells(size(px(1024.), px(512.)), false, true);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].0.size, size(px(1022.), px(510.)));
        assert_eq!(single[0].1, [0.1, 1. / 512., 1., 1.]);
    }
}
