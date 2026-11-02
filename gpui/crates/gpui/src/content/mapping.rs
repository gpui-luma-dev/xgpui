//! Checked image placement, independent of source realization and GPU backend.
use super::*;

/// How the selected source rectangle fits the full unclipped destination.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Scale both axes independently.
    #[default]
    Stretch,
    /// Preserve aspect ratio, leaving transparent letterboxing.
    Contain,
    /// Preserve aspect ratio and crop the excess source area.
    Cover,
}

/// A nonempty rectangle in base-level source pixel edge coordinates.
/// Fractional edges are supported; (0, 0) is the upper-left image edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageCrop {
    origin: [f32; 2],
    size: [f32; 2],
}
impl ImageCrop {
    /// Validate finite nonnegative origin and finite positive size.
    /// Bounds are checked against the displayed image during resolution.
    pub fn new(origin: [f32; 2], size: [f32; 2]) -> Result<Self, ContentError> {
        if origin.iter().any(|v| !v.is_finite() || *v < 0.)
            || size.iter().any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err(invalid("invalid source crop"));
        }
        Ok(Self { origin, size })
    }
    /// Source pixel edge origin.
    pub fn origin(self) -> [f32; 2] {
        self.origin
    }
    /// Source pixel dimensions.
    pub fn size(self) -> [f32; 2] {
        self.size
    }
}

/// Immutable presentation mapping; never part of an image upload cache key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageMapping {
    fit: ImageFit,
    alignment: [f32; 2],
    crop: Option<ImageCrop>,
    viewport: Option<ImageCrop>,
    sampler: Sampler,
}
impl Default for ImageMapping {
    fn default() -> Self {
        Self::new(ImageFit::Stretch)
    }
}
impl ImageMapping {
    /// Centered fit with full source and linear clamp sampling, without mip access.
    pub fn new(fit: ImageFit) -> Self {
        Self {
            fit,
            alignment: [0.5; 2],
            crop: None,
            viewport: None,
            sampler: Sampler::linear_clamp(),
        }
    }
    /// Align free destination space (contain) or excess source space (cover).
    /// Zero selects left/top; one selects right/bottom.
    pub fn alignment(mut self, alignment: [f32; 2]) -> Result<Self, ContentError> {
        if alignment
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err(invalid("image alignment must be finite and in 0..=1"));
        }
        self.alignment = alignment;
        Ok(self)
    }
    /// Select a base-level source rectangle. None restores the full source.
    pub fn crop(mut self, crop: Option<ImageCrop>) -> Self {
        self.crop = crop;
        self
    }
    /// Select the visible source rectangle within the crop without introducing
    /// filter boundaries. Unlike `crop`, this preserves surrounding texels for
    /// interpolation, for example when clipping a zoomed tile with a halo.
    /// Bounds are validated against the selected crop during resolution.
    pub fn viewport(mut self, viewport: Option<ImageCrop>) -> Self {
        self.viewport = viewport;
        self
    }
    /// Explicit filtering policy. Presentation requires clamped addressing so
    /// filtering cannot escape the selected crop; graph passes support repetition.
    pub fn sampler(mut self, sampler: Sampler) -> Result<Self, ContentError> {
        if sampler.address != [AddressMode::Clamp; 2] {
            return Err(invalid("image presentation requires clamp addressing"));
        }
        self.sampler = sampler;
        Ok(self)
    }
    /// Resolve against the actual displayed descriptor and full destination size.
    /// Use a retained image's descriptor while its replacement is pending.
    pub fn resolve(
        self,
        image: &ImageDescriptor,
        destination: [f32; 2],
    ) -> Result<ResolvedImageMapping, ContentError> {
        if destination.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err(invalid("image destination must be finite and positive"));
        }
        let extent = image.extent();
        let source = [extent.width() as f64, extent.height() as f64];
        let (crop_origin, crop_size) = self
            .crop
            .map(|crop| (crop.origin.map(f64::from), crop.size.map(f64::from)))
            .unwrap_or(([0.; 2], source));
        for axis in 0..2 {
            if crop_origin[axis] + crop_size[axis] > source[axis] {
                return Err(invalid("source crop exceeds displayed image bounds"));
            }
        }
        // f64 prevents intermediate overflow for valid extreme f32 destinations.
        let (mut origin, mut size) = self
            .viewport
            .map(|v| (v.origin.map(f64::from), v.size.map(f64::from)))
            .unwrap_or((crop_origin, crop_size));
        for axis in 0..2 {
            if origin[axis] < crop_origin[axis]
                || origin[axis] + size[axis] > crop_origin[axis] + crop_size[axis]
            {
                return Err(invalid("source viewport exceeds selected crop"));
            }
        }
        let dest = destination.map(f64::from);
        let mut placed_origin = [0.; 2];
        let mut placed_size = dest;
        let scale = [dest[0] / size[0], dest[1] / size[1]];
        match self.fit {
            ImageFit::Stretch => {}
            ImageFit::Contain => {
                let scale = scale[0].min(scale[1]);
                for axis in 0..2 {
                    placed_size[axis] = size[axis] * scale;
                    placed_origin[axis] =
                        (dest[axis] - placed_size[axis]) * f64::from(self.alignment[axis]);
                }
            }
            ImageFit::Cover => {
                let scale = scale[0].max(scale[1]);
                for axis in 0..2 {
                    let visible = dest[axis] / scale;
                    origin[axis] += (size[axis] - visible) * f64::from(self.alignment[axis]);
                    size[axis] = visible;
                }
            }
        }
        let uv_origin = std::array::from_fn(|i| (origin[i] / source[i]) as f32);
        let uv_size = std::array::from_fn(|i| (size[i] / source[i]) as f32);
        // Clamp filter footprints to the selected crop, including sub-texel crops.
        // Cover is a viewport into that crop, so its visible edges do not alter
        // the sampling domain or introduce an artificial seam.
        let clamp_min = std::array::from_fn(|i| {
            let half = 0.5f64.min(crop_size[i] / 2.);
            ((crop_origin[i] + half) / source[i]) as f32
        });
        let clamp_max = std::array::from_fn(|i| {
            let half = 0.5f64.min(crop_size[i] / 2.);
            ((crop_origin[i] + crop_size[i] - half) / source[i]) as f32
        });
        let result = ResolvedImageMapping {
            destination_origin: std::array::from_fn(|i| (placed_origin[i] / dest[i]) as f32),
            destination_size: std::array::from_fn(|i| (placed_size[i] / dest[i]) as f32),
            uv_origin,
            uv_size,
            clamp_min,
            clamp_max,
            sampler: self.sampler,
        };
        if result
            .destination_size
            .iter()
            .chain(result.uv_size.iter())
            .any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err(invalid("image mapping exceeds representable precision"));
        }
        Ok(result)
    }
}

