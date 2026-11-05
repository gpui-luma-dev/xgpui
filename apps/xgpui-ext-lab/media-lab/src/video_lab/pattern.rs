//! Deterministic BGRA fixture: animated center, fixed color/alpha reference areas.
//! Shared by CoreVideo, D3D11 and Linux DMA-BUF; independent of either backend so motion and reference coverage run on every host.
pub(super) const WIDTH: u32 = 320;
pub(super) const HEIGHT: u32 = 180;
pub(super) const COLORS: [[u8; 3]; 4] =
    [[224, 64, 32], [64, 208, 64], [32, 64, 224], [224, 192, 96]];
const MOTION_TOP: u32 = 48;
const MOTION_BOTTOM: u32 = 112;

pub(super) fn pixels(index: u64) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    // Triangular paths avoid an abrupt wrap while crossing the full source.
    let horizontal = ((index % 180) * 3) as i32;
    let center_x = 25 + horizontal.min(540 - horizontal);
    let vertical = (index % 60) as i32;
    let center_y = 70 + vertical.min(60 - vertical) / 2;
    let phase = ((index % 16) * 3) as u32;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let [mut r, mut g, mut b] = COLORS[(x / (WIDTH / 4)) as usize];
            let mut a = if y >= HEIGHT / 2 && x >= WIDTH * 3 / 4 {
                128
            } else {
                255
            };
            if (MOTION_TOP..MOTION_BOTTOM).contains(&y) {
                // Moving diagonal bands make continuous playback obvious even
                // when the puck is near its turning points or partly clipped.
                [r, g, b] = if ((x + y + phase) / 24) % 2 == 0 {
                    [24, 48, 96]
                } else {
                    [48, 176, 224]
                };
                let dx = x as i32 - center_x;
                let dy = y as i32 - center_y;
                let distance = dx * dx + dy * dy;
                if distance <= 20 * 20 {
                    [r, g, b] = if distance <= 16 * 16 {
                        [255, 240, 96]
                    } else {
                        [255; 3]
                    };
                }
                a = 255;
            }
            pixels.extend_from_slice(&[b, g, r, a]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn motion_changes_a_large_region_and_preserves_all_reference_areas() {
        let first = pixels(0);
        let next = pixels(8);
        assert_eq!(first.len(), (WIDTH * HEIGHT * 4) as usize);
        let changed = first
            .chunks_exact(4)
            .zip(next.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > (WIDTH * HEIGHT / 4) as usize,
            "motion must affect more than a quarter of the image, not a small tick: {changed}"
        );
        for index in [0, 8, 30, 60, 90, 179, u64::MAX] {
            let frame = pixels(index);
            for y in 0..HEIGHT {
                if !(MOTION_TOP..MOTION_BOTTOM).contains(&y) {
                    let start = (y * WIDTH * 4) as usize;
                    let end = start + (WIDTH * 4) as usize;
                    assert_eq!(
                        &frame[start..end],
                        &first[start..end],
                        "reference row {y} changed at frame {index}"
                    );
                }
            }
            for (x, [r, g, b]) in [20, 100, 180, 260].into_iter().zip(COLORS) {
                let at = ((20 * WIDTH + x) * 4) as usize;
                assert_eq!(&frame[at..at + 4], &[b, g, r, 255]);
            }
            let at = ((120 * WIDTH + 260) * 4) as usize;
            assert_eq!(&frame[at..at + 4], &[96, 192, 224, 128]);
        }
    }
}

/// NV12 has no alpha channel: preserve the pattern's straight RGB as opaque.
/// The BGRA variants remain the cross-platform reference for alpha behavior.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug)]
pub(super) enum Matrix {
    Bt601,
    Bt709,
}

#[cfg(any(target_os = "macos", test))]
pub(super) struct Nv12 {
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
}

#[cfg(any(target_os = "macos", test))]
fn codes(bgra: &[u8], matrix: Matrix, video_range: bool) -> [f32; 3] {
    let (kr, kb) = match matrix {
        Matrix::Bt601 => (0.299, 0.114),
        Matrix::Bt709 => (0.2126, 0.0722),
    };
    let [b, g, r] = [bgra[0], bgra[1], bgra[2]].map(f32::from);
    let y = kr * r + (1. - kr - kb) * g + kb * b;
    let cb = (b - y) / (2. * (1. - kb));
    let cr = (r - y) / (2. * (1. - kr));
    let (ys, cs, y0) = if video_range {
        (219. / 255., 224. / 255., 16.)
    } else {
        (1., 1., 0.)
    };
    [y * ys + y0, cb * cs + 128., cr * cs + 128.]
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn nv12(index: u64, matrix: Matrix, video_range: bool) -> Nv12 {
    let bgra = pixels(index);
    let quantize = |value: f32| value.round().clamp(0., 255.) as u8;
    let mut y = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for pixel in bgra.chunks_exact(4) {
        y.push(quantize(codes(pixel, matrix, video_range)[0]));
    }
    let mut uv = Vec::with_capacity((WIDTH * HEIGHT / 2) as usize);
    // Centered 4:2:0 chroma is averaged over the same 2x2 source pixels.
    for row in (0..HEIGHT).step_by(2) {
        for column in (0..WIDTH).step_by(2) {
            let mut chroma = [0.; 2];
            for dy in 0..2 {
                for dx in 0..2 {
                    let at = (((row + dy) * WIDTH + column + dx) * 4) as usize;
                    let c = codes(&bgra[at..at + 4], matrix, video_range);
                    chroma[0] += c[1] * 0.25;
                    chroma[1] += c[2] * 0.25;
                }
            }
            uv.extend(chroma.map(quantize));
        }
    }
    Nv12 { y, uv }
}

#[cfg(test)]
mod nv12_tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn matrix_and_range_match_fixed_independent_color_codes() {
        for (matrix, video_range, expected) in [
            (Matrix::Bt601, false, [108, 85, 211]),
            (Matrix::Bt601, true, [109, 90, 201]),
            (Matrix::Bt709, false, [96, 94, 209]),
            (Matrix::Bt709, true, [98, 98, 200]),
        ] {
            assert_eq!(
                codes(&[32, 64, 224, 255], matrix, video_range).map(|v| v.round() as u8),
                expected
            );
            let first = nv12(0, matrix, video_range);
            let next = nv12(8, matrix, video_range);
            assert_eq!(first.y.len(), (WIDTH * HEIGHT) as usize);
            assert_eq!(first.uv.len(), (WIDTH * HEIGHT / 2) as usize);
            assert!(
                first.y.iter().zip(&next.y).filter(|(a, b)| a != b).count()
                    > (WIDTH * HEIGHT / 4) as usize
            );
            assert_eq!(first.y[20 * WIDTH as usize + 20], expected[0]);
            let chroma = (10 * WIDTH + 20) as usize;
            assert_eq!(&first.uv[chroma..chroma + 2], &expected[1..]);
        }
    }
}
