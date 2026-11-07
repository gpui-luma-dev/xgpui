//! Color glyphs: COLR v0/v1 painting and sbix/CBDT bitmap strikes.
//!
//! Output is GPUI's polychrome sprite format: BGRA with straight alpha, sized to
//! integer device-pixel bounds relative to the glyph origin (y down).
use anyhow::{Context, Result, ensure};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    bitmap::{BitmapData, BitmapStrikes, Origin},
    color::{Brush, ColorGlyphCollection, ColorPainter, ColorStop, CompositeMode, Extend},
    instance::{LocationRef, NormalizedCoord, Size},
    outline::{DrawSettings, OutlinePen},
    raw::{TableProvider, types::BoundingBox},
};
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, GradientStop, LinearGradient, Mask, Paint, Path,
    PathBuilder, Pixmap, PixmapPaint, Point, RadialGradient, Rect, Shader, SpreadMode,
    SweepGradient, Transform,
};

const MAX_EXTENT: f32 = 4096.0;

/// Whether a font has color data for a glyph. Built once per shaped run.
pub(crate) struct ColorProbe<'a> {
    colr: ColorGlyphCollection<'a>,
    bitmaps: BitmapStrikes<'a>,
}

impl<'a> ColorProbe<'a> {
    /// `None` when the font has neither COLR nor color bitmap strikes.
    pub fn new(data: &'a [u8], index: u32) -> Option<Self> {
        let font = FontRef::from_index(data, index).ok()?;
        let probe = Self {
            colr: font.color_glyphs(),
            bitmaps: font.bitmap_strikes(),
        };
        let has_colr = font.data_for_tag(skrifa::Tag::new(b"COLR")).is_some();
        (has_colr || !probe.bitmaps.is_empty()).then_some(probe)
    }

    pub fn has(&self, glyph: u32) -> bool {
        let glyph = GlyphId::new(glyph);
        self.colr.get(glyph).is_some()
            || self
                .bitmaps
                .glyph_for_size(Size::new(64.0), glyph)
                .is_some_and(|bitmap| !matches!(bitmap.data, BitmapData::Mask(_)))
    }
}

/// A rendered color glyph: device-pixel origin and BGRA straight-alpha pixels.
pub(crate) struct ColorRaster {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Render `glyph` at `ppem` device pixels per em. COLR is preferred over
/// bitmaps; returns `None` when the font has no color data for the glyph.
pub(crate) fn render(
    font: &FontRef,
    coords: &[NormalizedCoord],
    glyph: u32,
    ppem: f32,
) -> Result<Option<ColorRaster>> {
    let glyph = GlyphId::new(glyph);
    if let Some(raster) = render_colr(font, coords, glyph, ppem)? {
        return Ok(Some(raster));
    }
    render_bitmap(font, glyph, ppem)
}

fn render_bitmap(font: &FontRef, glyph: GlyphId, ppem: f32) -> Result<Option<ColorRaster>> {
    let Some(bitmap) = font.bitmap_strikes().glyph_for_size(Size::new(ppem), glyph) else {
        return Ok(None);
    };
    let source = match bitmap.data {
        BitmapData::Png(png) => Pixmap::decode_png(png).context("invalid color bitmap PNG")?,
        BitmapData::Bgra(bgra) => {
            // Premultiplied BGRA in the font; tiny-skia stores premultiplied RGBA.
            let mut rgba = bgra.to_vec();
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            let size = tiny_skia::IntSize::from_wh(bitmap.width, bitmap.height)
                .context("empty color bitmap")?;
            Pixmap::from_vec(rgba, size).context("invalid color bitmap")?
        }
        // Monochrome strikes are not color glyphs; outlines handle them.
        BitmapData::Mask(_) => return Ok(None),
    };
    let upem = font.head().map(|head| head.units_per_em()).unwrap_or(1000) as f32;
    let strike_scale = ppem / bitmap.ppem_y.max(1.0);
    let unit_scale = ppem / upem.max(1.0);
    let width = source.width() as f32 * strike_scale;
    let height = source.height() as f32 * strike_scale;
    let left = bitmap.bearing_x * unit_scale + bitmap.inner_bearing_x * strike_scale;
    let rise = bitmap.bearing_y * unit_scale + bitmap.inner_bearing_y * strike_scale;
    let top = match bitmap.placement_origin {
        Origin::BottomLeft => -(rise + height),
        Origin::TopLeft => -rise,
    };
    let (x0, y0) = (left.floor(), top.floor());
    let (x1, y1) = ((left + width).ceil(), (top + height).ceil());
    let Some(mut target) = canvas(x1 - x0, y1 - y0)? else {
        return Ok(None);
    };
    target.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &PixmapPaint {
            quality: FilterQuality::Bicubic,
            ..Default::default()
        },
        Transform::from_row(strike_scale, 0.0, 0.0, strike_scale, left - x0, top - y0),
        None,
    );
    Ok(Some(finish(&target, x0 as i32, y0 as i32)))
}

