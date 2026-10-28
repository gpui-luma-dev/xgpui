//! A bounded, injective bicubic geometry patch evaluated in the main pass.
use super::*;
use std::sync::Mutex;

/// Full-precision position, geometry derivatives and straight encoded-sRGB RGBA.
/// Grid points are row-major; a single patch is TL, TR, BL, BR.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushMeshPoint {
    pub position: [f32; 2],
    pub u_tangent: [f32; 2],
    pub v_tangent: [f32; 2],
    pub color: [f32; 4],
}
impl BrushMeshPoint {
    pub fn new(
        position: [f32; 2],
        u_tangent: [f32; 2],
        v_tangent: [f32; 2],
        color: [f32; 4],
    ) -> Self {
        Self {
            position,
            u_tangent,
            v_tangent,
            color,
        }
    }
}
impl Brush {
    /// A single distorted 2x2 Hermite patch, with zero mixed geometry derivatives
    /// and zero color derivatives. The receiver maps to normalized mesh space.
    /// Empty regions outside the curved patch are transparent and antialiased.
    ///
    /// A conservative Bezier-hull certificate requires ||I-J||_infinity <= 0.5
    /// throughout the patch. Folds, singular and severely distorted patches
    /// return an error; use MeshGradient for unsupported geometry.
    /// Retain this brush: editing points specializes a new program.
    pub fn hermite_patch(points: [BrushMeshPoint; 4]) -> Result<Self> {
        Self::hermite_mesh(2, 2, points.to_vec())
    }
    /// A shared 2x2 through 3x3 control grid (up to four bicubic patches).
    /// Geometry tangents are patch-local derivatives, as in MeshGrid. Adjacent
    /// patches share their corner positions, colors and U/V derivatives.
    /// The global inverse is certified across the entire normalized domain;
    /// only the outer mesh boundary is antialiased, so joins do not fade.
    pub fn hermite_mesh(width: u32, height: u32, points: Vec<BrushMeshPoint>) -> Result<Self> {
        ensure!(
            (2..=3).contains(&width) && (2..=3).contains(&height),
            "direct Hermite meshes require 2..=3 rows and columns"
        );
        ensure!(
            points.len() == (width * height) as usize,
            "Hermite point count does not match dimensions"
        );
        let mut geometry = Vec::new();
        let mut colors = Vec::new();
        for y in 0..height - 1 {
            for x in 0..width - 1 {
                let i = (y * width + x) as usize;
                let patch = [
                    points[i],
                    points[i + 1],
                    points[i + width as usize],
                    points[i + width as usize + 1],
                ];
                geometry.extend(coefficients(
                    &patch,
                    [(width - 1) as f64, (height - 1) as f64],
                )?);
                colors.extend(patch.map(|p| p.color));
            }
        }
        type Key = (u32, u32, Vec<[u32; 10]>);
        static PROGRAMS: OnceLock<Mutex<ProgramCache<Key, 16>>> = OnceLock::new();
        let key = (
            width,
            height,
            points
                .iter()
                .map(|p| {
                    [
                        p.position[0],
                        p.position[1],
                        p.u_tangent[0],
                        p.u_tangent[1],
                        p.v_tangent[0],
                        p.v_tangent[1],
                        p.color[0],
                        p.color[1],
                        p.color[2],
                        p.color[3],
                    ]
                    .map(|v| if v == 0. { 0 } else { v.to_bits() })
                })
                .collect(),
        );
        let mut programs = PROGRAMS.get_or_init(Default::default).lock().unwrap();
        let program = if let Some(program) = programs.get(&key) {
            program
        } else {
            let coefficients = geometry
                .iter()
                .map(|p| format!("vec2<f32>({:?},{:?})", p[0], p[1]))
                .collect::<Vec<_>>()
                .join(",");
            let rgba = colors
                .iter()
                .map(|c| format!("vec4<f32>({:?},{:?},{:?},{:?})", c[0], c[1], c[2], c[3]))
                .collect::<Vec<_>>()
                .join(",");
            let source = include_str!("brush.wgsl");
            let start = source.find("fn mesh_color(").expect("mesh entry");
            let end = source.find("fn base_color(").expect("base entry");
            let n = geometry.len();
            let m = colors.len();
            let columns = width - 1;
            let rows = height - 1;
            let source = format!(
                "{}\nconst PATCH_COLUMNS:u32={columns}u;\nconst PATCH_ROWS:u32={rows}u;\nconst PATCH_COEFFICIENTS:array<vec2<f32>,{n}>=array<vec2<f32>,{n}>({coefficients});\nconst PATCH_COLORS:array<vec4<f32>,{m}>=array<vec4<f32>,{m}>({rgba});\n{}\n{}{BASE_BRUSH_COLOR}",
                &source[..start],
                include_str!("hermite.wgsl"),
                &source[end..]
            );
            let program = ShaderProgram::new(&source)?;
            programs.insert(key, program.clone());
            program
        };
        let mut brush = Self::solid(points[0].color)?;
        brush.parameters.options[0] = -4.;
        brush.program = program.clone();
        brush.unmodulated = program;
        Ok(brush)
    }
}

