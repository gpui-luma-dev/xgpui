//! Retained decoded raster with linear-light, source-sized float pages.
//! The caller owns decoding/profile resolution; this is not a RAW/ICC decoder.
use super::*;
use anyhow::{Result, ensure};
use half::f16;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One bounded CPU RGBA8 source. Mip rectangles are averaged on admitted tile
/// workers, without allocating or uploading a whole float image or mip pyramid.
/// Source pixels must have straight alpha and explicitly declared RGB metadata.
#[derive(Debug)]
pub struct RasterSource {
    pixels: image::RgbaImage,
    source: SourceId,
    descriptor: ImageDescriptor,
    decode: [[[f64; 3]; 256]; 3],
    black: [f64; 3],
}
impl RasterSource {
    pub fn new(
        pixels: image::RgbaImage,
        color: impl Into<PixelColor>,
        max_source_bytes: usize,
    ) -> Result<Self> {
        let color = color.into();
        let alpha = match &color {
            PixelColor::Named(c) => c.alpha,
            PixelColor::Custom { alpha, .. } => *alpha,
            PixelColor::ExtendedLinear { .. } => {
                anyhow::bail!("RGBA8 raster requires bounded encoded/linear RGB")
            }
        };
        ensure!(
            alpha == AlphaMode::Straight,
            "raster source requires straight alpha"
        );
        ensure!(
            pixels.as_raw().len() <= max_source_bytes,
            "raster source exceeds CPU pixel allowance"
        );
        let descriptor = ImageDescriptor::new(
            pixels.width(),
            pixels.height(),
            PixelFormat::Rgba16Float,
            ImageColor {
                primaries: ImagePrimaries::DisplayP3,
                transfer: TransferFunction::Linear,
                alpha: AlphaMode::Premultiplied,
            },
        )?;
        let black = color
            .decode([0., 0., 0., 1.], ImagePrimaries::DisplayP3)?
            .rgba();
        let black = [black[0], black[1], black[2]];
        let mut decode = [[[0.; 3]; 256]; 3];
        for channel in 0..3 {
            for value in 0..256 {
                let mut rgba = [0., 0., 0., 1.];
                rgba[channel] = value as f64 / 255.;
                let rgb = color.decode(rgba, ImagePrimaries::DisplayP3)?.rgba();
                decode[channel][value] = std::array::from_fn(|i| rgb[i] - black[i]);
            }
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            pixels,
            source: SourceId {
                asset: NEXT.fetch_add(1, Ordering::Relaxed),
                generation: 0,
            },
            descriptor,
            decode,
            black,
        })
    }
    /// Retained source pixel bytes; page/scratch/upload budgets belong to TileCanvas.
    pub fn source_bytes(&self) -> usize {
        self.pixels.as_raw().len()
    }
}
impl TileProvider for RasterSource {
    fn source(&self) -> SourceId {
        self.source
    }
    fn image(&self) -> ImageDescriptor {
        self.descriptor
    }
    fn tile(&self, tile: TileDescriptor, cancelled: &AtomicBool) -> Result<gpui::ImageTextureData> {
        ensure!(
            tile.key().source == self.source && tile.key().image == self.descriptor,
            "foreign raster tile"
        );
        let r = tile.upload();
        let [mw, mh] = self.descriptor.mip_size(tile.key().mip)?;
        let sx = self.pixels.width() as f64 / mw as f64;
        let sy = self.pixels.height() as f64 / mh as f64;
        let mut bytes = Vec::with_capacity(tile.payload_bytes() as usize);
        for y in r.y..r.y + r.height {
            let top = y as f64 * sy;
            let bottom = (y + 1) as f64 * sy;
            for x in r.x..r.x + r.width {
                let left = x as f64 * sx;
                let right = (x + 1) as f64 * sx;
                let mut sum = [0.; 4];
                for iy in top.floor() as u32..(bottom.ceil() as u32).min(self.pixels.height()) {
                    ensure!(!cancelled.load(Ordering::Acquire), "raster tile cancelled");
                    let wy = (bottom.min((iy + 1) as f64) - top.max(iy as f64)).max(0.);
                    for ix in left.floor() as u32..(right.ceil() as u32).min(self.pixels.width()) {
                        let p = self.pixels.get_pixel(ix, iy).0;
                        let weight =
                            wy * (right.min((ix + 1) as f64) - left.max(ix as f64)).max(0.);
                        let a = p[3] as f64 / 255.;
                        let red = self.decode[0][p[0] as usize];
                        let green = self.decode[1][p[1] as usize];
                        let blue = self.decode[2][p[2] as usize];
                        for i in 0..3 {
                            sum[i] += (self.black[i] + red[i] + green[i] + blue[i]) * a * weight;
                        }
                        sum[3] += a * weight;
                    }
                }
                let area = sx * sy;
                let alpha = (sum[3] / area).clamp(0., 1.);
                // Explicit bounded P3 display storage. Processing parameters are
                // applied at sampling; source pages are never regenerated for edits.
                for value in &sum[..3] {
                    let c = (*value / area).clamp(0., alpha);
                    bytes.extend_from_slice(&f16::from_f64(c).to_bits().to_le_bytes());
                }
                bytes.extend_from_slice(&f16::from_f64(alpha).to_bits().to_le_bytes());
            }
        }
        gpui::ImageTextureData::page(
            r.width,
            r.height,
            bytes,
            gpui::ImageTextureFormat::LinearP3Rgba16Float,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(source: &RasterSource, mip: u32, cancelled: bool) -> Result<gpui::ImageTextureData> {
        let pyramid = TilePyramid::new(source.image(), TileLayout::new(256, 1)?);
        let tile = pyramid.tile(source.source(), mip, 0, 0)?;
        // These fixtures have a single coarse pixel at the requested mip.
        assert_eq!(tile.key().mip, mip);
        source.tile(tile, &AtomicBool::new(cancelled))
    }
    #[test]
    fn odd_mip_averages_linear_associated_light_without_transparent_color_bleed() -> Result<()> {
        let image =
            image::RgbaImage::from_raw(3, 1, vec![255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 0])
                .unwrap();
        let source = RasterSource::new(image, ImageColor::SRGB, 12)?;
        let data = page(&source, 1, false)?;
        let c: Vec<f32> = data.levels()[0]
            .chunks_exact(2)
            .map(|b| f16::from_bits(u16::from_le_bytes([b[0], b[1]])).to_f32())
            .collect();
        for value in &c[..3] {
            assert!((*value - 1. / 3.).abs() < 0.001);
        }
        assert!((c[3] - 2. / 3.).abs() < 0.001);
        assert!(page(&source, 1, true).is_err());
        Ok(())
    }
    #[test]
    fn source_admission_and_identity_are_explicit() -> Result<()> {
        assert!(RasterSource::new(image::RgbaImage::new(2, 2), ImageColor::SRGB, 15).is_err());
        let a = RasterSource::new(image::RgbaImage::new(2, 2), ImageColor::SRGB, 16)?;
        let b = RasterSource::new(image::RgbaImage::new(2, 2), ImageColor::SRGB, 16)?;
        assert_ne!(a.source(), b.source());
        assert_eq!(a.source_bytes(), 16);
        Ok(())
    }
}

#[cfg(test)]
mod custom_color_tests {
    use super::*;
    #[test]
    fn custom_channel_curves_match_the_unclipped_color_reference_before_storage() -> Result<()> {
        let definition = xgpui_color::RgbDefinition::new_d65(
            xgpui_color::RgbSpace::AdobeRgb1998.linear_to_xyz_matrix(),
            [
                xgpui_color::RgbCurve::Gamma(1.8),
                xgpui_color::RgbCurve::Gamma(2.2),
                xgpui_color::RgbCurve::Gamma(2.4),
            ],
        )?;
        let color = PixelColor::Custom {
            rgb: std::sync::Arc::new(definition),
            alpha: AlphaMode::Straight,
        };
        let pixel = [80, 140, 210, 128];
        let reference = color
            .decode(pixel.map(|v| v as f64 / 255.), ImagePrimaries::DisplayP3)?
            .rgba();
        let source = RasterSource::new(
            image::RgbaImage::from_pixel(1, 1, image::Rgba(pixel)),
            color,
            4,
        )?;
        let tile = TilePyramid::new(source.image(), TileLayout::new(256, 1)?).tile(
            source.source(),
            0,
            0,
            0,
        )?;
        let data = source.tile(tile, &AtomicBool::new(false))?;
        let values: Vec<f64> = data.levels()[0]
            .chunks_exact(2)
            .map(|b| f16::from_bits(u16::from_le_bytes([b[0], b[1]])).to_f64())
            .collect();
        for i in 0..3 {
            assert!(
                (values[i] - (reference[i] * reference[3]).clamp(0., reference[3])).abs() < 0.001
            );
        }
        Ok(())
    }
}
