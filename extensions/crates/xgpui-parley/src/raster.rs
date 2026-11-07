use crate::{FaceRegistry, RegisteredFace};
use anyhow::{Context, Result, ensure};
use gpui::{
    Bounds, DevicePixels, RenderGlyphParams, SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y, Size, point,
    size,
};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, NormalizedCoord, Size as FontSize},
    outline::{DrawSettings, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target},
};
use std::sync::Arc;

pub(crate) mod color;
mod hints;
use hints::HintCache;
use tiny_skia::{FillRule, Mask, Path, PathBuilder, Transform};

const HINT_LIMIT: usize = 64;
const MAX_EXTENT: f32 = 4096.0;

#[derive(Clone, Hash, PartialEq, Eq)]
struct HintKey {
    blob: u64,
    index: u32,
    coords: Arc<[NormalizedCoord]>,
    ppem: u32,
}

/// Grayscale outline and color glyph rasterization. Retain one instance per backend/scaler thread.
/// Retains 64 hinting instances and one prepared outline for bounds-to-raster reuse.
/// GPUI's TextSystem and sprite atlas retain bounds and final glyph bitmaps.
#[derive(Default)]
pub struct GlyphRasterizer {
    hints: HintCache,
    prepared: Option<PreparedOutline>,
    #[cfg(test)]
    outline_builds: usize,
    #[cfg(test)]
    hint_builds: usize,
}

struct PreparedOutline {
    params: RenderGlyphParams,
    blob: u64,
    index: u32,
    coords: Arc<[NormalizedCoord]>,
    content: Content,
    bounds: Bounds<DevicePixels>,
}

enum Content {
    /// A coverage outline; `None` for empty glyphs.
    Outline(Option<Path>),
    /// BGRA straight-alpha pixels for a color glyph.
    Color(Vec<u8>),
}

impl GlyphRasterizer {
    /// Baseline-relative, y-down device-pixel bounds, including subpixel positioning.
    /// Outlines are measured without allocating a bitmap. Empty glyphs have zero bounds.
    pub fn raster_bounds(
        &mut self,
        faces: &FaceRegistry,
        params: &RenderGlyphParams,
    ) -> Result<Bounds<DevicePixels>> {
        self.raster_bounds_for_face(faces.get(params.font_id)?, params)
    }

    pub(crate) fn raster_bounds_for_face(
        &mut self,
        face: &RegisteredFace,
        params: &RenderGlyphParams,
    ) -> Result<Bounds<DevicePixels>> {
        self.prepare_cached(face, params)?;
        Ok(self.prepared.as_ref().unwrap().bounds)
    }

    /// Produce a one-byte-per-pixel coverage mask, or BGRA pixels for emoji requests,
    /// matching previously measured bounds. LCD rendering and dilation are unsupported.
    pub fn rasterize_glyph(
        &mut self,
        faces: &FaceRegistry,
        params: &RenderGlyphParams,
        expected: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.rasterize_face(faces.get(params.font_id)?, params, expected)
    }

    pub(crate) fn rasterize_face(
        &mut self,
        face: &RegisteredFace,
        params: &RenderGlyphParams,
        expected: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.prepare_cached(face, params)?;
        let PreparedOutline {
            content, bounds, ..
        } = self.prepared.take().unwrap();
        ensure!(bounds == expected, "glyph raster bounds changed");
        let path = match content {
            Content::Color(bgra) => return Ok((bounds.size, bgra)),
            Content::Outline(None) => return Ok((bounds.size, Vec::new())),
            Content::Outline(Some(path)) => path,
        };
        let mut mask = Mask::new(bounds.size.width.0 as u32, bounds.size.height.0 as u32)
            .context("unable to allocate glyph mask")?;
        mask.fill_path(
            &path,
            FillRule::Winding,
            true,
            Transform::from_translate(-bounds.origin.x.0 as f32, -bounds.origin.y.0 as f32),
        );
        Ok((bounds.size, mask.take()))
    }

    fn prepare_cached(&mut self, face: &RegisteredFace, params: &RenderGlyphParams) -> Result<()> {
        if self.prepared.as_ref().is_some_and(|cached| {
            cached.params == *params
                && cached.blob == face.data().data.id()
                && cached.index == face.data().index
                && cached.coords.as_ref() == face.normalized_coords()
        }) {
            return Ok(());
        }
        // Drop the previous outline before allocating its replacement.
        self.prepared = None;
        let (content, bounds) = if params.is_emoji {
            self.prepare_color(face, params)?
        } else {
            let (path, bounds) = self.prepare(face, params)?;
            (Content::Outline(path), bounds)
        };
        self.prepared = Some(PreparedOutline {
            params: params.clone(),
            blob: face.data().data.id(),
            index: face.data().index,
            coords: face.coordinate_handle(),
            content,
            bounds,
        });
        Ok(())
    }

