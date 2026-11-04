//! Fresh immutable IOSurfaces populated from the shared video fixture.
use super::{
    SourceFormat,
    pattern::{self, HEIGHT, Matrix, WIDTH},
};
use anyhow::{Context, Result, ensure};
use gpui_apple::custom_draw::core_video::{CoreVideoFrame, Nv12Matrix};
use objc2_core_foundation::{CFDictionary, CFRetained, CFType};
use objc2_core_video::*;
use std::ptr::{self, NonNull};

struct Locked<'a> {
    buffer: &'a CVPixelBuffer,
    locked: bool,
}
impl<'a> Locked<'a> {
    fn new(buffer: &'a CVPixelBuffer) -> Result<Self> {
        // SAFETY: the retained buffer remains alive for the lock's lifetime.
        let status =
            unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::empty()) };
        ensure!(
            status == kCVReturnSuccess,
            "video pixel buffer lock failed: {status}"
        );
        Ok(Self {
            buffer,
            locked: true,
        })
    }
    fn finish(mut self) -> Result<()> {
        // SAFETY: this guard owns the matching successful lock.
        let status =
            unsafe { CVPixelBufferUnlockBaseAddress(self.buffer, CVPixelBufferLockFlags::empty()) };
        self.locked = false;
        ensure!(
            status == kCVReturnSuccess,
            "video pixel buffer unlock failed: {status}"
        );
        Ok(())
    }
}
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        if self.locked {
            // SAFETY: error-path cleanup of this guard's successful lock.
            let status = unsafe {
                CVPixelBufferUnlockBaseAddress(self.buffer, CVPixelBufferLockFlags::empty())
            };
            if status != kCVReturnSuccess {
                log::error!("video buffer cleanup unlock failed: {status}");
            }
        }
    }
}

fn format_info(format: SourceFormat) -> (u32, Option<(Matrix, bool)>) {
    match format {
        SourceFormat::Bgra => (kCVPixelFormatType_32BGRA, None),
        SourceFormat::Full601 => (
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            Some((Matrix::Bt601, false)),
        ),
        SourceFormat::Video601 => (
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            Some((Matrix::Bt601, true)),
        ),
        SourceFormat::Full709 => (
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            Some((Matrix::Bt709, false)),
        ),
        SourceFormat::Video709 => (
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            Some((Matrix::Bt709, true)),
        ),
    }
}

fn buffer(format: SourceFormat, index: u64) -> Result<CFRetained<CVPixelBuffer>> {
    let (pixel_format, conversion) = format_info(format);
    let attributes = CFDictionary::from_slices(
        // SAFETY: CoreVideo exports an immutable dictionary key.
        &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
        &[&*CFDictionary::<CFType, CFType>::empty()],
    );
    let mut raw = ptr::null_mut();
    // SAFETY: dimensions are fixed positive/even fixture sizes, the attribute
    // dictionary is alive, and raw is a writable output pointer.
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            WIDTH as usize,
            HEIGHT as usize,
            pixel_format,
            Some(attributes.as_opaque()),
            NonNull::from(&mut raw),
        )
    };
    ensure!(
        status == kCVReturnSuccess,
        "video IOSurface allocation failed: {status}"
    );
    // SAFETY: successful CVPixelBufferCreate returns one owned retain.
    let buffer =
        unsafe { CFRetained::from_raw(NonNull::new(raw).context("null video pixel buffer")?) };
    let lock = Locked::new(&buffer)?;
    if let Some((matrix, video_range)) = conversion {
        let pixels = pattern::nv12(index, matrix, video_range);
        for (plane, source, rows) in [
            (0, &pixels.y, HEIGHT as usize),
            (1, &pixels.uv, HEIGHT as usize / 2),
        ] {
            // SAFETY: the freshly allocated NV12 buffer is locked and has the
            // requested two planes. CoreVideo supplies each plane's row stride.
            let (base, stride) = (
                CVPixelBufferGetBaseAddressOfPlane(&buffer, plane),
                CVPixelBufferGetBytesPerRowOfPlane(&buffer, plane),
            );
            ensure!(
                !base.is_null() && stride >= WIDTH as usize,
                "invalid NV12 plane layout"
            );
            // SAFETY: the plane owns stride * rows writable bytes while locked.
            let target =
                unsafe { std::slice::from_raw_parts_mut(base.cast::<u8>(), stride * rows) };
            copy_rows(target, stride, source, WIDTH as usize, rows);
        }
    } else {
        let pixels = pattern::pixels(index);
        // SAFETY: this freshly allocated non-planar BGRA buffer is locked.
        let (base, stride) = (
            CVPixelBufferGetBaseAddress(&buffer),
            CVPixelBufferGetBytesPerRow(&buffer),
        );
        ensure!(
            !base.is_null() && stride >= WIDTH as usize * 4,
            "invalid BGRA row layout"
        );
        // SAFETY: the allocation owns stride * HEIGHT bytes while locked.
        let target =
            unsafe { std::slice::from_raw_parts_mut(base.cast::<u8>(), stride * HEIGHT as usize) };
        copy_rows(target, stride, &pixels, WIDTH as usize * 4, HEIGHT as usize);
    }
    lock.finish()?;
    Ok(buffer)
}
fn copy_rows(target: &mut [u8], stride: usize, source: &[u8], row_bytes: usize, rows: usize) {
    for row in 0..rows {
        target[row * stride..row * stride + row_bytes]
            .copy_from_slice(&source[row * row_bytes..(row + 1) * row_bytes]);
    }
}

pub(super) fn frame(format: SourceFormat, index: u64) -> Result<CoreVideoFrame> {
    let buffer = buffer(format, index)?;
    // SAFETY: all writes are finished/unlocked. Every frame owns a fresh buffer
    // that is never mutated or pooled; native output retains its full lifetime.
    unsafe {
        match format_info(format).1 {
            None => CoreVideoFrame::from_bgra(buffer),
            Some((matrix, _)) => CoreVideoFrame::from_nv12(
                buffer,
                match matrix {
                    Matrix::Bt601 => Nv12Matrix::Bt601,
                    Matrix::Bt709 => Nv12Matrix::Bt709,
                },
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn bgra_iosurface_matches_windows_source_bytes_without_rewriting_prior_frames() -> Result<()> {
        let first = buffer(SourceFormat::Bgra, 0)?;
        let second = buffer(SourceFormat::Bgra, 8)?;
        for (buffer, index) in [(&first, 0), (&second, 8), (&first, 0)] {
            let lock = Locked::new(buffer)?;
            let expected = pattern::pixels(index);
            // SAFETY: our retained BGRA buffer is locked and still owns its rows.
            let (base, stride) = (
                CVPixelBufferGetBaseAddress(buffer),
                CVPixelBufferGetBytesPerRow(buffer),
            );
            ensure!(
                !base.is_null() && stride >= WIDTH as usize * 4,
                "invalid BGRA layout"
            );
            // SAFETY: CoreVideo provides stride * HEIGHT readable bytes while locked.
            let actual =
                unsafe { std::slice::from_raw_parts(base.cast::<u8>(), stride * HEIGHT as usize) };
            for row in 0..HEIGHT as usize {
                let from = row * WIDTH as usize * 4;
                ensure!(
                    &actual[row * stride..row * stride + WIDTH as usize * 4]
                        == &expected[from..from + WIDTH as usize * 4],
                    "IOSurface row {row} differs at frame {index}"
                );
            }
            lock.finish()?;
        }
        Ok(())
    }
}
