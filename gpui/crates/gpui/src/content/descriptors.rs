use super::{ContentError, ContentErrorKind};
use crate::{DevicePixels, Pixels, Size, size};

type Result<T> = std::result::Result<T, ContentError>;
fn invalid(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::InvalidDescriptor, message)
}

/// Exact nonempty physical image dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageExtent {
    width: u32,
    height: u32,
}
impl ImageExtent {
    /// Validate dimensions without assuming a backend's maximum texture size.
    pub fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(invalid(
                "image dimensions must be positive device-pixel sizes",
            ));
        }
        Ok(Self { width, height })
    }
    /// Physical width.
    pub fn width(self) -> u32 {
        self.width
    }
    /// Physical height.
    pub fn height(self) -> u32 {
        self.height
    }
    /// Convert to GPUI physical dimensions.
    pub fn device_size(self) -> Size<DevicePixels> {
        size(
            DevicePixels(self.width as i32),
            DevicePixels(self.height as i32),
        )
    }
}
impl TryFrom<Size<DevicePixels>> for ImageExtent {
    type Error = ContentError;
    fn try_from(value: Size<DevicePixels>) -> Result<Self> {
        let width = u32::try_from(value.width.0).map_err(|_| invalid("negative image width"))?;
        let height = u32::try_from(value.height.0).map_err(|_| invalid("negative image height"))?;
        Self::new(width, height)
    }
}

/// Portable storage layout, independent of pixel interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// Four normalized 8-bit RGBA channels.
    Rgba8,
    /// Four normalized 8-bit BGRA channels.
    Bgra8,
    /// Four little-endian binary16 RGBA channels.
    Rgba16Float,
    /// A normalized 8-bit coverage channel.
    R8,
}
impl PixelFormat {
    /// Unpadded bytes per pixel.
    pub fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::Rgba8 | Self::Bgra8 => 4,
            Self::Rgba16Float => 8,
            Self::R8 => 1,
        }
    }
}

/// Initial standard SDR color interpretations; all use a D65 white point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorEncoding {
    /// Encoded sRGB.
    Srgb,
    /// Linear sRGB primaries.
    LinearSrgb,
    /// Linear Display P3 primaries.
    LinearP3,
    /// Linear Rec.2020 primaries, without implied HDR presentation.
    LinearRec2020,
    /// Coverage rather than color.
    Coverage,
}
/// How alpha relates to stored RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlphaMode {
    /// Alpha is one.
    Opaque,
    /// RGB is unassociated.
    Straight,
    /// RGB is multiplied by alpha in the declared encoding.
    Premultiplied,
}
/// Pixel meaning, separate from channel storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageInterpretation {
    /// Primaries and transfer function.
    pub color: ColorEncoding,
    /// Alpha association.
    pub alpha: AlphaMode,
}

/// Immutable finite bounded-SDR color, with canonical zero for structural reuse.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColorValue {
    bits: [u32; 4],
    interpretation: ImageInterpretation,
}
impl ColorValue {
    /// Straight-alpha linear Display P3/D65 color in 0..=1.
    pub fn linear_p3(rgba: [f32; 4]) -> Result<Self> {
        Self::new(
            rgba,
            ImageInterpretation {
                color: ColorEncoding::LinearP3,
                alpha: AlphaMode::Straight,
            },
        )
    }
    /// Straight-alpha encoded-sRGB color in 0..=1.
    pub fn srgb(rgba: [f32; 4]) -> Result<Self> {
        Self::new(
            rgba,
            ImageInterpretation {
                color: ColorEncoding::Srgb,
                alpha: AlphaMode::Straight,
            },
        )
    }
    /// Validate channels and association. Coverage uses image masks, not colors.
    pub fn new(rgba: [f32; 4], interpretation: ImageInterpretation) -> Result<Self> {
        if interpretation.color == ColorEncoding::Coverage
            || rgba
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || (interpretation.alpha == AlphaMode::Opaque && rgba[3] != 1.0)
            || (interpretation.alpha == AlphaMode::Premultiplied
                && rgba[..3].iter().any(|v| *v > rgba[3]))
        {
            return Err(invalid("invalid bounded-SDR color or alpha association"));
        }
        Ok(Self {
            bits: rgba.map(|v| if v == 0.0 { 0 } else { v.to_bits() }),
            interpretation,
        })
    }
    /// Declared channel values.
    pub fn rgba(&self) -> [f32; 4] {
        self.bits.map(f32::from_bits)
    }
    /// Declared color/alpha domain.
    pub fn interpretation(&self) -> ImageInterpretation {
        self.interpretation
    }
}

