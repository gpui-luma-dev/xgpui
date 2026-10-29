//! IOSurface-backed BGRA and NV12 snapshots sampled by the host compositor.
use super::*;
use anyhow::Context as _;
use objc2_core_foundation::CFRetained;
use objc2_core_video::*;
use std::{
    ptr,
    sync::{Arc, Mutex},
};

/// Explicit SDR YCbCr coefficients. Range comes from the CoreVideo pixel format.
/// Chroma is sampled at normalized centered coordinates; no HDR/transfer mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nv12Matrix {
    Bt601,
    Bt709,
}
impl Nv12Matrix {
    fn weights(self) -> (f32, f32) {
        match self {
            Self::Bt601 => (0.299, 0.114),
            Self::Bt709 => (0.2126, 0.0722),
        }
    }
    fn rows(self, video_range: bool) -> [[f32; 4]; 3] {
        let (kr, kb) = self.weights();
        let kg = 1. - kr - kb;
        let (ys, cs, y0) = if video_range {
            (255. / 219., 255. / 224., 16. / 255.)
        } else {
            (1., 1., 0.)
        };
        let cb = 128. / 255.;
        let mut rows = [
            [ys, 0., 2. * (1. - kr) * cs, 0.],
            [
                ys,
                -2. * kb * (1. - kb) / kg * cs,
                -2. * kr * (1. - kr) / kg * cs,
                0.,
            ],
            [ys, 2. * (1. - kb) * cs, 0., 0.],
        ];
        for row in &mut rows {
            row[3] = -ys * y0 - (row[1] + row[2]) * cb;
        }
        rows
    }
}
#[derive(Clone, Copy, Debug)]
enum Format {
    Bgra,
    Nv12 {
        matrix: Nv12Matrix,
        video_range: bool,
    },
}
enum Output {
    Bgra(MetalImageOutput),
    Nv12(MetalNv12Output),
}
impl Output {
    fn reusable(&self, context: &MetalCustomContext) -> Result<Box<dyn std::any::Any>> {
        match self {
            Self::Bgra(output) => Ok(Box::new(output.reusable_for(context)?)),
            Self::Nv12(output) => Ok(Box::new(output.reusable_for(context)?)),
        }
    }
}
struct Cached {
    _cache: CFRetained<CVMetalTextureCache>,
    _views: Vec<CFRetained<CVMetalTexture>>,
    output: Output,
}
struct Inner {
    buffer: CFRetained<CVPixelBuffer>,
    size: Size<DevicePixels>,
    format: Format,
    outputs: Mutex<Vec<(u64, Cached)>>,
}
// SAFETY: construction requires immutable, synchronized pixels and attachments.
// Native retains are thread-safe; import mutation is serialized. The renderer
// retains the producer through sampling completion, including all native views.
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