/// Backend-independent normalized placement and source sampling parameters.
/// Compositors retain full destination geometry for coverage and opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedImageMapping {
    /// Upper-left placement in normalized destination coordinates.
    pub destination_origin: [f32; 2],
    /// Placement dimensions in normalized destination coordinates.
    pub destination_size: [f32; 2],
    /// Visible source UV edge origin.
    pub uv_origin: [f32; 2],
    /// Visible source UV dimensions.
    pub uv_size: [f32; 2],
    /// Minimum UV for crop-bounded filtering at base level.
    pub clamp_min: [f32; 2],
    /// Maximum UV for crop-bounded filtering at base level.
    pub clamp_max: [f32; 2],
    /// Explicit source sampling policy; no mip access when mip_filter is None.
    pub sampler: Sampler,
}
impl Default for ResolvedImageMapping {
    fn default() -> Self {
        Self {
            destination_origin: [0.; 2],
            destination_size: [1.; 2],
            uv_origin: [0.; 2],
            uv_size: [1.; 2],
            clamp_min: [0.; 2],
            clamp_max: [1.; 2],
            sampler: Sampler::linear_clamp(),
        }
    }
}
impl ResolvedImageMapping {
    /// Shared float4 packing for placement, UV transform, crop clamp and mip policy.
    pub fn shader_uniforms(self) -> [[f32; 4]; 4] {
        [
            [
                self.destination_origin[0],
                self.destination_origin[1],
                self.destination_size[0],
                self.destination_size[1],
            ],
            [
                self.uv_origin[0],
                self.uv_origin[1],
                self.uv_size[0],
                self.uv_size[1],
            ],
            [
                self.clamp_min[0],
                self.clamp_min[1],
                self.clamp_max[0],
                self.clamp_max[1],
            ],
            [
                match self.sampler.mip_filter {
                    None => 0.,
                    Some(FilterMode::Nearest) => 1.,
                    Some(FilterMode::Linear) => 2.,
                },
                0.,
                0.,
                0.,
            ],
        ]
    }

    /// Map a normalized destination point to UV. Letterboxing and points outside
    /// the full destination return None; compositors must output transparency.
    /// Coordinates describe pixel centers; upper placement edges are exclusive.
    pub fn sample_uv(self, point: [f32; 2]) -> Option<[f32; 2]> {
        let mut uv = [0.; 2];
        for i in 0..2 {
            if !point[i].is_finite() || !(0. ..1.).contains(&point[i]) {
                return None;
            }
            let local = (point[i] - self.destination_origin[i]) / self.destination_size[i];
            if !(0. ..1.).contains(&local) {
                return None;
            }
            uv[i] = (self.uv_origin[i] + local * self.uv_size[i])
                .clamp(self.clamp_min[i], self.clamp_max[i]);
        }
        Some(uv)
    }
}
fn invalid(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::InvalidDescriptor, message)
}