    /// Color glyphs ignore subpixel variants: GPUI places emoji at integer origins.
    fn prepare_color(
        &mut self,
        face: &RegisteredFace,
        params: &RenderGlyphParams,
    ) -> Result<(Content, Bounds<DevicePixels>)> {
        let ppem = valid_ppem(params)?;
        ensure!(
            !params.subpixel_rendering && params.dilation == 0,
            "color glyphs support neither LCD rendering nor dilation"
        );
        let font = FontRef::from_index(face.data().data.data(), face.data().index)?;
        let raster = color::render(&font, face.normalized_coords(), params.glyph_id.0, ppem)?
            .context("glyph has no color data")?;
        Ok((
            Content::Color(raster.bgra),
            Bounds {
                origin: point(DevicePixels(raster.left), DevicePixels(raster.top)),
                size: size(
                    DevicePixels(raster.width as i32),
                    DevicePixels(raster.height as i32),
                ),
            },
        ))
    }

    fn prepare(
        &mut self,
        face: &RegisteredFace,
        params: &RenderGlyphParams,
    ) -> Result<(Option<Path>, Bounds<DevicePixels>)> {
        let ppem = valid_ppem(params)?;
        ensure!(
            !params.subpixel_rendering && params.dilation == 0,
            "only grayscale outline masks without dilation are supported"
        );
        ensure!(
            params.subpixel_variant.x < SUBPIXEL_VARIANTS_X
                && params.subpixel_variant.y < SUBPIXEL_VARIANTS_Y,
            "invalid subpixel variant"
        );
        let font = FontRef::from_index(face.data().data.data(), face.data().index)?;
        let outlines = font.outline_glyphs();
        let glyph = outlines
            .get(skrifa::GlyphId::new(params.glyph_id.0))
            .context("missing glyph outline")?;
        let key = HintKey {
            blob: face.data().data.id(),
            index: face.data().index,
            coords: face.coordinate_handle(),
            ppem: ppem.to_bits(),
        };
        let hint = self.hints.get_or_insert(key, || {
            #[cfg(test)]
            {
                self.hint_builds += 1;
            }
            Ok(HintingInstance::new(
                &outlines,
                FontSize::new(ppem),
                LocationRef::new(face.normalized_coords()),
                HintingOptions {
                    target: Target::Smooth {
                        mode: SmoothMode::Normal,
                        symmetric_rendering: false,
                        preserve_linear_metrics: true,
                    },
                    ..Default::default()
                },
            )?)
        })?;
        #[cfg(test)]
        {
            self.outline_builds += 1;
        }
        let mut pen = Pen(PathBuilder::new());
        glyph.draw(DrawSettings::hinted(hint, false), &mut pen)?;
        let Some(path) = pen.0.finish() else {
            return Ok((None, Bounds::default()));
        };
        let path = path
            .transform(Transform::from_translate(
                params.subpixel_variant.x as f32 / SUBPIXEL_VARIANTS_X as f32,
                params.subpixel_variant.y as f32 / SUBPIXEL_VARIANTS_Y as f32,
            ))
            .context("invalid glyph path")?;
        let b = path.bounds();
        let left = b.left().floor();
        let top = b.top().floor();
        let width = b.right().ceil() - left;
        let height = b.bottom().ceil() - top;
        ensure!(
            left.is_finite()
                && top.is_finite()
                && left.abs() <= MAX_EXTENT
                && top.abs() <= MAX_EXTENT
                && width <= MAX_EXTENT
                && height <= MAX_EXTENT,
            "glyph bounds exceed raster limits"
        );
        if width <= 0.0 || height <= 0.0 {
            return Ok((None, Bounds::default()));
        }
        Ok((
            Some(path),
            Bounds {
                origin: point(DevicePixels(left as i32), DevicePixels(top as i32)),
                size: size(DevicePixels(width as i32), DevicePixels(height as i32)),
            },
        ))
    }
}

fn valid_ppem(params: &RenderGlyphParams) -> Result<f32> {
    let ppem = f32::from(params.font_size) * params.scale_factor;
    ensure!(
        f32::from(params.font_size) > 0.0
            && params.scale_factor > 0.0
            && ppem > 0.0
            && ppem.is_finite()
            && ppem <= MAX_EXTENT,
        "invalid or excessive glyph size"
    );
    Ok(ppem)
}

// Font outlines are y-up; GPUI raster images use y-down coordinates.
struct Pen(PathBuilder);
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, -y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, -y);
    }
    fn quad_to(&mut self, x: f32, y: f32, ex: f32, ey: f32) {
        self.0.quad_to(x, -y, ex, -ey);
    }
    fn curve_to(&mut self, x: f32, y: f32, x2: f32, y2: f32, ex: f32, ey: f32) {
        self.0.cubic_to(x, -y, x2, -y2, ex, -ey);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests;