fn render_colr(
    font: &FontRef,
    coords: &[NormalizedCoord],
    glyph: GlyphId,
    ppem: f32,
) -> Result<Option<ColorRaster>> {
    let Some(color_glyph) = font.color_glyphs().get(glyph) else {
        return Ok(None);
    };
    let location = LocationRef::new(coords);
    let upem = font.head().map(|head| head.units_per_em()).unwrap_or(1000) as f32;
    let scale = ppem / upem.max(1.0);
    // Paint into the clip box when the font declares one, otherwise into a
    // generous em-based box, then trim to the painted pixels.
    let (x0, y0, x1, y1) = match color_glyph.bounding_box(location, Size::new(ppem)) {
        Some(b) => (b.x_min, -b.y_max, b.x_max, -b.y_min),
        None => {
            let metrics = font.metrics(Size::new(ppem), location);
            let advance = font
                .glyph_metrics(Size::new(ppem), location)
                .advance_width(glyph)
                .unwrap_or(ppem);
            let margin = ppem * 0.5;
            (
                -margin,
                -metrics.ascent - margin,
                advance + margin,
                -metrics.descent + margin,
            )
        }
    };
    let (x0, y0) = (x0.floor(), y0.floor());
    let Some(target) = canvas(x1.ceil() - x0, y1.ceil() - y0)? else {
        return Ok(None);
    };
    let palette: Vec<Color> = font
        .color_palettes()
        .get(0)
        .map(|palette| {
            palette
                .colors()
                .iter()
                .map(|c| Color::from_rgba8(c.red, c.green, c.blue, c.alpha))
                .collect()
        })
        .unwrap_or_default();
    let mut painter = Painter {
        font,
        location,
        palette,
        // Font units, y up, to canvas pixels, y down.
        transforms: vec![Transform::from_row(scale, 0.0, 0.0, -scale, -x0, -y0)],
        clips: vec![None],
        layers: vec![target],
    };
    color_glyph
        .paint(location, &mut painter)
        .map_err(|error| anyhow::anyhow!("color glyph paint failed: {error:?}"))?;
    let target = painter
        .layers
        .pop()
        .context("color glyph layers unbalanced")?;
    Ok(trim(&target, x0 as i32, y0 as i32))
}

fn canvas(width: f32, height: f32) -> Result<Option<Pixmap>> {
    ensure!(
        width.is_finite() && height.is_finite() && width <= MAX_EXTENT && height <= MAX_EXTENT,
        "color glyph bounds exceed raster limits"
    );
    if width < 1.0 || height < 1.0 {
        return Ok(None);
    }
    Pixmap::new(width as u32, height as u32)
        .context("unable to allocate color glyph")
        .map(Some)
}

/// Crop to painted pixels; `None` when nothing was painted.
fn trim(pixmap: &Pixmap, left: i32, top: i32) -> Option<ColorRaster> {
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    let pixels = pixmap.pixels();
    let painted = |x: usize, y: usize| pixels[y * width + x].alpha() > 0;
    let rows: Vec<usize> = (0..height)
        .filter(|&y| (0..width).any(|x| painted(x, y)))
        .collect();
    let (&first_row, &last_row) = (rows.first()?, rows.last()?);
    let first_col = (0..width).find(|&x| (0..height).any(|y| painted(x, y)))?;
    let last_col = (0..width)
        .rev()
        .find(|&x| (0..height).any(|y| painted(x, y)))?;
    let cropped = pixmap.clone_rect(tiny_skia::IntRect::from_ltrb(
        first_col as i32,
        first_row as i32,
        last_col as i32 + 1,
        last_row as i32 + 1,
    )?)?;
    Some(finish(
        &cropped,
        left + first_col as i32,
        top + first_row as i32,
    ))
}