/// Immutable native CoreVideo snapshot, imported without CPU readback/upload.
#[derive(Clone)]
pub struct CoreVideoFrame(Arc<Inner>);
impl std::fmt::Debug for CoreVideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreVideoFrame")
            .field("size", &self.0.size)
            .field("format", &self.0.format)
            .finish_non_exhaustive()
    }
}
impl CoreVideoFrame {
    /// Retain an IOSurface/Metal-compatible straight-alpha or opaque BGRA buffer.
    ///
    /// # Safety
    /// Complete producer writes/synchronization first. Do not mutate/recycle
    /// pixels or attachments while this frame or any clone exists. The host
    /// retains clones through GPU sampling completion. Premultiplied alpha is unsupported.
    pub unsafe fn from_bgra(buffer: CFRetained<CVPixelBuffer>) -> Result<Self> {
        ensure!(
            CVPixelBufferGetPixelFormatType(&buffer) == kCVPixelFormatType_32BGRA,
            "video snapshot requires 32BGRA"
        );
        Self::new(buffer, Format::Bgra)
    }
    /// Retain an opaque 8-bit bi-planar NV12 SDR snapshot (420f or 420v).
    /// The pixel format selects full/video range; matrix selection is explicit.
    ///
    /// # Safety
    /// Complete producer writes/synchronization first. Buffer and attachments
    /// must stay immutable through all clones/GPU sampling, and be IOSurface/
    /// Metal compatible. Supply the matching SDR matrix and centered chroma;
    /// attachment-based inference, other chroma siting and HDR are unsupported.
    pub unsafe fn from_nv12(buffer: CFRetained<CVPixelBuffer>, matrix: Nv12Matrix) -> Result<Self> {
        let format = CVPixelBufferGetPixelFormatType(&buffer);
        ensure!(
            format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
                || format == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            "NV12 snapshot requires 420f or 420v"
        );
        let width = CVPixelBufferGetWidth(&buffer);
        let height = CVPixelBufferGetHeight(&buffer);
        ensure!(
            width.is_multiple_of(2)
                && height.is_multiple_of(2)
                && CVPixelBufferGetPlaneCount(&buffer) == 2,
            "NV12 requires even dimensions and two planes"
        );
        ensure!(
            CVPixelBufferGetWidthOfPlane(&buffer, 0) == width
                && CVPixelBufferGetHeightOfPlane(&buffer, 0) == height
                && CVPixelBufferGetWidthOfPlane(&buffer, 1) == width / 2
                && CVPixelBufferGetHeightOfPlane(&buffer, 1) == height / 2,
            "NV12 plane extents do not match"
        );
        Self::new(
            buffer,
            Format::Nv12 {
                matrix,
                video_range: format == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            },
        )
    }
    fn new(buffer: CFRetained<CVPixelBuffer>, format: Format) -> Result<Self> {
        let width = CVPixelBufferGetWidth(&buffer);
        let height = CVPixelBufferGetHeight(&buffer);
        ensure!(
            width > 0 && height > 0 && width <= 16384 && height <= 16384,
            "video dimensions must be 1..16384"
        );
        Ok(Self(Arc::new(Inner {
            buffer,
            format,
            size: gpui::size(DevicePixels(width as i32), DevicePixels(height as i32)),
            outputs: Mutex::new(vec![]),
        })))
    }
    #[cfg(test)]
    pub(crate) fn buffer_for_test(&self) -> CFRetained<CVPixelBuffer> {
        self.0.buffer.clone()
    }
    pub fn size(&self) -> Size<DevicePixels> {
        self.0.size
    }
    pub fn import_count(&self) -> usize {
        self.0.outputs.lock().unwrap().len()
    }
}
impl gpui::CustomContent for CoreVideoFrame {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        let context = backend
            .downcast_ref::<MetalCustomContext>()
            .context("CoreVideo video frames require native Metal")?;
        let mut outputs = self.0.outputs.lock().unwrap();
        if let Some((_, cached)) = outputs.iter().find(|(key, _)| *key == context.cache_key()) {
            return cached.output.reusable(context);
        }
        ensure!(
            outputs.len() < 4,
            "video snapshot exceeds four queue imports"
        );
        let mut cache = ptr::null_mut();
        let status = unsafe {
            CVMetalTextureCache::create(
                None,
                None,
                context.objc2_device(),
                None,
                ptr::NonNull::from(&mut cache),
            )
        };
        ensure!(
            status == kCVReturnSuccess,
            "CoreVideo texture cache creation failed: {status}"
        );
        let cache = unsafe {
            CFRetained::from_raw(ptr::NonNull::new(cache).context("null CoreVideo cache")?)
        };
        let mut views = vec![];
        let mut plane = |index, format, width, height| -> Result<Texture> {
            let mut view = ptr::null_mut();
            let status = unsafe {
                CVMetalTextureCache::create_texture_from_image(
                    None,
                    &cache,
                    &self.0.buffer,
                    None,
                    format,
                    width,
                    height,
                    index,
                    ptr::NonNull::from(&mut view),
                )
            };
            ensure!(
                status == kCVReturnSuccess,
                "CoreVideo plane import failed (IOSurface/Metal compatibility required): {status}"
            );
            let view = unsafe {
                CFRetained::from_raw(ptr::NonNull::new(view).context("null CoreVideo texture")?)
            };
            let texture =
                CVMetalTextureGetTexture(&view).context("CoreVideo returned no Metal texture")?;
            let texture = crate::metal_compat::into_legacy_texture(texture);
            views.push(view);
            Ok(texture)
        };
        let width = self.0.size.width.0 as usize;
        let height = self.0.size.height.0 as usize;
        let output = match self.0.format {
            Format::Bgra => Output::Bgra(MetalImageOutput {
                output: MetalCustomOutput {
                    texture: plane(0, objc2_metal::MTLPixelFormat::BGRA8Unorm, width, height)?,
                    commands: vec![],
                },
                queue: context.queue.clone(),
            }),
            Format::Nv12 {
                matrix,
                video_range,
            } => Output::Nv12(MetalNv12Output {
                y: plane(0, objc2_metal::MTLPixelFormat::R8Unorm, width, height)?,
                uv: plane(
                    1,
                    objc2_metal::MTLPixelFormat::RG8Unorm,
                    width / 2,
                    height / 2,
                )?,
                rows: matrix.rows(video_range),
                queue: context.queue.clone(),
            }),
        };
        let returned = output.reusable(context)?;
        outputs.push((
            context.cache_key(),
            Cached {
                _cache: cache,
                _views: views,
                output,
            },
        ));
        Ok(returned)
    }
}

