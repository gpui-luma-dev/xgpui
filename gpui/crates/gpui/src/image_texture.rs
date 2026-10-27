//! Validated immutable upload data for opt-in source-sized image textures.
use anyhow::{Result, ensure};
use std::sync::Arc;

/// Storage and color contract for source-sized image textures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageTextureFormat {
    /// Straight alpha, encoded sRGB.
    Rgba8Srgb,
    /// Premultiplied, bounded linear Display P3 / D65, little-endian binary16.
    LinearP3Rgba16Float,
}

/// Full mip chain with a validated storage, color and alpha contract.
/// This is an upload description, not a device resource. Backends retain normal
/// scene ordering and coverage when sampling it at a different display size.
#[derive(Clone, Debug)]
pub struct ImageTextureData {
    width: u32,
    height: u32,
    levels: Arc<[Vec<u8>]>,
    byte_len: usize,
    format: ImageTextureFormat,
}
impl ImageTextureData {
    /// Maximum combined mip bytes for one source (64 MiB).
    pub const MAX_BYTES: usize = 64 * 1024 * 1024;
    /// Validate a complete mip chain, including odd and one-dimensional sizes.
    /// Every level halves each dimension, rounding down and clamping to one.
    pub fn new(width: u32, height: u32, levels: Vec<Vec<u8>>) -> Result<Self> {
        Self::with_format(width, height, levels, ImageTextureFormat::Rgba8Srgb, false)
    }
    /// Validate a complete bounded premultiplied linear-P3 RGBA16F mip chain.
    /// Rejects non-finite, negative, out-of-range and invalid associated pixels.
    pub fn new_linear_p3_rgba16f(width: u32, height: u32, levels: Vec<Vec<u8>>) -> Result<Self> {
        Self::with_format(
            width,
            height,
            levels,
            ImageTextureFormat::LinearP3Rgba16Float,
            false,
        )
    }
    /// A single immutable mip page; the canvas selects the pyramid level.
    pub fn page(
        width: u32,
        height: u32,
        bytes: Vec<u8>,
        format: ImageTextureFormat,
    ) -> Result<Self> {
        Self::with_format(width, height, vec![bytes], format, true)
    }
    fn with_format(
        width: u32,
        height: u32,
        levels: Vec<Vec<u8>>,
        format: ImageTextureFormat,
        page: bool,
    ) -> Result<Self> {
        let bpp = if format == ImageTextureFormat::Rgba8Srgb {
            4
        } else {
            8
        };
        ensure!(
            width > 0 && height > 0 && width <= 16384 && height <= 16384,
            "invalid image texture dimensions"
        );
        let count = (32 - width.max(height).leading_zeros()) as usize;
        ensure!(
            levels.len() == if page { 1 } else { count },
            "image requires a complete mip chain or an explicit single-level page"
        );
        let mut byte_len = 0usize;
        for (level, bytes) in levels.iter().enumerate() {
            let w = (width >> level).max(1) as usize;
            let h = (height >> level).max(1) as usize;
            ensure!(bytes.len() == w * h * bpp, "invalid image mip byte count");
            if bpp == 8 {
                for pixel in bytes.chunks_exact(8) {
                    let c: [u16; 4] = std::array::from_fn(|i| {
                        u16::from_le_bytes([pixel[i * 2], pixel[i * 2 + 1]])
                    });
                    ensure!(
                        c[3] <= 0x3c00 && c[..3].iter().all(|v| *v <= c[3]),
                        "RGBA16F requires bounded premultiplied linear P3"
                    );
                }
            }
            byte_len += bytes.len();
            ensure!(byte_len <= Self::MAX_BYTES, "image texture exceeds 64 MiB");
        }
        Ok(Self {
            width,
            height,
            levels: levels.into(),
            byte_len,
            format,
        })
    }
    /// Validated source storage/color contract.
    pub fn format(&self) -> ImageTextureFormat {
        self.format
    }
    /// Unpadded bytes per source pixel.
    pub fn bytes_per_pixel(&self) -> usize {
        if self.format == ImageTextureFormat::Rgba8Srgb {
            4
        } else {
            8
        }
    }
    /// Source width in physical pixels.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Source height in physical pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
    /// Complete immutable mip chain, largest level first.
    pub fn levels(&self) -> &[Vec<u8>] {
        &self.levels
    }
    /// Bytes in a 256-byte-row-aligned staging buffer for the complete chain.
    pub fn staging_byte_len(&self) -> usize {
        (0..self.levels.len())
            .map(|level| {
                let row = (((self.width >> level).max(1) as usize) * self.bytes_per_pixel())
                    .div_ceil(256)
                    * 256;
                row * (self.height >> level).max(1) as usize
            })
            .sum()
    }
    /// Combined unpadded upload and GPU payload size.
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_full_odd_and_thin_chains() {
        assert!(ImageTextureData::new(3, 1, vec![vec![0; 12], vec![0; 4]]).is_ok());
        assert!(ImageTextureData::new(1, 4, vec![vec![0; 16], vec![0; 8], vec![0; 4]]).is_ok());
        assert!(ImageTextureData::new(3, 1, vec![vec![0; 12]]).is_err());
        assert!(ImageTextureData::new(0, 1, vec![]).is_err());
        assert!(ImageTextureData::new(1, 1, vec![vec![0; 3]]).is_err());
    }
}

