//! Portable analytic gradients. Native execution and caches belong to the host.
use anyhow::ensure;
use gpui::content::*;
use std::sync::{Arc, OnceLock};
type Result<T> = std::result::Result<T, ContentError>;
fn invalid(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::InvalidDescriptor, message)
}
fn interpretation() -> ImageInterpretation {
    ImageInterpretation {
        color: ColorEncoding::Srgb,
        alpha: AlphaMode::Straight,
    }
}
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
    fn validate(self) -> anyhow::Result<()> {
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

/// Immutable analytic paint with 2..=32 sorted stops. Duplicate offsets form
/// hard edges (the last stop at an offset wins); endpoint colors clamp outside.
/// RGB interpolates in premultiplied encoded sRGB and returns straight alpha.
/// Clones retain source identity; edits create new immutable content snapshots.
#[derive(Clone, Debug)]
pub struct Gradient {
    source: Source,
    content: Content,
}
#[derive(Clone, Debug)]
struct Source {
    kind: GradientKind,
    stops: Arc<[GradientStop]>,
    stop_uniforms: Arc<UniformValues>,
    program: PortableProgram,
}
impl Gradient {
    pub fn new(kind: GradientKind, stops: Vec<GradientStop>) -> Result<Self> {
        kind.validate().map_err(|e| invalid(&e.to_string()))?;
        validate_stops(&stops)?;
        static PROGRAM: OnceLock<Result<PortableProgram>> = OnceLock::new();
        let program = PROGRAM
            .get_or_init(|| {
                PortableProgram::fullscreen(
                    include_str!("gradient.wgsl"),
                    "paint_fragment",
                    interpretation(),
                    ImageContracts::new(),
                )
            })
            .clone()?;
        // A fixed-size uniform table works on WebGL2 as well as storage-capable
        // hosts. Reflection packs sixteen column-major blocks without ABI casts.
        let mut bytes = vec![0u8; 32 * 32];
        for (index, stop) in stops.iter().enumerate() {
            for (word, value) in stop
                .color
                .into_iter()
                .chain([stop.offset, 0., 0., 0.])
                .enumerate()
            {
                let value = if value == 0. { 0. } else { value };
                bytes[index * 32 + word * 4..index * 32 + word * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        let mut stop_uniforms = UniformValues::new();
        for (index, block) in bytes.chunks_exact(64).enumerate() {
            let words: Vec<f32> = block
                .chunks_exact(4)
                .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
                .collect();
            stop_uniforms = stop_uniforms.matrix(format!("data{index}"), 4, 4, words);
        }
        Self::from_source(Source {
            kind,
            stops: stops.into(),
            stop_uniforms: Arc::new(stop_uniforms),
            program,
        })
    }
    fn from_source(source: Source) -> Result<Self> {
        let content = Content::new(source.clone());
        Ok(Self { source, content })
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
        self.source.kind
    }
    pub fn stops(&self) -> &[GradientStop] {
        &self.source.stops
    }
    /// Retain immutable portable content for paints, scene records or dependencies.
    pub fn content(&self) -> Content {
        self.content.clone()
    }
    pub fn with_kind(&self, kind: GradientKind) -> Result<Self> {
        kind.validate().map_err(|e| invalid(&e.to_string()))?;
        Self::from_source(Source {
            kind,
            ..self.source.clone()
        })
    }
    pub fn with_stops(&self, stops: Vec<GradientStop>) -> Result<Self> {
        Self::new(self.kind(), stops)
    }
}
fn validate_stops(stops: &[GradientStop]) -> Result<()> {
    if !(2..=32).contains(&stops.len()) {
        return Err(invalid("gradient needs 2..=32 stops"));
    }
    for stop in stops {
        if !stop.offset.is_finite() || !(0. ..=1.).contains(&stop.offset) {
            return Err(invalid("stop offset must be in 0..=1"));
        }
        if stop
            .color
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err(invalid("stop RGBA must be finite and in 0..=1"));
        }
    }
    if stops.windows(2).any(|s| s[0].offset > s[1].offset) {
        return Err(invalid("stops must be sorted"));
    }
    Ok(())
}
impl ContentSource for Source {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest) -> Result<ImageRef> {
        let (kind, center, radius, angle) = match self.kind {
            GradientKind::Linear { angle } => (0., [0.5; 2], [0.5; 2], angle),
            GradientKind::Radial { center, radius } => (1., center, radius, 0.),
            GradientKind::Conic { center, angle } => (2., center, [0.5; 2], angle),
            GradientKind::Diamond {
                center,
                radius,
                angle,
            } => (3., center, radius, angle),
        };
        let radians = angle.rem_euclid(360.).to_radians();
        let direction = if kind == 0. {
            [radians.sin(), -radians.cos(), radians, 0.]
        } else {
            [radians.cos(), radians.sin(), radians, 0.]
        };
        let uniforms = UniformValues::new()
            .vec4(
                "size_kind_count",
                [
                    request.extent().width() as f32,
                    request.extent().height() as f32,
                    kind,
                    self.stops.len() as f32,
                ],
            )
            .vec4(
                "center_radius",
                [center[0], center[1], radius[0], radius[1]],
            )
            .vec4("direction", direction);
        cx.render(
            RenderPass::fullscreen(
                &self.program,
                ImageDescriptor::new(request.extent(), PixelFormat::Rgba8, interpretation(), 1)?,
            )
            .uniform("p", uniforms)
            .uniform("stops", (*self.stop_uniforms).clone()),
        )
    }
}
impl ContentSource for Gradient {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest) -> Result<ImageRef> {
        self.source.describe(cx, request)
    }
}
impl From<Gradient> for Content {
    fn from(value: Gradient) -> Self {
        value.content
    }
}
impl From<Gradient> for crate::Paint {
    fn from(value: Gradient) -> Self {
        value.content.into()
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
    #[test]
    fn immutable_snapshots_share_identity_and_portable_stop_uniforms() {
        let base = Gradient::linear(
            0.,
            vec![
                GradientStop::new(0., [1.; 4]),
                GradientStop::new(1., [0.; 4]),
            ],
        )
        .unwrap();
        assert_eq!(base.content().id(), base.clone().content().id());
        let edited = base.with_kind(GradientKind::Linear { angle: 90. }).unwrap();
        assert_ne!(base.content().id(), edited.content().id());
        assert_eq!(base.kind(), GradientKind::Linear { angle: 0. });
        assert!(Arc::ptr_eq(
            &base.source.stop_uniforms,
            &edited.source.stop_uniforms
        ));
        let request = ContentRequest::new(
            gpui::size(gpui::px(8.), gpui::px(8.)),
            ImageExtent::new(8, 8).unwrap(),
            1.,
        )
        .unwrap();
        let graph = ContentGraph::describe(
            &[
                ContentRoot {
                    content: base.content(),
                    request: request.clone(),
                },
                ContentRoot {
                    content: edited.content(),
                    request,
                },
            ],
            ContentCapabilities {
                bindings: BindingLimits {
                    max_storage_bytes: 0,
                    ..BindingLimits::default()
                },
                ..ContentCapabilities::default()
            },
            GraphLimits::default(),
        )
        .unwrap();
        assert_eq!(graph.nodes().len(), 2); // independent renders; no storage-buffer requirement
        assert_eq!(
            graph
                .nodes()
                .iter()
                .filter(|n| matches!(n.operation(), ContentOperation::UploadBuffer(_)))
                .count(),
            0
        );
        let next = base
            .with_stops(vec![GradientStop::new(0., [0.; 4]); 2])
            .unwrap();
        assert_ne!(base.content().id(), next.content().id());
        assert_eq!(base.stops()[0].color, [1.; 4]);
    }
}
