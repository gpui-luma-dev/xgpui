//! Spatial color fields within the existing portable instance payload.
use super::*;

/// Maximum independently positioned freeform points. Four or more specialize
/// a program, keeping the portable instance payload at 128 bytes.
pub const MAX_FREEFORM_POINTS: usize = 16;

/// Normalized freeform control point and straight-alpha encoded-sRGB color.
/// Positions are in [0,1] of the full receiver, independent of clipping and DPI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushPoint {
    pub position: [f32; 2],
    pub color: [f32; 4],
}
impl BrushPoint {
    pub fn new(position: [f32; 2], color: [f32; 4]) -> Self {
        Self { position, color }
    }
}

impl Brush {
    /// One to 16 inverse-square-distance control points. The field is a
    /// normalized blend of associated colors, not a biharmonic diffusion solve.
    /// Points must be separated by at least 0.0001 in normalized coordinates.
    /// Four or more points are full-precision program constants: retain the
    /// immutable brush instead of rebuilding/animating its points each frame.
    /// Only identical larger point sets share a program and instance batch.
    pub fn freeform(points: Vec<BrushPoint>) -> Result<Self> {
        ensure!(
            (1..=MAX_FREEFORM_POINTS).contains(&points.len()),
            "freeform brushes require 1..=16 points"
        );
        for (index, point) in points.iter().enumerate() {
            ensure!(
                point
                    .color
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "freeform colors must be finite encoded-sRGB RGBA in [0,1]"
            );
            ensure!(
                point
                    .position
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "freeform positions must be finite and in [0,1]"
            );
            for previous in &points[..index] {
                let dx = point.position[0] - previous.position[0];
                let dy = point.position[1] - previous.position[1];
                ensure!(
                    dx.hypot(dy) >= 0.0001,
                    "freeform points must be separated by at least 0.0001"
                );
            }
        }
        if points.len() > 3 {
            return specialized_freeform(&points);
        }
        let stops: Vec<_> = points
            .iter()
            .map(|point| BrushStop::new(0., point.color))
            .collect();
        let mut brush = Self::new(&stops, 0., [0.; 4])?;
        let second = points.get(1).unwrap_or(&points[0]);
        let third = points.get(2).unwrap_or(&points[0]);
        brush.parameters.stops = [
            points[0].position[0],
            points[0].position[1],
            second.position[0],
            second.position[1],
        ];
        brush.parameters.gradient = [third.position[0], third.position[1], 0., 0.];
        brush.parameters.options[0] = -(points.len() as f32);
        Ok(brush)
    }
    /// Regular 2x2 Hermite color patch, in row-major order: top-left, top-right,
    /// bottom-left, bottom-right. Geometry stays a unit rectangle. Zero color
    /// derivatives at corners give the same Hermite color basis as MeshGradient.
    /// This does not implement distorted geometry, folds or larger patch grids.
    pub fn mesh(colors: [[f32; 4]; 4]) -> Result<Self> {
        for color in colors {
            ensure!(
                color
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "mesh colors must be finite encoded-sRGB RGBA in [0,1]"
            );
        }
        let mut brush = Self::solid(colors[0])?;
        brush.parameters.second = colors[1];
        brush.parameters.third = colors[2];
        brush.parameters.stops = colors[3];
        brush.parameters.options[0] = -4.;
        Ok(brush)
    }
    /// Convert a 2x2 through 3x3 legacy grid without changing its opaque color field.
    /// A regular 2x2 grid shares the inline mesh program. Other accepted grids use
    /// the certified Hermite inverse; unsupported geometry returns an error.
    #[cfg(any(target_os = "macos", target_os = "windows", feature = "wgpu"))]
    pub fn from_mesh_grid(grid: &crate::MeshGrid) -> Result<Self> {
        ensure!(
            (2..=3).contains(&grid.width()) && (2..=3).contains(&grid.height()),
            "direct mesh brushes require 2..=3 rows and columns; use MeshGradient for larger grids"
        );
        let regular = grid.width() == 2
            && grid.height() == 2
            && grid.points().iter().enumerate().all(|(index, p)| {
                p.position == [(index % 2) as f32, (index / 2) as f32]
                    && p.u_tangent == [1., 0.]
                    && p.v_tangent == [0., 1.]
            });
        if regular {
            return Self::mesh(std::array::from_fn(|i| {
                let c = grid.points()[i].color;
                [c[0], c[1], c[2], 1.]
            }));
        }
        Self::hermite_mesh(
            grid.width(),
            grid.height(),
            grid.points()
                .iter()
                .map(|p| {
                    BrushMeshPoint::new(
                        p.position,
                        p.u_tangent,
                        p.v_tangent,
                        [p.color[0], p.color[1], p.color[2], 1.],
                    )
                })
                .collect(),
        )
    }
}

fn specialized_freeform(points: &[BrushPoint]) -> Result<Brush> {
    type Key = Vec<[u32; 6]>;
    static PROGRAMS: OnceLock<std::sync::Mutex<ProgramCache<Key, 16>>> = OnceLock::new();
    let key = points
        .iter()
        .map(|point| {
            [
                point.position[0],
                point.position[1],
                point.color[0],
                point.color[1],
                point.color[2],
                point.color[3],
            ]
            .map(|v| if v == 0. { 0 } else { v.to_bits() })
        })
        .collect();
    let mut programs = PROGRAMS.get_or_init(Default::default).lock().unwrap();
    let program = if let Some(program) = programs.get(&key) {
        program
    } else {
        let positions = points
            .iter()
            .map(|p| format!("vec2<f32>({:?},{:?})", p.position[0], p.position[1]))
            .collect::<Vec<_>>()
            .join(",");
        let colors = points
            .iter()
            .map(|p| {
                format!(
                    "vec4<f32>({:?},{:?},{:?},{:?})",
                    p.color[0], p.color[1], p.color[2], p.color[3]
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let count = points.len();
        let field = format!(
            r#"
const FIELD_POSITIONS: array<vec2<f32>,{count}> = array<vec2<f32>,{count}>({positions});
const FIELD_COLORS: array<vec4<f32>,{count}> = array<vec4<f32>,{count}>({colors});
fn freeform_color(uv: vec2<f32>, p: Parameters) -> vec4<f32> {{
    var distances: array<f32,{count}>;
    var nearest = 1e30;
    for (var i = 0u; i < {count}u; i += 1u) {{
        let delta = uv-FIELD_POSITIONS[i];
        let distance = dot(delta,delta);
        if (distance == 0.) {{ return straight(associated(FIELD_COLORS[i])); }}
        distances[i] = distance;
        nearest = min(nearest,distance);
    }}
    var color = vec4(0.);
    var total = 0.;
    for (var i = 0u; i < {count}u; i += 1u) {{
        let weight = nearest/distances[i];
        color += associated(FIELD_COLORS[i])*weight;
        total += weight;
    }}
    return straight(color/total);
}}
"#
        );
        let source = include_str!("brush.wgsl");
        let start = source.find("fn freeform_color(").expect("freeform entry");
        let end = source.find("fn mesh_color(").expect("mesh entry");
        let source = format!(
            "{}{field}{}{BASE_BRUSH_COLOR}",
            &source[..start],
            &source[end..]
        );
        let program = ShaderProgram::new(&source)?;
        programs.insert(key, program.clone());
        program
    };
    let mut brush = Brush::solid(points[0].color)?;
    brush.parameters.options[0] = -5.;
    brush.unmodulated = program.clone();
    brush.program = program;
    Ok(brush)
}