/// Premultiplied RGBA to GPUI's BGRA with straight alpha.
fn finish(pixmap: &Pixmap, left: i32, top: i32) -> ColorRaster {
    let mut bgra = Vec::with_capacity(pixmap.data().len());
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        bgra.extend_from_slice(&[color.blue(), color.green(), color.red(), color.alpha()]);
    }
    ColorRaster {
        left,
        top,
        width: pixmap.width(),
        height: pixmap.height(),
        bgra,
    }
}

struct Painter<'a, 'f> {
    font: &'f FontRef<'a>,
    location: LocationRef<'f>,
    palette: Vec<Color>,
    transforms: Vec<Transform>,
    clips: Vec<Option<Mask>>,
    layers: Vec<Pixmap>,
}

impl Painter<'_, '_> {
    fn transform(&self) -> Transform {
        *self.transforms.last().unwrap()
    }

    fn size(&self) -> (u32, u32) {
        let layer = self.layers.last().unwrap();
        (layer.width(), layer.height())
    }

    fn color(&self, palette_index: u16, alpha: f32) -> Color {
        // 0xFFFF is the text foreground color; emoji sprites have none, so use black.
        let mut color = self
            .palette
            .get(palette_index as usize)
            .copied()
            .unwrap_or(Color::BLACK);
        color.apply_opacity(alpha.clamp(0.0, 1.0));
        color
    }

    fn stops(&self, stops: &[ColorStop]) -> Vec<GradientStop> {
        stops
            .iter()
            .map(|stop| {
                GradientStop::new(
                    stop.offset.clamp(0.0, 1.0),
                    self.color(stop.palette_index, stop.alpha),
                )
            })
            .collect()
    }

    fn push_clip_path(&mut self, path: Option<Path>) {
        let (width, height) = self.size();
        let transform = self.transform();
        let mut mask = match self.clips.last().unwrap() {
            Some(mask) => mask.clone(),
            None => Mask::new(width, height).unwrap(),
        };
        match (path, self.clips.last().unwrap().is_some()) {
            (Some(path), true) => mask.intersect_path(&path, FillRule::Winding, true, transform),
            (Some(path), false) => mask.fill_path(&path, FillRule::Winding, true, transform),
            // An empty clip hides everything painted inside it.
            (None, _) => mask.clear(),
        }
        self.clips.push(Some(mask));
    }

    fn glyph_path(&self, glyph: GlyphId) -> Option<Path> {
        let outline = self.font.outline_glyphs().get(glyph)?;
        let mut pen = Pen(PathBuilder::new());
        outline
            .draw(
                DrawSettings::unhinted(Size::unscaled(), self.location),
                &mut pen,
            )
            .ok()?;
        pen.0.finish()
    }
}

fn spread(extend: Extend) -> SpreadMode {
    match extend {
        Extend::Repeat => SpreadMode::Repeat,
        Extend::Reflect => SpreadMode::Reflect,
        _ => SpreadMode::Pad,
    }
}

fn point(p: skrifa::raw::types::Point<f32>) -> Point {
    Point::from_xy(p.x, p.y)
}

fn blend(mode: CompositeMode) -> BlendMode {
    match mode {
        CompositeMode::Clear => BlendMode::Clear,
        CompositeMode::Src => BlendMode::Source,
        CompositeMode::Dest => BlendMode::Destination,
        CompositeMode::DestOver => BlendMode::DestinationOver,
        CompositeMode::SrcIn => BlendMode::SourceIn,
        CompositeMode::DestIn => BlendMode::DestinationIn,
        CompositeMode::SrcOut => BlendMode::SourceOut,
        CompositeMode::DestOut => BlendMode::DestinationOut,
        CompositeMode::SrcAtop => BlendMode::SourceAtop,
        CompositeMode::DestAtop => BlendMode::DestinationAtop,
        CompositeMode::Xor => BlendMode::Xor,
        CompositeMode::Plus => BlendMode::Plus,
        CompositeMode::Screen => BlendMode::Screen,
        CompositeMode::Overlay => BlendMode::Overlay,
        CompositeMode::Darken => BlendMode::Darken,
        CompositeMode::Lighten => BlendMode::Lighten,
        CompositeMode::ColorDodge => BlendMode::ColorDodge,
        CompositeMode::ColorBurn => BlendMode::ColorBurn,
        CompositeMode::HardLight => BlendMode::HardLight,
        CompositeMode::SoftLight => BlendMode::SoftLight,
        CompositeMode::Difference => BlendMode::Difference,
        CompositeMode::Exclusion => BlendMode::Exclusion,
        CompositeMode::Multiply => BlendMode::Multiply,
        CompositeMode::HslHue => BlendMode::Hue,
        CompositeMode::HslSaturation => BlendMode::Saturation,
        CompositeMode::HslColor => BlendMode::Color,
        CompositeMode::HslLuminosity => BlendMode::Luminosity,
        _ => BlendMode::SourceOver,
    }
}

