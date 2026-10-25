//! Texture-free path coverage specialized into an ordinary brush program.
use super::*;
use gpui::{ContentMask, Path, ShaderDraw};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};

pub const MAX_DIRECT_PATH_TRIANGLES: usize = 512;
pub const MAX_DIRECT_PATH_EDGES: usize = 256;
/// Largest supported width or height in device pixels.
pub const MAX_DIRECT_PATH_EXTENT: f32 = 16_384.;
type Key = (u64, Vec<[u32; 2]>);
#[derive(Default)]
struct DirectPrograms {
    valid: ProgramCache<Key, 16>,
    unsupported: VecDeque<Key>,
}
/// Immutable direct-path material and geometry. Retain this value to move a path
/// without re-tessellation, geometry certification or shader specialization.
#[derive(Clone)]
pub struct DirectPath {
    template: ShaderDraw,
    bounds: Bounds<Pixels>,
}
impl DirectPath {
    pub(crate) fn draw(
        &self,
        offset: gpui::Point<Pixels>,
        clip: ContentMask<Pixels>,
        opacity: f32,
        scale: f32,
    ) -> Result<ShaderDraw> {
        ensure!(
            scale.is_finite() && scale > 0. && opacity.is_finite() && (0. ..=1.).contains(&opacity),
            "invalid direct path scale/opacity"
        );
        let mut bounds = self.bounds;
        bounds.origin += offset;
        ensure!(
            [f32::from(bounds.origin.x), f32::from(bounds.origin.y)]
                .iter()
                .all(|v| v.is_finite()),
            "invalid direct path offset"
        );
        ensure!(
            f32::from(bounds.size.width) * scale <= MAX_DIRECT_PATH_EXTENT
                && f32::from(bounds.size.height) * scale <= MAX_DIRECT_PATH_EXTENT,
            "direct path extent exceeds 16384 device pixels"
        );
        let pad = 1. / scale;
        ensure!(pad.is_finite(), "invalid direct path scale");
        let mut draw = self.template.clone();
        draw.bounds = bounds.dilate(gpui::px(pad)).scale(scale);
        draw.content_mask = clip.scale(scale);
        draw.opacity = opacity;
        draw.scale_factor = scale;
        const RADII_SLOT: usize = std::mem::offset_of!(BrushInstance, radii) / 16;
        draw.data[RADII_SLOT] = [
            pad,
            pad,
            bounds.size.width.into(),
            bounds.size.height.into(),
        ];
        Ok(draw)
    }
    /// Paint relative to the original path coordinates plus a logical offset.
    /// Inherited opacity and the current host clip are applied exactly once.
    pub fn paint(&self, window: &mut Window, offset: gpui::Point<Pixels>) -> Result<()> {
        let draw = self.draw(offset, window.content_mask(), 1., window.scale_factor())?;
        window.paint_brushed_path(draw);
        Ok(())
    }
}
type Position = [f32; 2];
type Triangle = [Position; 3];
fn cross(a: Position, b: Position, c: Position) -> f64 {
    (b[0] as f64 - a[0] as f64) * (c[1] as f64 - a[1] as f64)
        - (b[1] as f64 - a[1] as f64) * (c[0] as f64 - a[0] as f64)
}
fn overlap(a: &Triangle, b: &Triangle) -> bool {
    for triangle in [a, b] {
        for i in 0..3 {
            let p = triangle[i];
            let q = triangle[(i + 1) % 3];
            let axis = [q[1] as f64 - p[1] as f64, p[0] as f64 - q[0] as f64];
            let project = |t: &Triangle| {
                t.iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), v| {
                        let d = v[0] as f64 * axis[0] + v[1] as f64 * axis[1];
                        (low.min(d), high.max(d))
                    })
            };
            let (al, ah) = project(a);
            let (bl, bh) = project(b);
            // Dot products along a shared edge can differ by a few f64 ulps.
            let epsilon =
                32. * f64::EPSILON * ah.abs().max(al.abs()).max(bh.abs()).max(bl.abs()).max(1.);
            if ah.min(bh) - al.max(bl) <= epsilon {
                return false;
            }
        }
    }
    true
}
fn boundary(triangles: &[Triangle]) -> Option<Vec<[f32; 4]>> {
    let mut edges = BTreeMap::<([u32; 2], [u32; 2]), i32>::new();
    let mut solid = Vec::new();
    for triangle in triangles {
        let mut triangle = *triangle;
        let area = cross(triangle[0], triangle[1], triangle[2]);
        if area == 0. {
            continue;
        }
        if area < 0. {
            triangle.swap(1, 2);
        }
        for i in 0..3 {
            let a = triangle[i].map(|v| if v == 0. { 0 } else { v.to_bits() });
            let b = triangle[(i + 1) % 3].map(|v| if v == 0. { 0 } else { v.to_bits() });
            let (key, sign) = if a < b { ((a, b), 1) } else { ((b, a), -1) };
            *edges.entry(key).or_default() += sign;
        }
        solid.push(triangle);
    }
    edges.retain(|_, count| *count != 0);
    if edges.is_empty()
        || edges.len() > MAX_DIRECT_PATH_EDGES
        || edges.values().any(|n| n.abs() != 1)
    {
        return None;
    }
    // An area sum is exact only for disjoint triangle interiors. Overlapping
    // stroke joins/self-intersections use native coverage rather than overcount.
    for (i, a) in solid.iter().enumerate() {
        let bounds = |t: &Triangle, axis: usize| {
            t.iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), p| {
                    (l.min(p[axis]), h.max(p[axis]))
                })
        };
        let ax = bounds(a, 0);
        let ay = bounds(a, 1);
        for b in &solid[..i] {
            let bx = bounds(b, 0);
            let by = bounds(b, 1);
            if ax.1 > bx.0 && bx.1 > ax.0 && ay.1 > by.0 && by.1 > ay.0 && overlap(a, b) {
                return None;
            }
        }
    }
    Some(
        edges
            .into_iter()
            .map(|((a, b), n)| {
                let (a, b) = if n > 0 { (a, b) } else { (b, a) };
                [
                    f32::from_bits(a[0]),
                    f32::from_bits(a[1]),
                    f32::from_bits(b[0]),
                    f32::from_bits(b[1]),
                ]
            })
            .collect(),
    )
}
impl Brush {
    pub(crate) fn direct_path_draw(
        &self,
        path: &Path<Pixels>,
        clip: ContentMask<Pixels>,
        opacity: f32,
        scale: f32,
    ) -> Result<Option<ShaderDraw>> {
        super::path::validate_path(path, opacity, scale)?;
        let mut parameters = self.instance(path.bounds, Corners::default())?;
        if path.vertices.len() / 3 > MAX_DIRECT_PATH_TRIANGLES
            || path
                .vertices
                .iter()
                .any(|v| v.st_position != gpui::point(0., 1.))
        {
            return Ok(None);
        }
        let w = f32::from(path.bounds.size.width);
        let h = f32::from(path.bounds.size.height);
        if w <= 0.
            || h <= 0.
            || w * scale > MAX_DIRECT_PATH_EXTENT
            || h * scale > MAX_DIRECT_PATH_EXTENT
        {
            return Ok(None);
        }
        let positions: Vec<Position> = path
            .vertices
            .iter()
            .map(|v| {
                [
                    ((f32::from(v.xy_position.x) as f64 - f32::from(path.bounds.origin.x) as f64)
                        / w as f64) as f32,
                    ((f32::from(v.xy_position.y) as f64 - f32::from(path.bounds.origin.y) as f64)
                        / h as f64) as f32,
                ]
            })
            .collect();
        static PROGRAMS: OnceLock<Mutex<DirectPrograms>> = OnceLock::new();
        let key = (
            self.program.id(),
            positions
                .iter()
                .map(|p| p.map(|v| if v == 0. { 0 } else { v.to_bits() }))
                .collect(),
        );
        let mut programs = PROGRAMS.get_or_init(Default::default).lock().unwrap();
        if programs.unsupported.contains(&key) {
            return Ok(None);
        }
        let program = if let Some(program) = programs.valid.get(&key) {
            program
        } else {
            let triangles: Vec<Triangle> = positions
                .chunks_exact(3)
                .map(|v| [v[0], v[1], v[2]])
                .collect();
            let Some(edges) = boundary(&triangles) else {
                programs.unsupported.push_back(key);
                if programs.unsupported.len() > 16 {
                    programs.unsupported.pop_front();
                }
                return Ok(None);
            };
            let constants = edges
                .iter()
                .map(|e| format!("vec4<f32>({:?},{:?},{:?},{:?})", e[0], e[1], e[2], e[3]))
                .collect::<Vec<_>>()
                .join(",");
            let source = self.program.source.as_ref();
            let start = source.find("fn shade(").expect("brush shape entry");
            let end = source[start..].find("\n}").expect("brush shape end") + start + 2;
            let n = edges.len();
            let source = format!(
                "{}\n{}\nconst PATH_EDGE_COUNT:u32={n}u;\nconst PATH_EDGES:array<vec4<f32>,{n}>=array<vec4<f32>,{n}>({constants});\n{}",
                &source[..start],
                &source[end..],
                include_str!("direct_path.wgsl")
            );
            let program = ShaderProgram::new(&source)?;
            programs.valid.insert(key, program.clone());
            program
        };
        let pad = 1. / scale;
        ensure!(pad.is_finite(), "invalid direct path scale");
        parameters.radii = [pad, pad, w, h];
        Ok(Some(ShaderDraw {
            order: 0,
            bounds: path.bounds.dilate(gpui::px(pad)).scale(scale),
            content_mask: clip.scale(scale),
            opacity,
            scale_factor: scale,
            program: program.scene_program(),
            glyph_mask: None,
            path_mask: None,
            data: program.pack(&parameters)?,
        }))
    }
    /// Prepare immutable direct geometry once, then paint it at different offsets.
    /// Unsupported geometry returns an error and never introduces a coverage pass.
    pub fn prepare_path(&self, path: &Path<Pixels>) -> Result<DirectPath> {
        let bounds = path.bounds;
        let template = self.direct_path_draw(path,ContentMask {bounds:bounds.dilate(gpui::px(1.))},1.,1.)?
            .ok_or_else(||anyhow::anyhow!("direct path requires non-overlapping solid triangles within the 512-triangle / 256-edge limits"))?;
        Ok(DirectPath { template, bounds })
    }
    /// Render supported tessellated paths without coverage textures or pass breaks.
    /// Returns an error for overlapping/analytic triangles or geometry exceeding
    /// 512 triangles / 256 uncancelled boundary edges. No native fallback occurs.
    pub fn paint_path_direct(&self, window: &mut Window, path: Path<Pixels>) -> Result<()> {
        if path.vertices.is_empty() {
            return Ok(());
        }
        let draw = self.direct_path_draw(&path,window.content_mask(),1.,window.scale_factor())?
            .ok_or_else(||anyhow::anyhow!("direct path requires non-overlapping solid triangles within the 512-triangle / 256-edge limits"))?;
        window.paint_brushed_path(draw);
        Ok(())
    }
}
