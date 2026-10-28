//! Analytic multi-stop paints. Geometry and GPU code stay outside GPUI.
use anyhow::{Result, ensure};
#[cfg(target_os = "macos")]
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "macos")]
mod metal;

/// A stop in normalized gradient space, with straight-alpha encoded-sRGB RGBA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub offset: f32,
    pub color: [f32; 4],
}
impl GradientStop {
    pub fn new(offset: f32, color: [f32; 4]) -> Self {
        Self { offset, color }
    }
}

/// Geometry of an analytic paint. Angles are degrees clockwise, zero pointing up
/// for linear/conic gradients. Diamond zero leaves its normalized axes unrotated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Linear {
        angle: f32,
    },
    /// Ellipse radii are fractions of the full bounds (0.5 reaches edge midpoints).
    Radial {
        center: [f32; 2],
        radius: [f32; 2],
    },
    /// Angles use physical pixel distances, preserving direction in non-square bounds.
    Conic {
        center: [f32; 2],
        angle: f32,
    },
    Diamond {
        center: [f32; 2],
        radius: [f32; 2],
        angle: f32,
    },
}
impl GradientKind {
    fn validate(self) -> Result<()> {
        let (center, radius, angle) = match self {
            Self::Linear { angle } => ([0.5; 2], [0.5; 2], angle),
            Self::Radial { center, radius } => (center, radius, 0.),
            Self::Conic { center, angle } => (center, [0.5; 2], angle),
            Self::Diamond {
                center,
                radius,
                angle,
            } => (center, radius, angle),
        };
        ensure!(angle.is_finite(), "gradient angle must be finite");
        ensure!(
            center
                .iter()
                .all(|v| v.is_finite() && (-16. ..=16.).contains(v)),
            "gradient center must be finite and in -16..=16"
        );
        ensure!(
            radius
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0001 && *v <= 16.),
            "gradient radius must be finite and in 0.0001..=16"
        );
        Ok(())
    }
}

/// Immutable paint with 2..=32 sorted stops. Duplicate offsets form hard
/// edges (the last stop at an offset wins). Outside stops, endpoint colors clamp.
/// Colors interpolate in premultiplied encoded sRGB, then output straight alpha.
/// Related snapshots share their compiled pipeline; static output is cached.
pub struct Gradient {
    kind: GradientKind,
    stops: Vec<GradientStop>,
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
impl std::fmt::Debug for Gradient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gradient")
            .field("kind", &self.kind)
            .field("stops", &self.stops)
            .finish_non_exhaustive()
    }
}
impl Gradient {
    pub fn new(kind: GradientKind, stops: Vec<GradientStop>) -> Result<Self> {
        kind.validate()?;
        ensure!(
            (2..=32).contains(&stops.len()),
            "gradient needs 2..=32 stops"
        );
        for stop in &stops {
            ensure!(
                stop.offset.is_finite() && (0. ..=1.).contains(&stop.offset),
                "stop offset must be in 0..=1"
            );
            ensure!(
                stop.color
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "stop RGBA must be finite and in 0..=1"
            );
        }
        ensure!(
            stops.windows(2).all(|s| s[0].offset <= s[1].offset),
            "stops must be sorted"
        );
        Ok(Self {
            kind,
            stops,
            #[cfg(target_os = "macos")]
            pipelines: Arc::default(),
            #[cfg(target_os = "macos")]
            output: crate::metal_cache::OutputCache::default(),
            #[cfg(feature = "wgpu")]
            wgpu: crate::wgpu::PaintCache::default(),
            #[cfg(target_os = "windows")]
            dx11: crate::dx11::PaintCache::default(),
            preparations: AtomicUsize::new(0),
        })
    }
    pub fn linear(angle: f32, stops: Vec<GradientStop>) -> Result<Self> {
        Self::new(GradientKind::Linear { angle }, stops)
    }
    pub fn radial(center: [f32; 2], radius: [f32; 2], stops: Vec<GradientStop>) -> Result<Self> {
        Self::new(GradientKind::Radial { center, radius }, stops)
    }
    pub fn conic(center: [f32; 2], angle: f32, stops: Vec<GradientStop>) -> Result<Self> {
        Self::new(GradientKind::Conic { center, angle }, stops)
    }
    pub fn diamond(
        center: [f32; 2],
        radius: [f32; 2],
        angle: f32,
        stops: Vec<GradientStop>,
    ) -> Result<Self> {
        Self::new(
            GradientKind::Diamond {
                center,
                radius,
                angle,
            },
            stops,
        )
    }
    pub fn kind(&self) -> GradientKind {
        self.kind
    }
    pub fn stops(&self) -> &[GradientStop] {
        &self.stops
    }
    pub fn with_kind(&self, kind: GradientKind) -> Result<Self> {
        kind.validate()?;
        Ok(Self {
            kind,
            stops: self.stops.clone(),
            #[cfg(target_os = "macos")]
            pipelines: self.pipelines.clone(),
            #[cfg(target_os = "macos")]
            output: crate::metal_cache::OutputCache::default(),
            #[cfg(feature = "wgpu")]
            wgpu: self.wgpu.snapshot(),
            #[cfg(target_os = "windows")]
            dx11: self.dx11.snapshot(),
            preparations: AtomicUsize::new(0),
        })
    }
    pub fn with_stops(&self, stops: Vec<GradientStop>) -> Result<Self> {
        let mut snapshot = Self::new(self.kind, stops)?;
        #[cfg(target_os = "macos")]
        {
            snapshot.pipelines = self.pipelines.clone();
        }
        #[cfg(feature = "wgpu")]
        {
            snapshot.wgpu = self.wgpu.snapshot();
        }
        #[cfg(target_os = "windows")]
        {
            snapshot.dx11 = self.dx11.snapshot();
        }
        Ok(snapshot)
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
}
impl gpui::CustomContent for Gradient {
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
            return crate::dx11::gradient(self, context, extent);
        }
        #[cfg(feature = "wgpu")]
        if let Some(context) = backend.downcast_ref::<gpui_wgpu::custom_draw::WgpuCustomContext>() {
            return crate::wgpu::gradient(self, context, extent);
        }
        anyhow::bail!("Gradient does not support this backend")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_stops_and_geometry() {
        let stops = vec![
            GradientStop::new(0., [1.; 4]),
            GradientStop::new(1., [0.; 4]),
        ];
        assert!(Gradient::linear(f32::NAN, stops.clone()).is_err());
        assert!(Gradient::radial([0.5; 2], [0., 0.5], stops.clone()).is_err());
        assert!(Gradient::linear(0., vec![stops[0]]).is_err());
        assert!(Gradient::linear(0., vec![stops[1], stops[0]]).is_err());
        assert!(
            Gradient::linear(0., vec![GradientStop::new(0., [f32::NAN; 4]), stops[1]]).is_err()
        );
        assert!(Gradient::linear(0., vec![stops[0]; 33]).is_err());
        let gradient = Gradient::linear(90., stops).unwrap();
        assert!(
            gradient
                .with_stops(vec![GradientStop::new(0.5, [1.; 4]); 2])
                .is_ok()
        );
        assert!(
            gradient
                .with_kind(GradientKind::Conic {
                    center: [f32::INFINITY, 0.5],
                    angle: 0.
                })
                .is_err()
        );
    }
}