/// Generated offline BGRA/IOSurface test source, not a decoder. Each call owns a
/// fresh buffer, so earlier frames cannot be overwritten while the GPU samples.
#[cfg(any(test, feature = "test-support"))]
pub fn test_frame(width: usize, height: usize, index: u64) -> Result<CoreVideoFrame> {
    use objc2_core_foundation::{CFDictionary, CFType};
    use objc2_core_video::*;
    ensure!(
        width > 0 && height > 0 && width <= 2048 && height <= 2048,
        "test frame dimensions must be 1..2048"
    );
    let attributes = CFDictionary::from_slices(
        &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
        &[&*CFDictionary::<CFType, CFType>::empty()],
    );
    let mut buffer = ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            width,
            height,
            kCVPixelFormatType_32BGRA,
            Some(attributes.as_opaque()),
            ptr::NonNull::from(&mut buffer),
        )
    };
    ensure!(
        status == kCVReturnSuccess,
        "test pixel buffer creation failed: {status}"
    );
    let buffer =
        unsafe { CFRetained::from_raw(ptr::NonNull::new(buffer).context("null test buffer")?) };
    unsafe {
        let status = CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty());
        ensure!(
            status == kCVReturnSuccess,
            "test buffer lock failed: {status}"
        );
        let stride = CVPixelBufferGetBytesPerRow(&buffer);
        let pixels = std::slice::from_raw_parts_mut(
            CVPixelBufferGetBaseAddress(&buffer).cast::<u8>(),
            stride * height,
        );
        for y in 0..height {
            for x in 0..width {
                let colors = [
                    [32, 64, 224, 255],
                    [64, 208, 64, 255],
                    [224, 64, 32, 255],
                    [96, 192, 224, 255],
                ];
                let mut bgra = colors[(x * 4 / width).min(3)];
                let moving = (index as usize * 3) % width;
                if x.abs_diff(moving) < width / 16 && y > height / 3 && y < height * 2 / 3 {
                    bgra = [240, 240, 240, 255];
                }
                pixels[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&bgra);
            }
        }
        let status = CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty());
        ensure!(
            status == kCVReturnSuccess,
            "test buffer unlock failed: {status}"
        );
        CoreVideoFrame::from_bgra(buffer)
    }
}

