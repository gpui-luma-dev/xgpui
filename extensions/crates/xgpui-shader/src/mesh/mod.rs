//! Hermite mesh gradient adapted from patskovn/MeshGradient (MIT).
//! See LICENSE-MeshGradient for the upstream notice and README for changes.
use anyhow::{Result, ensure};
#[cfg(target_os = "macos")]
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "macos")]
mod metal;

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
        let buffer_bytes = grid.points.len() as u64 * 48
            + patches as u64 * 320
            + evaluated_vertices as u64 * 32
            + triangles as u64 * 3 * 32;
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

/// Immutable mesh snapshot. New parameter snapshots share compiled pipelines
/// through `with_grid` / `with_subdivisions`. Keep static snapshots in app state.
pub struct MeshGradient {
    grid: MeshGrid,
    samples: u32,
    stats: MeshStats,
    #[cfg(target_os = "macos")]
    pipelines: Arc<metal::PipelineCache>,
    #[cfg(feature = "wgpu")]
    pub(super) wgpu: crate::wgpu::PaintCache,
    #[cfg(target_os = "windows")]
    pub(super) dx11: crate::dx11::PaintCache,
    #[cfg(target_os = "macos")]
    output: crate::metal_cache::OutputCache,
    pub(super) preparations: AtomicUsize,
}
impl std::fmt::Debug for MeshGradient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshGradient")
            .field("grid", &self.grid)
            .field("samples", &self.samples)
            .finish_non_exhaustive()
    }
}
impl MeshGradient {
    pub fn new(grid: MeshGrid, subdivisions: u32) -> Result<Self> {
        let stats = MeshStats::new(&grid, subdivisions)?;
        Ok(Self {
            grid,
            samples: subdivisions,
            stats,
            #[cfg(target_os = "macos")]
            pipelines: Arc::new(metal::PipelineCache::default()),
            #[cfg(target_os = "macos")]
            output: crate::metal_cache::OutputCache::default(),
            #[cfg(feature = "wgpu")]
            wgpu: crate::wgpu::PaintCache::default(),
            #[cfg(target_os = "windows")]
            dx11: crate::dx11::PaintCache::default(),
            preparations: AtomicUsize::new(0),
        })
    }
    pub fn with_grid(&self, grid: MeshGrid) -> Result<Self> {
        let mut next = Self::new(grid, self.samples)?;
        #[cfg(target_os = "macos")]
        {
            next.pipelines = self.pipelines.clone();
        }
        #[cfg(feature = "wgpu")]
        {
            next.wgpu = self.wgpu.snapshot();
        }
        #[cfg(target_os = "windows")]
        {
            next.dx11 = self.dx11.snapshot();
        }
        Ok(next)
    }
    pub fn with_subdivisions(&self, subdivisions: u32) -> Result<Self> {
        let mut next = Self::new(self.grid.clone(), subdivisions)?;
        #[cfg(target_os = "macos")]
        {
            next.pipelines = self.pipelines.clone();
        }
        #[cfg(feature = "wgpu")]
        {
            next.wgpu = self.wgpu.snapshot();
        }
        #[cfg(target_os = "windows")]
        {
            next.dx11 = self.dx11.snapshot();
        }
        Ok(next)
    }
    pub fn grid(&self) -> &MeshGrid {
        &self.grid
    }
    pub fn subdivisions(&self) -> u32 {
        self.samples
    }
    pub fn stats(&self) -> MeshStats {
        self.stats
    }
    pub fn preparations(&self) -> usize {
        self.preparations.load(Ordering::Relaxed)
    }
    pub fn pipeline_compilations(&self) -> usize {
        let mut count = 0;
        #[cfg(target_os = "macos")]
        {
            count += self.pipelines.compilations.load(Ordering::Relaxed);
        }
        #[cfg(feature = "wgpu")]
        {
            count += self.wgpu.compilations();
        }
        #[cfg(target_os = "windows")]
        {
            count += self.dx11.compilations();
        }
        count
    }
    #[cfg(target_os = "macos")]
    pub fn cached_texture_info(&self) -> Option<(String, u64)> {
        self.output.texture_info()
    }
}

impl gpui::CustomContent for MeshGradient {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        extent: gpui::Size<gpui::DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        #[cfg(target_os = "macos")]
        if backend.is::<gpui_apple::custom_draw::MetalCustomContext>() {
            return self.prepare_metal(backend, extent);
        }
        #[cfg(target_os = "windows")]
        if let Some(context) =
            backend.downcast_ref::<gpui_windows::custom_draw::Dx11CustomContext>()
        {
            return crate::dx11::mesh(self, context, extent);
        }
        #[cfg(feature = "wgpu")]
        if let Some(context) = backend.downcast_ref::<gpui_wgpu::custom_draw::WgpuCustomContext>() {
            return crate::wgpu::mesh(self, context, extent);
        }
        anyhow::bail!("MeshGradient does not support this backend")
    }
}

#[cfg(target_os = "windows")]
impl MeshGradient {
    pub fn cached_texture_info(&self) -> Option<(String, u64)> {
        self.dx11.texture_info()
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
}
