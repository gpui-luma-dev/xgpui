use super::{ImageColor, SourceId};
use anyhow::{Result, ensure};

/// Pixel storage, independent of transfer function, gamut and alpha semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    Rgba8,
    Rgba16Float,
}
impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::Rgba8 => 4,
            Self::Rgba16Float => 8,
        }
    }
}
/// Metadata only; no full-resolution allocation or backend support is implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageDescriptor {
    width: u32,
    height: u32,
    pub format: PixelFormat,
    pub color: ImageColor,
}
impl ImageDescriptor {
    pub fn new(width: u32, height: u32, format: PixelFormat, color: ImageColor) -> Result<Self> {
        ensure!(width > 0 && height > 0, "image dimensions must be nonzero");
        Ok(Self {
            width,
            height,
            format,
            color,
        })
    }
    pub fn width(self) -> u32 {
        self.width
    }
    pub fn height(self) -> u32 {
        self.height
    }
    /// Full chain uses floor-halving, matching the retained RGBA8 path.
    pub fn mip_count(self) -> u32 {
        32 - self.width.max(self.height).leading_zeros()
    }
    pub fn mip_size(self, mip: u32) -> Result<[u32; 2]> {
        ensure!(mip < self.mip_count(), "mip level outside image pyramid");
        Ok([(self.width >> mip).max(1), (self.height >> mip).max(1)])
    }
    /// Unpadded full-chain payload for planning only. Reject arithmetic overflow;
    /// this does not make a whole-image upload legal under existing limits.
    pub fn full_mip_bytes(self) -> Result<u64> {
        let mut bytes = 0_u64;
        for mip in 0..self.mip_count() {
            let [w, h] = self.mip_size(mip)?;
            let level = u64::from(w)
                .checked_mul(u64::from(h))
                .and_then(|v| v.checked_mul(self.format.bytes_per_pixel()));
            bytes = bytes
                .checked_add(level.ok_or_else(|| anyhow::anyhow!("image byte count overflow"))?)
                .ok_or_else(|| anyhow::anyhow!("image byte count overflow"))?;
        }
        Ok(bytes)
    }
}
/// Logical page edge and filter overlap, measured at each mip's resolution.
/// Request sufficient halo for the chosen filter; allocation does not add it
/// implicitly. Physical sparse textures are not required by this contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileLayout {
    edge: u32,
    halo: u32,
}
impl TileLayout {
    pub fn new(edge: u32, halo: u32) -> Result<Self> {
        ensure!(
            edge > 0 && u64::from(edge) + 2 * u64::from(halo) <= 16384,
            "invalid tile edge/halo (maximum physical extent 16384)"
        );
        Ok(Self { edge, halo })
    }
    pub fn edge(self) -> u32 {
        self.edge
    }
    pub fn halo(self) -> u32 {
        self.halo
    }
}
/// Source-page identity. Adjustment parameters are deliberately excluded: changing
/// a draw's matrix/vignette must not invalidate raw source uploads. A future cache
/// of *processed* intermediates must additionally include all effect parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub source: SourceId,
    pub image: ImageDescriptor,
    pub layout: TileLayout,
    pub mip: u32,
    pub x: u32,
    pub y: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
