//! Hermite mesh gradient adapted from patskovn/MeshGradient (MIT).
//! See LICENSE-MeshGradient for the upstream notice and README for changes.
use anyhow::{Result, ensure};
use gpui::content::*;
use std::sync::{Arc, OnceLock};
/// A normalized top-left-origin control point. Tangents are derivatives over a
/// single patch, not a global grid. RGB channels are encoded SDR sRGB; mesh
/// color is opaque and element opacity is applied by the GPUI compositor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlPoint {
    pub position: [f32; 2],
    pub u_tangent: [f32; 2],
    pub v_tangent: [f32; 2],
    pub color: [f32; 3],
}

/// Validated row-major grid. Shared control points keep adjacent patch edges
/// continuous. Folds/overlaps are permitted; later triangles overwrite earlier
/// ones, and regions outside the mesh are transparent.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshGrid {
    width: u32,
    height: u32,
    points: Vec<ControlPoint>,
}
impl MeshGrid {
    /// Grid dimensions are 2..=16. Coordinates/tangents must be finite and in
    /// -16..=16; colors must be in 0..=1. Positions outside the unit square are
    /// allowed for distortion, but are clipped by the offscreen target.
    pub fn new(width: u32, height: u32, points: Vec<ControlPoint>) -> Result<Self> {
        ensure!(
            (2..=16).contains(&width) && (2..=16).contains(&height),
            "mesh dimensions must be 2..=16"
        );
        ensure!(
            points.len() == (width * height) as usize,
            "mesh point count does not match dimensions"
        );
        for point in &points {
            ensure!(
                point
                    .position
                    .iter()
                    .chain(&point.u_tangent)
                    .chain(&point.v_tangent)
                    .all(|v| v.is_finite() && (-16.0..=16.0).contains(v)),
                "mesh geometry must be finite and in -16..=16"
            );
            ensure!(
                point
                    .color
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "mesh RGB must be finite and in 0..=1"
            );
        }
        Ok(Self {
            width,
            height,
            points,
        })
    }
    /// Build a regular unit-square grid with tangents yielding straight edges.
    pub fn regular(width: u32, height: u32, colors: Vec<[f32; 3]>) -> Result<Self> {
        ensure!(
            (2..=16).contains(&width) && (2..=16).contains(&height),
            "mesh dimensions must be 2..=16"
        );
        ensure!(
            colors.len() == (width * height) as usize,
            "mesh color count does not match dimensions"
        );
        let points = colors
            .into_iter()
            .enumerate()
            .map(|(i, color)| ControlPoint {
                position: [
                    (i as u32 % width) as f32 / (width - 1) as f32,
                    (i as u32 / width) as f32 / (height - 1) as f32,
                ],
                u_tangent: [1. / (width - 1) as f32, 0.],
                v_tangent: [0., 1. / (height - 1) as f32],
                color,
            })
            .collect();
        Self::new(width, height, points)
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn points(&self) -> &[ControlPoint] {
        &self.points
    }
}

/// Tessellation costs, excluding the output texture and driver allocations.
#[derive(Clone, Copy, Debug)]
pub struct MeshStats {
    pub patches: u32,
    /// Unique patch-local samples; the compute pass expands these into triangles.
    pub evaluated_vertices: u32,
    pub triangles: u32,
    pub buffer_bytes: u64,
}
impl MeshStats {
    fn new(grid: &MeshGrid, samples: u32) -> Result<Self> {
        ensure!(
            (2..=64).contains(&samples),
            "mesh subdivisions (samples per edge) must be 2..=64"
        );
        let patches = (grid.width - 1) * (grid.height - 1);
        let evaluated_vertices = patches * samples * samples;
        let triangles = patches * (samples - 1) * (samples - 1) * 2;
        let buffer_bytes = grid.points.len() as u64 * 48 + triangles as u64 * 3 * 32;
        ensure!(
            buffer_bytes <= 64 * 1024 * 1024,
            "mesh geometry exceeds the 64 MiB per-generation budget"
        );
        Ok(Self {
            patches,
            evaluated_vertices,
            triangles,
            buffer_bytes,
        })
    }
}

/// Immutable CPU-only Hermite mesh source. Clones retain identity; edits create
/// new snapshots. Native execution, pipelines and output caching belong to GPUI.
#[derive(Clone, Debug)]
pub struct MeshGradient {
    source: Source,
    content: Content,
}
#[derive(Clone, Debug)]
struct Source {
    grid: MeshGrid,
    samples: u32,
    stats: MeshStats,
    controls: Arc<[u8]>,
    programs: Programs,
}
#[derive(Clone, Debug)]
struct Programs {
    compute: PortableProgram,
    render: PortableProgram,
}
fn interpretation() -> ImageInterpretation {
    ImageInterpretation {
        color: ColorEncoding::Srgb,
        alpha: AlphaMode::Straight,
    }
}
impl MeshGradient {
    pub fn new(grid: MeshGrid, subdivisions: u32) -> Result<Self> {
        let stats = MeshStats::new(&grid, subdivisions)?;
        static PROGRAMS: OnceLock<std::result::Result<Programs, ContentError>> = OnceLock::new();
        let programs = PROGRAMS
            .get_or_init(|| {
                Ok(Programs {
                    compute: PortableProgram::compute(
                        include_str!("tessellate.wgsl"),
                        "tessellate",
                        ImageContracts::new(),
                    )?,
                    render: PortableProgram::render(
                        include_str!("render.wgsl"),
                        "mesh_vertex",
                        "mesh_fragment",
                        interpretation(),
                        ImageContracts::new(),
                    )?,
                })
            })
            .clone()?;
        let mut controls = Vec::with_capacity(grid.points.len() * 48);
        for point in &grid.points {
            for value in [
                point.position[0],
                point.position[1],
                point.u_tangent[0],
                point.u_tangent[1],
                point.v_tangent[0],
                point.v_tangent[1],
                0.,
                0.,
                point.color[0],
                point.color[1],
                point.color[2],
                1.,
            ] {
                controls.extend_from_slice(&(if value == 0. { 0f32 } else { value }).to_le_bytes());
            }
        }
        let source = Source {
            grid,
            samples: subdivisions,
            stats,
            controls: controls.into(),
            programs,
        };
        let content = Content::new(source.clone());
        Ok(Self { source, content })
    }
    pub fn with_grid(&self, grid: MeshGrid) -> Result<Self> {
        Self::new(grid, self.source.samples)
    }
    pub fn with_subdivisions(&self, subdivisions: u32) -> Result<Self> {
        Self::new(self.source.grid.clone(), subdivisions)
    }
    pub fn grid(&self) -> &MeshGrid {
        &self.source.grid
    }
    pub fn subdivisions(&self) -> u32 {
        self.source.samples
    }
    pub fn stats(&self) -> MeshStats {
        self.source.stats
    }
    pub fn content(&self) -> Content {
        self.content.clone()
    }
}
impl ContentSource for Source {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> std::result::Result<ImageRef, ContentError> {
        let controls = cx.upload_buffer(
            BufferDescriptor::new(
                self.controls.len() as u64,
                BufferLayout::new(48, 16)?,
                BufferUsage::STORAGE,
            )?,
            self.controls.clone(),
        )?;
        let count = self.stats.triangles * 3;
        let bytes = u64::from(count) * 32;
        let vertices = cx
            .compute(
                ComputePass::new(&self.programs.compute, [count.div_ceil(64), 1, 1])
                    .uniform(
                        "p",
                        UniformValues::new().uints(
                            "grid",
                            [
                                self.grid.width,
                                self.grid.height,
                                self.samples,
                                self.stats.patches,
                            ],
                        ),
                    )
                    .storage_buffer(
                        "points",
                        BufferSlice {
                            buffer: controls,
                            offset: 0,
                            byte_len: self.controls.len() as u64,
                        },
                    )
                    .write_buffer(
                        "vertices",
                        BufferDescriptor::new(
                            bytes,
                            BufferLayout::new(32, 16)?,
                            BufferUsage::STORAGE,
                        )?,
                    ),
            )?
            .buffer("vertices")?;
        cx.render(
            RenderPass::mesh(
                &self.programs.render,
                ImageDescriptor::new(request.extent(), PixelFormat::Rgba8, interpretation(), 1)?,
                count,
            )
            .storage_buffer(
                "vertices",
                BufferSlice {
                    buffer: vertices,
                    offset: 0,
                    byte_len: bytes,
                },
            ),
        )
    }
}
impl ContentSource for MeshGradient {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> std::result::Result<ImageRef, ContentError> {
        self.source.describe(cx, request)
    }
}
impl From<MeshGradient> for Content {
    fn from(mesh: MeshGradient) -> Self {
        mesh.content
    }
}
impl From<MeshGradient> for crate::Paint {
    fn from(mesh: MeshGradient) -> Self {
        mesh.content.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_geometry_and_bounded_tessellation() {
        assert!(MeshGrid::regular(1, 2, vec![[0.; 3]; 2]).is_err());
        assert!(MeshGrid::regular(2, 2, vec![[0.; 3]; 3]).is_err());
        assert!(MeshGrid::regular(2, 2, vec![[f32::NAN; 3]; 4]).is_err());
        let grid = MeshGrid::regular(3, 2, vec![[1., 0., 0.]; 6]).unwrap();
        assert_eq!(grid.points[5].position, [1., 1.]);
        assert_eq!(grid.points[0].u_tangent, [0.5, 0.]);
        let stats = MeshStats::new(&grid, 3).unwrap();
        assert_eq!(
            (stats.patches, stats.evaluated_vertices, stats.triangles),
            (2, 18, 16)
        );
        assert!(MeshStats::new(&grid, 1).is_err());
        assert!(MeshStats::new(&grid, 65).is_err());
        let huge = MeshGrid::regular(16, 16, vec![[0.; 3]; 256]).unwrap();
        assert!(MeshStats::new(&huge, 64).is_err());
        let mut points = grid.points.clone();
        points[0].position[0] = f32::INFINITY;
        assert!(MeshGrid::new(3, 2, points).is_err());
    }
    #[test]
    fn portable_snapshots_share_geometry_across_output_extents() {
        let mesh =
            MeshGradient::new(MeshGrid::regular(3, 2, vec![[0.4; 3]; 6]).unwrap(), 8).unwrap();
        assert_eq!(mesh.content().id(), mesh.clone().content().id());
        let edited = mesh.with_subdivisions(16).unwrap();
        assert_ne!(mesh.content().id(), edited.content().id());
        assert_eq!(mesh.subdivisions(), 8);
        assert_eq!(mesh.source.controls.len(), 6 * 48);
        let roots = [(32, 16), (64, 32)].map(|(w, h)| ContentRoot {
            content: mesh.content(),
            request: ContentRequest::new(
                gpui::size(gpui::px(w as f32), gpui::px(h as f32)),
                ImageExtent::new(w, h).unwrap(),
                1.,
            )
            .unwrap(),
        });
        let graph = ContentGraph::describe(
            &roots,
            ContentCapabilities {
                compute: Some(ComputeLimits {
                    max_workgroup_size: [64; 3],
                    max_workgroup_invocations: 256,
                    max_dispatch: [65535; 3],
                    max_workgroup_memory_bytes: 16384,
                }),
                ..ContentCapabilities::default()
            },
            GraphLimits::default(),
        )
        .unwrap();
        assert_eq!(graph.nodes().len(), 4); // shared upload + compute, two render outputs
        assert_eq!(
            graph
                .nodes()
                .iter()
                .filter(|n| matches!(n.operation(), ContentOperation::Compute(_)))
                .count(),
            1
        );
        assert_eq!(
            mesh.stats().buffer_bytes,
            6 * 48 + u64::from(mesh.stats().triangles) * 3 * 32
        );
    }
}
