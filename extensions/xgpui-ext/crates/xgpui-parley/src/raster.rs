use crate::FaceRegistry;
use anyhow::{Context, Result, ensure};
use gpui::{
    Bounds, DevicePixels, RenderGlyphParams, SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y, Size, point,
    size,
};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size as FontSize},
    outline::{DrawSettings, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target},
};
use std::collections::VecDeque;
use tiny_skia::{FillRule, Mask, Path, PathBuilder, Transform};

const HINT_LIMIT: usize = 64;
const MAX_EXTENT: f32 = 4096.0;

#[derive(PartialEq, Eq)]
struct HintKey {
    blob: u64,
    index: u32,
    coords: Vec<i16>,
    ppem: u32,
}

/// Grayscale outline rasterization. Retain one instance per backend/scaler thread.
/// Only hinting setup is cached here (64 recent face/location/size instances).
/// GPUI's TextSystem and sprite atlas retain bounds and final glyph bitmaps.
#[derive(Default)]
pub struct GlyphRasterizer {
    hints: VecDeque<(HintKey, HintingInstance)>,
    #[cfg(test)]
    hint_builds: usize,
}

impl GlyphRasterizer {
    /// Baseline-relative, y-down device-pixel bounds, including subpixel positioning.
    /// Outlines are measured without allocating a bitmap. Empty glyphs have zero bounds.
    pub fn raster_bounds(
        &mut self,
        faces: &FaceRegistry,
        params: &RenderGlyphParams,
    ) -> Result<Bounds<DevicePixels>> {
        Ok(self.prepare(faces, params)?.1)
    }

    /// Produce a one-byte-per-pixel coverage mask matching previously measured bounds.
    /// Color emoji, LCD rendering and native dilation are explicitly unsupported.
    pub fn rasterize_glyph(
        &mut self,
        faces: &FaceRegistry,
        params: &RenderGlyphParams,
        expected: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        let (path, bounds) = self.prepare(faces, params)?;
        ensure!(bounds == expected, "glyph raster bounds changed");
        let Some(path) = path else {
            return Ok((bounds.size, Vec::new()));
        };
        let mut mask = Mask::new(bounds.size.width.0 as u32, bounds.size.height.0 as u32)
            .context("unable to allocate glyph mask")?;
        mask.fill_path(
            &path,
            FillRule::Winding,
            true,
            Transform::from_translate(-bounds.origin.x.0 as f32, -bounds.origin.y.0 as f32),
        );
        Ok((bounds.size, mask.data().to_vec()))
    }

    fn prepare(
        &mut self,
        faces: &FaceRegistry,
        params: &RenderGlyphParams,
    ) -> Result<(Option<Path>, Bounds<DevicePixels>)> {
        let ppem = f32::from(params.font_size) * params.scale_factor;
        ensure!(
            f32::from(params.font_size) > 0.0
                && params.scale_factor > 0.0
                && ppem > 0.0
                && ppem.is_finite()
                && ppem <= MAX_EXTENT,
            "invalid or excessive glyph size"
        );
        ensure!(
            !params.is_emoji && !params.subpixel_rendering && params.dilation == 0,
            "only grayscale outline masks without dilation are supported"
        );
        ensure!(
            params.subpixel_variant.x < SUBPIXEL_VARIANTS_X
                && params.subpixel_variant.y < SUBPIXEL_VARIANTS_Y,
            "invalid subpixel variant"
        );
        let face = faces.get(params.font_id)?;
        let font = FontRef::from_index(face.data().data.data(), face.data().index)?;
        let outlines = font.outline_glyphs();
        let glyph = outlines
            .get(skrifa::GlyphId::new(params.glyph_id.0))
            .context("missing glyph outline")?;
        let key = HintKey {
            blob: face.data().data.id(),
            index: face.data().index,
            coords: face
                .normalized_coords()
                .iter()
                .map(|coord| coord.to_bits())
                .collect(),
            ppem: ppem.to_bits(),
        };
        let hint = if let Some(index) = self.hints.iter().position(|(cached, _)| cached == &key) {
            self.hints.remove(index).unwrap().1
        } else {
            #[cfg(test)]
            {
                self.hint_builds += 1;
            }
            HintingInstance::new(
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
            )?
        };
        if self.hints.len() == HINT_LIMIT {
            self.hints.pop_front();
        }
        self.hints.push_back((key, hint));
        let mut pen = Pen(PathBuilder::new());
        glyph.draw(
            DrawSettings::hinted(&self.hints.back().unwrap().1, false),
            &mut pen,
        )?;
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