/// One validated mip page: content bounds exclude overlap; upload bounds include
/// halo clipped to the mip image. Both use full-mip pixel coordinates. This is
/// uncompressed RGBA accounting, not a total process/driver-memory ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileDescriptor {
    key: TileKey,
    content: PixelRect,
    upload: PixelRect,
    payload_bytes: u64,
    staging_bytes: u64,
}
impl TileDescriptor {
    pub fn key(self) -> TileKey {
        self.key
    }
    pub fn content(self) -> PixelRect {
        self.content
    }
    pub fn upload(self) -> PixelRect {
        self.upload
    }
    pub fn payload_bytes(self) -> u64 {
        self.payload_bytes
    }
    /// Includes 256-byte row alignment, even for a narrow edge page.
    pub fn staging_bytes(self) -> u64 {
        self.staging_bytes
    }
}
#[derive(Clone, Copy, Debug)]
pub struct TilePyramid {
    image: ImageDescriptor,
    layout: TileLayout,
}
impl TilePyramid {
    pub fn new(image: ImageDescriptor, layout: TileLayout) -> Self {
        Self { image, layout }
    }
    pub fn tile_count(self, mip: u32) -> Result<[u32; 2]> {
        let [w, h] = self.image.mip_size(mip)?;
        Ok([w.div_ceil(self.layout.edge), h.div_ceil(self.layout.edge)])
    }
    pub fn tile(self, source: SourceId, mip: u32, x: u32, y: u32) -> Result<TileDescriptor> {
        let [w, h] = self.image.mip_size(mip)?;
        let [cols, rows] = self.tile_count(mip)?;
        ensure!(x < cols && y < rows, "tile outside mip bounds");
        // These products are bounded by w/h after the grid checks above.
        let left = x * self.layout.edge;
        let top = y * self.layout.edge;
        let right = (u64::from(left) + u64::from(self.layout.edge)).min(u64::from(w)) as u32;
        let bottom = (u64::from(top) + u64::from(self.layout.edge)).min(u64::from(h)) as u32;
        let content = PixelRect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        };
        let ux = left.saturating_sub(self.layout.halo);
        let uy = top.saturating_sub(self.layout.halo);
        let ur = right.saturating_add(self.layout.halo).min(w);
        let ub = bottom.saturating_add(self.layout.halo).min(h);
        let upload = PixelRect {
            x: ux,
            y: uy,
            width: ur - ux,
            height: ub - uy,
        };
        let row_bytes = u64::from(upload.width) * self.image.format.bytes_per_pixel();
        let payload_bytes = row_bytes * u64::from(upload.height);
        let staging_bytes = row_bytes.div_ceil(256) * 256 * u64::from(upload.height);
        Ok(TileDescriptor {
            key: TileKey {
                source,
                image: self.image,
                layout: self.layout,
                mip,
                x,
                y,
            },
            content,
            upload,
            payload_bytes,
            staging_bytes,
        })
    }
}

/// Independent future scheduler budgets. CPU bytes include retained decoded
/// pages plus reserved decode output/scratch. GPU bytes include cached, visible
/// and in-flight pages until safe retirement. Staging includes in-flight and idle
/// pooled allocations. Encoded input/codec internals and driver overhead require
/// separate provider/backend limits; these fields are not a total RSS/VRAM cap.
/// No allocator/scheduler is enabled by constructing this policy.
#[derive(Clone, Copy, Debug)]
pub struct TileBudgets {
    pub cpu_bytes: u64,
    pub gpu_bytes: u64,
    pub staging_bytes: u64,
    pub upload_bytes_per_frame: u64,
    pub uploads_per_frame: u32,
    pub decode_jobs: u32,
}
impl Default for TileBudgets {
    fn default() -> Self {
        Self {
            cpu_bytes: 128 * 1024 * 1024,
            gpu_bytes: 256 * 1024 * 1024,
            staging_bytes: 32 * 1024 * 1024,
            upload_bytes_per_frame: 8 * 1024 * 1024,
            uploads_per_frame: 4,
            decode_jobs: 2,
        }
    }
}
impl TileBudgets {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.cpu_bytes > 0
                && self.gpu_bytes > 0
                && self.staging_bytes > 0
                && self.upload_bytes_per_frame > 0
                && self.uploads_per_frame > 0
                && self.decode_jobs > 0,
            "tile budgets must be nonzero"
        );
        Ok(())
    }
    /// Minimum feasibility check, not a reservation or proof of available memory.
    /// A page must fit each pool and one frame; codec scratch may need more CPU
    /// space. Charge aligned staging bytes against per-frame transfer work.
    pub fn check_tile(self, tile: &TileDescriptor) -> Result<()> {
        self.validate()?;
        ensure!(
            tile.payload_bytes <= self.cpu_bytes
                && tile.payload_bytes <= self.gpu_bytes
                && tile.staging_bytes <= self.staging_bytes
                && tile.staging_bytes <= self.upload_bytes_per_frame,
            "tile exceeds a memory or frame upload budget"
        );
        Ok(())
    }
}