/// Convert zero-cross-derivative Hermite geometry to a Bezier control net, then
/// bound derivatives by the convex hull of its Bernstein derivative controls.
fn coefficients(points: &[BrushMeshPoint; 4], scale: [f64; 2]) -> Result<[[f32; 2]; 16]> {
    for p in points {
        ensure!(
            p.position
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "patch positions must be finite and in [0,1]"
        );
        ensure!(
            p.u_tangent
                .iter()
                .chain(&p.v_tangent)
                .all(|v| v.is_finite() && (-2. ..=2.).contains(v)),
            "patch tangents must be finite and in [-2,2]"
        );
        ensure!(
            p.color
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "patch colors must be finite encoded-sRGB RGBA in [0,1]"
        );
    }
    let mut bezier = [[[0_f64; 2]; 4]; 4];
    for y in 0..4 {
        for x in 0..4 {
            let p = points[usize::from(x >= 2) + usize::from(y >= 2) * 2];
            let u = match x {
                1 => 1. / 3.,
                2 => -1. / 3.,
                _ => 0.,
            };
            let v = match y {
                1 => 1. / 3.,
                2 => -1. / 3.,
                _ => 0.,
            };
            for axis in 0..2 {
                bezier[y][x][axis] = p.position[axis] as f64
                    + p.u_tangent[axis] as f64 * u
                    + p.v_tangent[axis] as f64 * v;
            }
        }
    }
    let mut bounds = [[0_f64; 2]; 2];
    for y in 0..4 {
        for x in 0..3 {
            for axis in 0..2 {
                let derivative = 3. * (bezier[y][x + 1][axis] - bezier[y][x][axis]) * scale[0];
                bounds[axis][0] =
                    bounds[axis][0].max((derivative - if axis == 0 { 1. } else { 0. }).abs());
            }
        }
    }
    for y in 0..3 {
        for x in 0..4 {
            for axis in 0..2 {
                let derivative = 3. * (bezier[y + 1][x][axis] - bezier[y][x][axis]) * scale[1];
                bounds[axis][1] =
                    bounds[axis][1].max((derivative - if axis == 1 { 1. } else { 0. }).abs());
            }
        }
    }
    ensure!(
        bounds.iter().all(|row| row[0] + row[1] <= 0.5),
        "patch distortion exceeds the certified inverse bound; use MeshGradient for stronger distortion or folds"
    );
    let basis = [
        [1., 0., 0., 0.],
        [-3., 3., 0., 0.],
        [3., -6., 3., 0.],
        [-1., 3., -3., 1.],
    ];
    let mut result = [[0_f32; 2]; 16];
    for v in 0..4 {
        for u in 0..4 {
            for axis in 0..2 {
                let mut coefficient = 0.;
                for y in 0..4 {
                    for x in 0..4 {
                        coefficient += basis[u][x] * basis[v][y] * bezier[y][x][axis];
                    }
                }
                result[v * 4 + u][axis] = coefficient as f32;
            }
        }
    }
    Ok(result)
}
