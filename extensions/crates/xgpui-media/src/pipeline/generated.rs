//! A deterministic 72 MP provider for offline examples and correctness tests.
use super::*;
use anyhow::{Result, ensure};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug)]
pub struct GeneratedCanvas {
    source: SourceId,
    image: ImageDescriptor,
}
impl GeneratedCanvas {
    pub fn new(generation: u64) -> Self {
        Self {
            source: SourceId {
                asset: 2604,
                generation,
            },
            image: ImageDescriptor::new(12000, 6000, PixelFormat::Rgba8, ImageColor::SRGB).unwrap(),
        }
    }
    /// Analytic full-source function, evaluated directly at selected mip centers.
    /// A real provider supplies prefiltered pyramid pages from its own data.
    pub fn sample(&self, x: f64, y: f64) -> [u8; 4] {
        let u = x / self.image.width() as f64;
        let v = y / self.image.height() as f64;
        let grid = x.rem_euclid(512.) < 10. || y.rem_euclid(512.) < 10.;
        let fine = (x / 32.).floor() as u64 % 2 == (y / 32.).floor() as u64 % 2;
        let gain = if grid {
            0.25
        } else if fine {
            1.
        } else {
            0.78
        };
        let swap = !self.source.generation.is_multiple_of(2);
        let c = [
            (0.1 + u * 0.9) * gain,
            (0.2 + v * 0.8) * gain,
            (0.95 - u * 0.65) * gain,
        ];
        let c = if swap { [c[2], c[0], c[1]] } else { c };
        [
            (c[0] * 255.) as u8,
            (c[1] * 255.) as u8,
            (c[2] * 255.) as u8,
            255,
        ]
    }
}
impl TileProvider for GeneratedCanvas {
    fn source(&self) -> SourceId {
        self.source
    }
    fn image(&self) -> ImageDescriptor {
        self.image
    }
    fn tile(&self, tile: TileDescriptor, cancelled: &AtomicBool) -> Result<gpui::ImageTextureData> {
        ensure!(
            tile.key().source == self.source && tile.key().image == self.image,
            "wrong generated source request"
        );
        let r = tile.upload();
        let [w, h] = self.image.mip_size(tile.key().mip)?;
        let mut bytes = Vec::with_capacity(tile.payload_bytes() as usize);
        for y in r.y..r.y + r.height {
            ensure!(
                !cancelled.load(Ordering::Acquire),
                "generated tile cancelled"
            );
            for x in r.x..r.x + r.width {
                bytes.extend_from_slice(&self.sample(
                    (x as f64 + 0.5) * self.image.width() as f64 / w as f64,
                    (y as f64 + 0.5) * self.image.height() as f64 / h as f64,
                ));
            }
        }
        gpui::ImageTextureData::page(
            r.width,
            r.height,
            bytes,
            gpui::ImageTextureFormat::Rgba8Srgb,
        )
    }
}