/// Offline NV12 color bars; each frame owns a fresh immutable IOSurface.
#[cfg(any(test, feature = "test-support"))]
pub fn test_nv12_frame(
    width: usize,
    height: usize,
    index: u64,
    matrix: Nv12Matrix,
    video_range: bool,
) -> Result<CoreVideoFrame> {
    use objc2_core_foundation::{CFDictionary, CFType};
    ensure!(
        width > 0
            && height > 0
            && width <= 2048
            && height <= 2048
            && width.is_multiple_of(2)
            && height.is_multiple_of(2),
        "NV12 test dimensions must be even and 2..2048"
    );
    let attributes = CFDictionary::from_slices(
        &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
        &[&*CFDictionary::<CFType, CFType>::empty()],
    );
    let mut buffer = ptr::null_mut();
    let format = if video_range {
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
    } else {
        kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
    };
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            width,
            height,
            format,
            Some(attributes.as_opaque()),
            ptr::NonNull::from(&mut buffer),
        )
    };
    ensure!(
        status == kCVReturnSuccess,
        "NV12 test allocation failed: {status}"
    );
    let buffer = unsafe {
        CFRetained::from_raw(ptr::NonNull::new(buffer).context("null NV12 test buffer")?)
    };
    let (kr, kb) = matrix.weights();
    let codes = |x: usize, y: usize| -> [u8; 3] {
        let mut rgb = [
            [224., 64., 32.],
            [64., 208., 64.],
            [32., 64., 224.],
            [224., 192., 96.],
        ][(x * 4 / width).min(3)];
        let moving = (index as usize * 3) % width;
        if x.abs_diff(moving) < width / 16 && y > height / 3 && y < height * 2 / 3 {
            rgb = [240.; 3];
        }
        let luma = kr * rgb[0] + (1. - kr - kb) * rgb[1] + kb * rgb[2];
        let cb = (rgb[2] - luma) / (2. * (1. - kb));
        let cr = (rgb[0] - luma) / (2. * (1. - kr));
        let (ys, cs, y0) = if video_range {
            (219. / 255., 224. / 255., 16.)
        } else {
            (1., 1., 0.)
        };
        [
            (luma * ys + y0).round().clamp(0., 255.) as u8,
            (cb * cs + 128.).round().clamp(0., 255.) as u8,
            (cr * cs + 128.).round().clamp(0., 255.) as u8,
        ]
    };
    unsafe {
        let status = CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty());
        ensure!(
            status == kCVReturnSuccess,
            "NV12 test lock failed: {status}"
        );
        let y_stride = CVPixelBufferGetBytesPerRowOfPlane(&buffer, 0);
        let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(&buffer, 1);
        let luma = std::slice::from_raw_parts_mut(
            CVPixelBufferGetBaseAddressOfPlane(&buffer, 0).cast::<u8>(),
            y_stride * height,
        );
        let chroma = std::slice::from_raw_parts_mut(
            CVPixelBufferGetBaseAddressOfPlane(&buffer, 1).cast::<u8>(),
            uv_stride * height / 2,
        );
        for y in 0..height {
            for x in 0..width {
                luma[y * y_stride + x] = codes(x, y)[0];
            }
        }
        for y in 0..height / 2 {
            for x in 0..width / 2 {
                let c = codes(x * 2, y * 2);
                chroma[y * uv_stride + x * 2..y * uv_stride + x * 2 + 2].copy_from_slice(&c[1..]);
            }
        }
        let status = CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty());
        ensure!(
            status == kCVReturnSuccess,
            "NV12 test unlock failed: {status}"
        );
        CoreVideoFrame::from_nv12(buffer, matrix)
    }
}

#[cfg(test)]
mod nv12_tests {
    use super::*;
    #[test]
    fn fixed_sdr_color_vectors() {
        // Fixed quantized midtone vectors exercise matrix/range independently
        // of the generated fixture's RGB -> YCbCr helper.
        for (matrix, video, codes) in [
            (Nv12Matrix::Bt601, false, [108., 85., 211.]),
            (Nv12Matrix::Bt601, true, [109., 90., 201.]),
            (Nv12Matrix::Bt709, false, [96., 94., 209.]),
            (Nv12Matrix::Bt709, true, [98., 98., 200.]),
        ] {
            for (row, expected) in matrix.rows(video).into_iter().zip([224., 64., 32.]) {
                let actual =
                    row[0] * codes[0] + row[1] * codes[1] + row[2] * codes[2] + row[3] * 255.;
                assert!(
                    (actual - expected).abs() < 2.,
                    "{matrix:?} video={video}: {actual}"
                );
            }
        }
    }
    #[test]
    fn neutral_range_endpoints_and_format_validation() -> Result<()> {
        for matrix in [Nv12Matrix::Bt601, Nv12Matrix::Bt709] {
            for video in [false, true] {
                let rows = matrix.rows(video);
                for (code, expected) in [
                    (if video { 16. } else { 0. }, 0.),
                    (if video { 235. } else { 255. }, 1.),
                ] {
                    for row in rows {
                        let actual =
                            row[0] * code / 255. + (row[1] + row[2]) * 128. / 255. + row[3];
                        assert!((actual - expected).abs() < 0.00001);
                    }
                }
                let frame = test_nv12_frame(32, 18, 0, matrix, video)?;
                assert_eq!(frame.size(), gpui::size(DevicePixels(32), DevicePixels(18)));
                assert!(unsafe { CoreVideoFrame::from_bgra(frame.buffer_for_test()) }.is_err());
            }
        }
        let bgra = test_frame(32, 18, 0)?;
        assert!(
            unsafe { CoreVideoFrame::from_nv12(bgra.buffer_for_test(), Nv12Matrix::Bt601) }
                .is_err()
        );
        assert!(test_nv12_frame(31, 18, 0, Nv12Matrix::Bt601, false).is_err());
        Ok(())
    }
}