/// Checked image allocation and sampling description.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageDescriptor {
    extent: ImageExtent,
    format: PixelFormat,
    interpretation: ImageInterpretation,
    mip_levels: u32,
    byte_len: u64,
}
impl ImageDescriptor {
    /// Check storage/interpretation compatibility and exact mip storage size.
    pub fn new(
        extent: ImageExtent,
        format: PixelFormat,
        interpretation: ImageInterpretation,
        mip_levels: u32,
    ) -> Result<Self> {
        if (format == PixelFormat::R8) != (interpretation.color == ColorEncoding::Coverage)
            || (format == PixelFormat::R8 && interpretation.alpha != AlphaMode::Opaque)
        {
            return Err(invalid(
                "R8 requires opaque coverage interpretation; color requires RGBA/BGRA",
            ));
        }
        let max_mips = u32::BITS - extent.width.max(extent.height).leading_zeros();
        if mip_levels == 0 || mip_levels > max_mips {
            return Err(invalid("invalid mip level count"));
        }
        let mut byte_len = 0u64;
        for level in 0..mip_levels {
            let bytes = u64::from((extent.width >> level).max(1))
                .checked_mul(u64::from((extent.height >> level).max(1)))
                .and_then(|v| v.checked_mul(format.bytes_per_pixel()))
                .ok_or_else(|| invalid("image byte count overflow"))?;
            byte_len = byte_len
                .checked_add(bytes)
                .ok_or_else(|| invalid("mip byte count overflow"))?;
        }
        Ok(Self {
            extent,
            format,
            interpretation,
            mip_levels,
            byte_len,
        })
    }
    /// Single-level linear P3 half-float surface.
    pub fn linear_p3_f16(extent: ImageExtent, alpha: AlphaMode) -> Result<Self> {
        Self::new(
            extent,
            PixelFormat::Rgba16Float,
            ImageInterpretation {
                color: ColorEncoding::LinearP3,
                alpha,
            },
            1,
        )
    }
    /// Exact source dimensions, which may differ from destination dimensions.
    pub fn extent(&self) -> ImageExtent {
        self.extent
    }
    /// Storage format.
    pub fn format(&self) -> PixelFormat {
        self.format
    }
    /// Pixel interpretation.
    pub fn interpretation(&self) -> ImageInterpretation {
        self.interpretation
    }
    /// Defined mip levels.
    pub fn mip_levels(&self) -> u32 {
        self.mip_levels
    }
    /// Unpadded storage across all levels.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }
}

/// Typed buffer element layout. Semantic vertex schemas belong to program reflection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferLayout {
    stride: u32,
    alignment: u32,
}
impl BufferLayout {
    /// Validate a nonzero stride and power-of-two alignment that divides it.
    pub fn new(stride: u32, alignment: u32) -> Result<Self> {
        if stride == 0 || !alignment.is_power_of_two() || !stride.is_multiple_of(alignment) {
            return Err(invalid(
                "buffer stride must be a multiple of its power-of-two alignment",
            ));
        }
        Ok(Self { stride, alignment })
    }
    /// Bytes per element.
    pub fn stride(self) -> u32 {
        self.stride
    }
    /// Required byte alignment.
    pub fn alignment(self) -> u32 {
        self.alignment
    }
}
bitflags::bitflags! {
    /// Declared permitted buffer uses; executors may allocate additional internal usage.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct BufferUsage: u8 {
        /// Vertex input.
        const VERTEX = 1;
        /// Index input.
        const INDEX = 2;
        /// Uniform data.
        const UNIFORM = 4;
        /// Storage read/write through a declared program binding.
        const STORAGE = 8;
        /// Copy source.
        const COPY_SOURCE = 16;
        /// Copy destination.
        const COPY_DESTINATION = 32;
    }
}
/// Validated immutable buffer allocation description.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BufferDescriptor {
    byte_len: u64,
    layout: BufferLayout,
    usage: BufferUsage,
}
impl BufferDescriptor {
    /// Require complete elements and nonempty usage.
    pub fn new(byte_len: u64, layout: BufferLayout, usage: BufferUsage) -> Result<Self> {
        if byte_len == 0
            || !byte_len.is_multiple_of(u64::from(layout.stride))
            || usage.is_empty()
            || !BufferUsage::all().contains(usage)
        {
            return Err(invalid(
                "buffer requires complete elements and valid nonempty usage",
            ));
        }
        Ok(Self {
            byte_len,
            layout,
            usage,
        })
    }
    /// Allocation size.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }
    /// Element layout.
    pub fn layout(&self) -> BufferLayout {
        self.layout
    }
    /// Declared uses.
    pub fn usage(&self) -> BufferUsage {
        self.usage
    }
}

/// Immutable source-geometry request, independent of clipping and opacity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContentRequest {
    logical: [u32; 2],
    extent: ImageExtent,
    scale: u32,
}
impl ContentRequest {
    /// Keep exact raster extent rather than deriving it from logical dimensions.
    pub fn new(logical_size: Size<Pixels>, extent: ImageExtent, scale_factor: f32) -> Result<Self> {
        let logical = [
            f32::from(logical_size.width),
            f32::from(logical_size.height),
        ];
        if logical.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || !scale_factor.is_finite()
            || scale_factor <= 0.0
        {
            return Err(invalid(
                "logical dimensions and scale must be finite and positive",
            ));
        }
        Ok(Self {
            logical: logical.map(f32::to_bits),
            extent,
            scale: scale_factor.to_bits(),
        })
    }
    /// Full unclipped logical size.
    pub fn logical_size(&self) -> Size<Pixels> {
        size(
            crate::px(f32::from_bits(self.logical[0])),
            crate::px(f32::from_bits(self.logical[1])),
        )
    }
    /// Requested destination raster extent, not a required source-image size.
    pub fn extent(&self) -> ImageExtent {
        self.extent
    }
    /// Original window scale.
    pub fn scale_factor(&self) -> f32 {
        f32::from_bits(self.scale)
    }
}
