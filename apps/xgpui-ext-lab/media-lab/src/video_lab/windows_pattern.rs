//! Deterministic BGRA fixture: animated center, fixed color/alpha reference areas.
//! Kept independent of D3D11 so motion and reference coverage run on every host.
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