impl ColorPainter for Painter<'_, '_> {
    fn push_transform(&mut self, t: skrifa::color::Transform) {
        let local = Transform::from_row(t.xx, t.yx, t.xy, t.yy, t.dx, t.dy);
        let current = self.transform().pre_concat(local);
        self.transforms.push(current);
    }

    fn pop_transform(&mut self) {
        if self.transforms.len() > 1 {
            self.transforms.pop();
        }
    }

    fn push_clip_glyph(&mut self, glyph: GlyphId) {
        let path = self.glyph_path(glyph);
        self.push_clip_path(path);
    }

    fn push_clip_box(&mut self, b: BoundingBox<f32>) {
        let path = Rect::from_ltrb(b.x_min, b.y_min, b.x_max, b.y_max).map(PathBuilder::from_rect);
        self.push_clip_path(path);
    }

    fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    fn fill(&mut self, brush: Brush<'_>) {
        let transform = self.transform();
        let shader = match brush {
            Brush::Solid {
                palette_index,
                alpha,
            } => Some(Shader::SolidColor(self.color(palette_index, alpha))),
            Brush::LinearGradient {
                p0,
                p1,
                color_stops,
                extend,
            } => gradient_or_solid(self, color_stops, |stops| {
                LinearGradient::new(point(p0), point(p1), stops, spread(extend), transform)
            }),
            Brush::RadialGradient {
                c0,
                r0,
                c1,
                r1,
                color_stops,
                extend,
            } => gradient_or_solid(self, color_stops, |stops| {
                RadialGradient::new(
                    point(c0),
                    r0.max(0.0),
                    point(c1),
                    r1.max(0.0),
                    stops,
                    spread(extend),
                    transform,
                )
            }),
            Brush::SweepGradient {
                c0,
                start_angle,
                end_angle,
                color_stops,
                extend,
            } => gradient_or_solid(self, color_stops, |stops| {
                SweepGradient::new(
                    point(c0),
                    start_angle,
                    end_angle,
                    stops,
                    spread(extend),
                    transform,
                )
            }),
        };
        let Some(shader) = shader else {
            return;
        };
        let (width, height) = self.size();
        let Some(rect) = Rect::from_xywh(0.0, 0.0, width as f32, height as f32) else {
            return;
        };
        let paint = Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        };
        let clip = self.clips.last().unwrap().clone();
        self.layers.last_mut().unwrap().fill_rect(
            rect,
            &paint,
            Transform::identity(),
            clip.as_ref(),
        );
    }

    fn push_layer(&mut self, _composite_mode: CompositeMode) {
        let (width, height) = self.size();
        self.layers.push(Pixmap::new(width, height).unwrap());
    }

    fn pop_layer_with_mode(&mut self, composite_mode: CompositeMode) {
        if self.layers.len() < 2 {
            return;
        }
        let layer = self.layers.pop().unwrap();
        self.layers.last_mut().unwrap().draw_pixmap(
            0,
            0,
            layer.as_ref(),
            &PixmapPaint {
                blend_mode: blend(composite_mode),
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
}

/// Gradients with fewer than two stops paint their single color.
fn gradient_or_solid(
    painter: &Painter,
    stops: &[ColorStop],
    gradient: impl FnOnce(Vec<GradientStop>) -> Option<Shader<'static>>,
) -> Option<Shader<'static>> {
    match stops {
        [] => None,
        [only] => Some(Shader::SolidColor(
            painter.color(only.palette_index, only.alpha),
        )),
        _ => gradient(painter.stops(stops)),
    }
}

/// Unflipped font-unit outlines; the painter's transform maps them to pixels.
struct Pen(PathBuilder);
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x: f32, y: f32, ex: f32, ey: f32) {
        self.0.quad_to(x, y, ex, ey);
    }
    fn curve_to(&mut self, x: f32, y: f32, x2: f32, y2: f32, ex: f32, ey: f32) {
        self.0.cubic_to(x, y, x2, y2, ex, ey);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
#[path = "color_tests.rs"]
mod tests;
