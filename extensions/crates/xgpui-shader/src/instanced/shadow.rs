//! Analytic rounded-box shadows with a programmable falloff and up to four color stops.
//! These are opt-in SDF effects, not pixel-identical CSS/Gaussian box shadows.
use super::*;
use gpui::{Point, point, px, size};

/// A straight-alpha encoded-sRGB color stop across the receiver's gradient axis.
#[derive(Clone, Copy, Debug)]
pub struct ShadowStop {
    /// Position in 0..=1; stops must be nondecreasing.
    pub position: f32,
    /// RGBA channels in 0..=1.
    pub color: [f32; 4],
}
impl ShadowStop {
    /// Construct a stop; values are checked before painting.
    pub fn new(position: f32, color: [f32; 4]) -> Self {
        Self { position, color }
    }
}
/// A single shadow layer. Paint outset layers before the receiver background,
/// and inset layers after the background but before children. Repeated calls stack layers.
#[derive(Clone, Debug)]
pub struct ShadowStyle {
    /// One to four color stops. One stop produces a solid-color shadow.
    pub stops: Vec<ShadowStop>,
    /// Gradient angle in degrees: zero points up, 90 points right.
    pub angle: f32,
    /// Offset of the shadow shape (or the inset hole), in logical pixels.
    pub offset: Point<Pixels>,
    /// Half-width of the smooth transition band, in logical pixels; zero gives a hard edge.
    pub blur_radius: Pixels,
    /// Expand an outset shape or shrink an inset hole. Negative values reverse this.
    pub spread_radius: Pixels,
    /// Power applied to smooth coverage, 0.25..=8; one is the default curve.
    pub falloff: f32,
    /// Clip the shadow inside the receiver rather than outside it.
    pub inset: bool,
}
impl Default for ShadowStyle {
    fn default() -> Self {
        Self {
            stops: vec![ShadowStop::new(0., [0., 0., 0., 0.4])],
            angle: 90.,
            offset: point(px(0.), px(8.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            falloff: 1.,
            inset: false,
        }
    }
}
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct ShadowInstance {
    shape: [f32; 4],
    metrics: [f32; 4],
    direction: [f32; 4],
    stops: [f32; 4],
    color0: [f32; 4],
    color1: [f32; 4],
    color2: [f32; 4],
    color3: [f32; 4],
}
impl ShaderInstance for ShadowInstance {
    const FIELDS: &'static [InstanceField] = &[
        InstanceField {
            name: "shape",
            offset: std::mem::offset_of!(Self, shape),
            components: 4,
        },
        InstanceField {
            name: "metrics",
            offset: std::mem::offset_of!(Self, metrics),
            components: 4,
        },
        InstanceField {
            name: "direction",
            offset: std::mem::offset_of!(Self, direction),
            components: 4,
        },
        InstanceField {
            name: "stops",
            offset: std::mem::offset_of!(Self, stops),
            components: 4,
        },
        InstanceField {
            name: "color0",
            offset: std::mem::offset_of!(Self, color0),
            components: 4,
        },
        InstanceField {
            name: "color1",
            offset: std::mem::offset_of!(Self, color1),
            components: 4,
        },
        InstanceField {
            name: "color2",
            offset: std::mem::offset_of!(Self, color2),
            components: 4,
        },
        InstanceField {
            name: "color3",
            offset: std::mem::offset_of!(Self, color3),
            components: 4,
        },
    ];
}
/// An immutable validated shadow layer for `painted().shadow_paint(...)`.
/// Clones share the painter's program and can be reused across styled containers.
#[derive(Clone)]
pub struct ShadowLayer {
    painter: ShadowPainter,
    style: ShadowStyle,
}
impl ShadowLayer {
    pub(crate) fn is_inset(&self) -> bool {
        self.style.inset
    }
    pub(crate) fn paint(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radius: Pixels,
        opacity: f32,
    ) -> Result<()> {
        let (extent, mut data) = prepare(bounds, radius, &self.style, 1. / window.scale_factor())?;
        ensure!(
            opacity.is_finite() && (0.0..=1.0).contains(&opacity),
            "shadow opacity must be in 0..=1"
        );
        for color in [
            &mut data.color0,
            &mut data.color1,
            &mut data.color2,
            &mut data.color3,
        ] {
            for channel in color {
                *channel *= opacity;
            }
        }
        if !extent.is_empty() {
            window.paint_custom_shader(extent, &self.painter.program, data)?;
        }
        Ok(())
    }
}

/// Reusable shader and typed recording API. Create once and share across shadow layers.
#[derive(Clone)]
pub struct ShadowPainter {
    program: ShaderProgram<ShadowInstance>,
}
impl ShadowPainter {
    /// Validate and translate the shared shadow shader for all supported backends.
    pub fn new() -> Result<Self> {
        Ok(Self {
            program: ShaderProgram::new(include_str!("shadow.wgsl"))?,
        })
    }
    /// Validate a reusable layer for a styled `painted()` container.
    pub fn layer(&self, style: ShadowStyle) -> Result<ShadowLayer> {
        prepare(Bounds::default(), gpui::px(0.), &style, 1.)?;
        Ok(ShadowLayer {
            painter: self.clone(),
            style,
        })
    }
    /// Paint one layer; effect bounds expand automatically for offset, spread and softness.
    /// `bounds` and `radius` describe the receiver, not the expanded shadow rectangle.
    pub fn paint(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radius: Pixels,
        style: &ShadowStyle,
    ) -> Result<()> {
        let (extent, data) = prepare(bounds, radius, style, 1. / window.scale_factor())?;
        if !extent.is_empty() {
            window.paint_custom_shader(extent, &self.program, data)?;
        }
        Ok(())
    }
}
fn prepare(
    bounds: Bounds<Pixels>,
    radius: Pixels,
    style: &ShadowStyle,
    aa: f32,
) -> Result<(Bounds<Pixels>, ShadowInstance)> {
    let x = f32::from(bounds.origin.x);
    let y = f32::from(bounds.origin.y);
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let radius = f32::from(radius);
    let blur = f32::from(style.blur_radius);
    let spread = f32::from(style.spread_radius);
    let dx = f32::from(style.offset.x);
    let dy = f32::from(style.offset.y);
    ensure!(
        [
            x,
            y,
            width,
            height,
            radius,
            blur,
            spread,
            dx,
            dy,
            style.falloff,
            style.angle
        ]
        .iter()
        .all(|v| v.is_finite()),
        "shadow geometry and parameters must be finite"
    );
    ensure!(
        width >= 0. && height >= 0. && radius >= 0. && blur >= 0.,
        "shadow dimensions, radius and blur must be nonnegative"
    );
    ensure!(
        (0.25..=8.).contains(&style.falloff),
        "shadow falloff must be in 0.25..=8"
    );
    ensure!(
        !style.stops.is_empty() && style.stops.len() <= 4,
        "shadow needs 1..=4 color stops"
    );
    let mut last = 0.;
    for stop in &style.stops {
        ensure!(
            stop.position.is_finite() && (last..=1.).contains(&stop.position),
            "shadow stops must be finite, sorted and in 0..=1"
        );
        ensure!(
            stop.color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "shadow color channels must be finite and in 0..=1"
        );
        last = stop.position;
    }
    // The shader's transition support ends at blur; retain an extra logical pixel for AA.
    let pad = blur + aa;
    let extent = if style.inset || bounds.is_empty() {
        bounds
    } else {
        let left = x.min(x + dx - spread) - pad;
        let top = y.min(y + dy - spread) - pad;
        let right = (x + width).max(x + width + dx + spread) + pad;
        let bottom = (y + height).max(y + height + dy + spread) + pad;
        Bounds::new(
            point(px(left), px(top)),
            size(px(right - left), px(bottom - top)),
        )
    };
    ensure!(
        [
            f32::from(extent.origin.x),
            f32::from(extent.origin.y),
            f32::from(extent.size.width),
            f32::from(extent.size.height)
        ]
        .iter()
        .all(|v| v.is_finite()),
        "shadow effect extent overflow"
    );
    let mut positions = [1.; 4];
    let mut colors = [[0.; 4]; 4];
    for i in 0..4 {
        let stop = style.stops[i.min(style.stops.len() - 1)];
        positions[i] = stop.position;
        let [r, g, b, a] = stop.color;
        // Interpolate premultiplied channels so transparent colored stops do not fringe.
        colors[i] = [r * a, g * a, b * a, a];
    }
    Ok((
        extent,
        ShadowInstance {
            shape: [
                x - f32::from(extent.origin.x),
                y - f32::from(extent.origin.y),
                width,
                height,
            ],
            metrics: [
                radius.min(width.min(height) * 0.5),
                blur,
                spread,
                style.falloff,
            ],
            direction: [
                dx,
                dy,
                style.angle.rem_euclid(360.).to_radians(),
                u8::from(style.inset) as f32,
            ],
            stops: positions,
            color0: colors[0],
            color1: colors[1],
            color2: colors[2],
            color3: colors[3],
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shadow_layout_geometry_and_validation() {
        ShadowPainter::new().unwrap();
        assert_eq!(std::mem::size_of::<ShadowInstance>(), 128);
        let bounds = Bounds::new(point(px(100.), px(100.)), size(px(100.), px(60.)));
        let mut style = ShadowStyle::default();
        let (extent, data) = prepare(bounds, px(10.), &style, 1.).unwrap();
        assert_eq!(extent.origin, point(px(75.), px(75.)));
        assert_eq!(extent.size, size(px(150.), px(118.)));
        assert_eq!(data.shape, [25., 25., 100., 60.]);
        style.inset = true;
        assert_eq!(prepare(bounds, px(10.), &style, 1.).unwrap().0, bounds);
        style.falloff = 0.;
        assert!(prepare(bounds, px(10.), &style, 1.).is_err());
        style.falloff = 1.;
        style.stops[0].color[0] = f32::NAN;
        assert!(prepare(bounds, px(10.), &style, 1.).is_err());
        style.stops = vec![ShadowStop::new(0.8, [1.; 4]), ShadowStop::new(0.2, [1.; 4])];
        assert!(prepare(bounds, px(10.), &style, 1.).is_err());
    }
    #[cfg(all(target_os = "macos", feature = "wgpu"))]
    #[test]
    fn shadow_pixels_cover_outset_inset_gradient_and_falloff_on_both_renderers() {
        use gpui::{
            ContentMask, DevicePixels, PlatformHeadlessRenderer, Primitive, ScaledPixels, Scene,
            ShaderDraw,
        };
        let painter = ShadowPainter::new().unwrap();
        let bounds = Bounds::new(point(px(24.), px(24.)), size(px(32.), px(24.)));
        let mut renderers: Vec<Box<dyn PlatformHeadlessRenderer>> = vec![
            Box::new(gpui_apple::metal_renderer::MetalHeadlessRenderer::new()),
            Box::new(gpui_wgpu::WgpuHeadlessRenderer::new().unwrap()),
        ];
        for renderer in &mut renderers {
            let mut outer_red = 0;
            for (inset, falloff) in [(false, 1.), (true, 1.), (false, 4.)] {
                let style = ShadowStyle {
                    inset,
                    falloff,
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(8.),
                    stops: vec![
                        ShadowStop::new(0., [1., 0., 0., 1.]),
                        ShadowStop::new(1., [0., 0., 1., 1.]),
                    ],
                    ..Default::default()
                };
                let (extent, instance) = prepare(bounds, px(0.), &style, 1.).unwrap();
                let mut scene = Scene::default();
                scene.insert_primitive(Primitive::Shader(ShaderDraw {
                    path_mask: None,
                    glyph_mask: None,
                    order: 0,
                    bounds: extent.scale(1.),
                    content_mask: ContentMask {
                        bounds: Bounds::new(
                            point(ScaledPixels(0.), ScaledPixels(0.)),
                            size(ScaledPixels(80.), ScaledPixels(72.)),
                        ),
                    },
                    opacity: 1.,
                    scale_factor: 1.,
                    program: painter.program.program.clone(),
                    data: painter.program.pack(&instance).unwrap(),
                }));
                scene.finish();
                let image = renderer
                    .render_scene_to_image(&scene, size(DevicePixels(80), DevicePixels(72)))
                    .unwrap();
                assert_eq!(
                    &image.get_pixel(40, 36).0[..3],
                    &[0, 0, 0],
                    "center remains clear"
                );
                assert_eq!(&image.get_pixel(4, 4).0[..3], &[0, 0, 0], "outside support");
                if inset {
                    assert_eq!(
                        &image.get_pixel(22, 36).0[..3],
                        &[0, 0, 0],
                        "inset is contained"
                    );
                    let left = image.get_pixel(25, 36).0;
                    let right = image.get_pixel(54, 36).0;
                    assert!(left[0] > 40 && left[0] > left[2], "red inset edge {left:?}");
                    assert!(
                        right[2] > 40 && right[2] > right[0],
                        "blue inset edge {right:?}"
                    );
                } else if falloff == 1. {
                    let left = image.get_pixel(22, 36).0;
                    let right = image.get_pixel(58, 36).0;
                    assert!(left[0] > 40 && left[0] > left[2], "red outset {left:?}");
                    assert!(
                        right[2] > 40 && right[2] > right[0],
                        "blue outset {right:?}"
                    );
                    outer_red = left[0];
                } else {
                    assert!(
                        image.get_pixel(22, 36).0[0] < outer_red / 2,
                        "falloff changes coverage"
                    );
                }
            }
        }
    }
}
