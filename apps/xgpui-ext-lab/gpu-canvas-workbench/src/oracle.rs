//! Independent CPU reference for the engine's working image. It mirrors the
//! WGSL in `engine.rs` line by line, so a replayed edit script must produce
//! the same linear P3 values on the GPU and here.

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 400;

/// One brush dab: a hard-edged disc of linear P3 colour composited with
/// `opacity` over the working image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dab {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub opacity: f32,
    pub color: [f32; 3],
}

/// Saturated P3 patches (row 140..220): red, green, blue, yellow.
pub const PATCHES: [[f32; 3]; 4] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [1., 1., 0.]];
/// Fractional-alpha discs (row 240..380) composited over the checkerboard.
pub const DISCS: [([f32; 3], f32); 4] = [
    ([1., 0., 0.], 0.25),
    ([0., 0.6, 1.], 0.5),
    ([1., 1., 1.], 0.75),
    ([0., 0., 0.], 0.9),
];

fn checker(x: u32, y: u32) -> f32 {
    if (x / 16 + y / 16).is_multiple_of(2) {
        0.18
    } else {
        0.30
    }
}

/// The generated image at pixel (x, y): linear P3, opaque.
pub fn base(x: u32, y: u32) -> [f32; 3] {
    let c = checker(x, y);
    if y < 80 {
        let v = x as f32 / (WIDTH - 1) as f32;
        return [v, v, v];
    }
    if y < 120 {
        let v = 0.02 * (x as f32 / (WIDTH - 1) as f32);
        return [v, v, v];
    }
    if (140..220).contains(&y) && x >= 20 {
        let i = (x - 20) / 140;
        if i < 4 && (x - 20) % 140 < 120 {
            return PATCHES[i as usize];
        }
    }
    if (240..380).contains(&y) {
        for (i, (color, alpha)) in DISCS.iter().enumerate() {
            let dx = x as f32 + 0.5 - (90. + 150. * i as f32);
            let dy = y as f32 + 0.5 - 310.;
            if dx * dx + dy * dy <= 3600. {
                return color.map(|v| v * alpha + c * (1. - alpha));
            }
        }
    }
    [c, c, c]
}

/// Whether pixel (x, y) lies inside `dab` (pixel centres, hard edge).
pub fn covers(dab: &Dab, x: u32, y: u32) -> bool {
    let dx = x as f32 + 0.5 - dab.x;
    let dy = y as f32 + 0.5 - dab.y;
    dx * dx + dy * dy <= dab.radius * dab.radius
}

/// Composite `dab` over `pixel` if it covers (x, y).
pub fn apply(dab: &Dab, x: u32, y: u32, pixel: &mut [f32; 3]) {
    if covers(dab, x, y) {
        for (channel, color) in pixel.iter_mut().zip(dab.color) {
            *channel = color * dab.opacity + *channel * (1. - dab.opacity);
        }
    }
}

/// The working image after a reset followed by `dabs`.
pub struct Reference {
    pub pixels: Vec<[f32; 3]>,
}
impl Reference {
    pub fn new() -> Self {
        let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                pixels.push(base(x, y));
            }
        }
        Self { pixels }
    }
    pub fn apply(&mut self, dab: &Dab) {
        let x0 = (dab.x - dab.radius).floor().max(0.) as u32;
        let y0 = (dab.y - dab.radius).floor().max(0.) as u32;
        let x1 = ((dab.x + dab.radius).ceil() as u32).min(WIDTH);
        let y1 = ((dab.y + dab.radius).ceil() as u32).min(HEIGHT);
        for y in y0..y1 {
            for x in x0..x1 {
                apply(dab, x, y, &mut self.pixels[(y * WIDTH + x) as usize]);
            }
        }
    }
    pub fn at(&self, x: u32, y: u32) -> [f32; 3] {
        self.pixels[(y * WIDTH + x) as usize]
    }
}

/// Published slot value for a working pixel: exposure, clamped to 0..=1.
pub fn published(pixel: [f32; 3], exposure: f32) -> [f32; 3] {
    let gain = exposure.exp2();
    pixel.map(|v| (v * gain).clamp(0., 1.))
}

/// IEEE half to f32 (slot readback).
pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1. } else { 1. };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let mantissa = (bits & 0x3ff) as f32;
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1. + mantissa / 1024.) * 2f32.powi(exponent - 15),
    }
}