/// Shared admission counters for one scene preparation. Old custom producers
/// ignore this optional context; image producers can share loader-specific limits.
#[derive(Default)]
pub struct PreparationFrame {
    uploads: std::sync::Mutex<std::collections::HashMap<u64, (usize, usize)>>,
}
impl PreparationFrame {
    /// Charge one source handoff. The key identifies a retained policy owner.
    /// A rejected reservation changes no counters. Already-retained outputs do
    /// not need a reservation; limits apply only to newly recorded image copies.
    pub fn try_reserve_upload(
        &self,
        key: u64,
        bytes: usize,
        max_bytes: usize,
        max_uploads: usize,
    ) -> bool {
        let mut uploads = self.uploads.lock().unwrap();
        let usage = uploads.entry(key).or_default();
        if usage.1 >= max_uploads || bytes > max_bytes.saturating_sub(usage.0) {
            return false;
        }
        usage.0 += bytes;
        usage.1 += 1;
        true
    }
}

/// Resource-only cache statistics. All allocated resources, including idle and
/// GPU-borrowed ones, count against capacity until evicted or the pool is dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageStagingPoolStats {
    /// Capacity currently owned by the pool and its leases.
    pub allocated_bytes: usize,
    /// Bytes currently idle and eligible for reuse.
    pub cached_bytes: usize,
    /// Successful backend resource allocations.
    pub allocations: usize,
    /// Successful checkouts of a previously idle resource.
    pub reuses: usize,
}
struct PoolState<T> {
    idle: Vec<(u64, usize, T)>,
    stats: ImageStagingPoolStats,
}
/// Backend SPI for bounded reusable staging resources. A lease may return to
/// this cache only after the host's completion guard has released its last Arc.
/// Backends own resource creation, mapping, synchronization and device identity.
pub struct ImageStagingPool<T> {
    capacity: usize,
    state: std::sync::Mutex<PoolState<T>>,
}
/// Exclusive staging ownership; sharing this lease delays reuse until all
/// completion guards and staging snapshots have released it.
pub struct ImageStagingLease<T> {
    resource: Option<T>,
    reused: bool,
    pool: std::sync::Weak<ImageStagingPool<T>>,
    key: u64,
    bytes: usize,
}
struct AllocationReservation<'a, T> {
    pool: &'a ImageStagingPool<T>,
    bytes: usize,
    committed: bool,
}
impl<T> Drop for AllocationReservation<'_, T> {
    fn drop(&mut self) {
        if !self.committed {
            self.pool.state.lock().unwrap().stats.allocated_bytes -= self.bytes;
        }
    }
}
impl<T> ImageStagingPool<T> {
    /// Create an empty cache with a hard resource-byte capacity.
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            capacity,
            state: std::sync::Mutex::new(PoolState {
                idle: vec![],
                stats: ImageStagingPoolStats::default(),
            }),
        })
    }
    /// Allocate outside the pool lock. Idle mismatches are evicted to make room;
    /// borrowed resources are never evicted or reused. Errors roll back capacity.
    pub fn checkout(
        self: &Arc<Self>,
        key: u64,
        bytes: usize,
        create: impl FnOnce() -> Result<T>,
    ) -> Result<ImageStagingLease<T>> {
        ensure!(
            bytes > 0 && bytes <= self.capacity,
            "invalid staging resource size"
        );
        let mut state = self.state.lock().unwrap();
        if let Some(index) = state
            .idle
            .iter()
            .position(|(k, b, _)| *k == key && *b == bytes)
        {
            let (_, _, resource) = state.idle.swap_remove(index);
            state.stats.cached_bytes -= bytes;
            state.stats.reuses += 1;
            return Ok(ImageStagingLease {
                resource: Some(resource),
                reused: true,
                pool: Arc::downgrade(self),
                key,
                bytes,
            });
        }
        ensure!(
            bytes <= self.capacity,
            "staging resource exceeds pool capacity"
        );
        while bytes > self.capacity.saturating_sub(state.stats.allocated_bytes) {
            let Some((_, evicted, resource)) = state.idle.pop() else {
                anyhow::bail!("staging pool is busy");
            };
            state.stats.allocated_bytes -= evicted;
            state.stats.cached_bytes -= evicted;
            drop(resource);
        }
        state.stats.allocated_bytes += bytes;
        drop(state);
        let mut reservation = AllocationReservation {
            pool: self,
            bytes,
            committed: false,
        };
        let resource = create()?;
        self.state.lock().unwrap().stats.allocations += 1;
        reservation.committed = true;
        Ok(ImageStagingLease {
            resource: Some(resource),
            reused: false,
            pool: Arc::downgrade(self),
            key,
            bytes,
        })
    }

    /// Read allocation and idle-cache counters.
    pub fn stats(&self) -> ImageStagingPoolStats {
        self.state.lock().unwrap().stats
    }
}
impl<T> ImageStagingLease<T> {
    /// Whether this checkout reuses a resource instead of allocating one.
    pub fn was_reused(&self) -> bool {
        self.reused
    }
    /// Access the backend resource while this exclusive lease is held.
    pub fn resource(&self) -> &T {
        self.resource.as_ref().unwrap()
    }
    /// Discard a mapping/driver failure instead of returning a resource whose
    /// state is no longer suitable for reuse.
    pub fn discard(mut self) {
        self.resource.take();
        if let Some(pool) = self.pool.upgrade() {
            pool.state.lock().unwrap().stats.allocated_bytes -= self.bytes;
        }
    }
}
impl<T> Drop for ImageStagingLease<T> {
    fn drop(&mut self) {
        if let Some(resource) = self.resource.take()
            && let Some(pool) = self.pool.upgrade()
        {
            let mut state = pool.state.lock().unwrap();
            state.stats.cached_bytes += self.bytes;
            state.idle.push((self.key, self.bytes, resource));
        }
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn frame_admission_shares_counts_and_bytes_without_charging_rejection() {
        let frame = PreparationFrame::default();
        assert!(frame.try_reserve_upload(1, 6, 10, 2));
        assert!(!frame.try_reserve_upload(1, 5, 10, 2));
        assert!(frame.try_reserve_upload(1, 4, 10, 2));
        assert!(!frame.try_reserve_upload(1, 0, 10, 2));
        assert!(frame.try_reserve_upload(2, 10, 10, 1));
        assert!(PreparationFrame::default().try_reserve_upload(1, 10, 10, 1));
    }
    #[test]
    fn pool_honors_outstanding_leases_eviction_and_failed_allocations() -> Result<()> {
        let pool = ImageStagingPool::new(16);
        let lease = Arc::new(pool.checkout(1, 16, || Ok(vec![7; 16]))?);
        let guard = lease.clone();
        drop(lease);
        assert!(pool.checkout(1, 16, || Ok(vec![0; 16])).is_err());
        drop(guard);
        let reused = pool.checkout(1, 16, || panic!("must reuse"))?;
        assert!(reused.was_reused());
        assert_eq!(reused.resource(), &vec![7; 16]);
        drop(reused);
        let failure: Result<ImageStagingLease<Vec<u8>>> =
            pool.checkout(2, 8, || anyhow::bail!("allocation failed"));
        assert!(failure.is_err());
        assert_eq!(pool.stats().allocated_bytes, 0);
        let discarded = pool.checkout(3, 8, || Ok(vec![3; 8]))?;
        discarded.discard();
        assert_eq!(pool.stats().allocated_bytes, 0);
        assert_eq!(pool.stats().cached_bytes, 0);
        assert_eq!(pool.stats().reuses, 1);
        let panic = std::panic::catch_unwind(|| {
            let _ = pool.checkout(4, 8, || -> Result<Vec<u8>> {
                panic!("driver allocation panic")
            });
        });
        assert!(panic.is_err());
        assert_eq!(pool.stats().allocated_bytes, 0);
        Ok(())
    }
}

/// Opt-in linear-sRGB image sampling parameters. Only bounded encoded-sRGB
/// sources are supported here; this is not a wide/HDR texture descriptor.
/// RGB rows multiply decoded [R,G,B,1], then vignette darkens, then RGB clips
/// and encodes to SDR sRGB. Alpha is preserved before native coverage/opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearImagePaint {
    rows: [[f32; 4]; 3],
    vignette: [f32; 4],
    p3: bool,
}
impl LinearImagePaint {
    /// Vignette = intensity, inner radius, positive feather, roundness. Radius
    /// units refer to the shortest source edge; roundness 0 follows its ellipse.
    /// Reject coefficients too large for finite f32 sampling arithmetic.
    pub fn new(rows: [[f32; 4]; 3], vignette: [f32; 4]) -> Result<Self> {
        ensure!(
            rows.iter().all(|row| row.iter().all(|v| v.is_finite())
                && row.iter().map(|v| f64::from(v.abs())).sum::<f64>() <= f64::from(f32::MAX) / 4.),
            "image matrix exceeds f32 sampling range"
        );
        let [intensity, radius, feather, roundness] = vignette;
        ensure!(
            vignette.iter().all(|v| v.is_finite())
                && (0.0..=1.0).contains(&intensity)
                && radius >= 0.
                && feather >= f32::MIN_POSITIVE
                && (0.0..=1.0).contains(&roundness),
            "invalid image vignette"
        );
        Ok(Self {
            rows,
            vignette,
            p3: false,
        })
    }
    /// Select linear Display P3 working primaries.
    pub fn with_p3(mut self, p3: bool) -> Self {
        self.p3 = p3;
        self
    }
    /// Whether rows operate in linear Display P3.
    pub fn is_p3(self) -> bool {
        self.p3
    }
    /// Affine rows evaluated in the selected linear working primaries.
    pub fn rows(self) -> [[f32; 4]; 3] {
        self.rows
    }
    /// Intensity, inner radius, feather and roundness in source-image units.
    pub fn vignette(self) -> [f32; 4] {
        self.vignette
    }
}

/// Source interpretation and requested presentation. Wide requires an accepted
/// native wide-SDR window; browser/wgpu hosts must reject it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageColorMode {
    #[default]
    /// Straight encoded-sRGB input.
    EncodedSrgb,
    /// Associated linear-P3 input mapped to clipped sRGB.
    LinearP3Preview,
    /// Associated linear-P3 input presented by the native wide-SDR compositor.
    LinearP3Wide,
}
impl ImageColorMode {
    /// Stable renderer uniform value.
    pub fn shader_value(self) -> f32 {
        match self {
            Self::EncodedSrgb => 0.,
            Self::LinearP3Preview => 1.,
            Self::LinearP3Wide => 2.,
        }
    }
}
