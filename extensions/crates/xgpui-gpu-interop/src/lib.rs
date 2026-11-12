//! Experimental zero-copy GPU interop for xgpui: show frames from a GPU engine
//! you own in xgpui windows, the way an engine presents to a swapchain. See the
//! [GPU interop guide](https://xgpui.dev/gpu-interop.html).
//!
//! The engine keeps its own device, queue and thread. An [`EngineSurface`] owns
//! a fixed ring of shareable slots:
//!
//! 1. `acquire()` a writable slot, or get [`AcquireError::WouldBlock`] when
//!    every slot is still in use (bounded backpressure).
//! 2. Render into the slot on the engine's GPU and submit a signal of a sync
//!    point after that work.
//! 3. `present(slot, sync)` publishes the slot. It is never written again
//!    until every retained scene that sampled it is gone and the compositor's
//!    GPU sampling has completed; then it returns to the ring.
//!
//! The host composites the latest presented slot as ordinary custom content,
//! after a GPU-side wait on the sync point. Nothing is copied through the CPU.
//!
//! Contract: call `present` only after the engine has *submitted* the work that
//! signals the sync point. A sync point whose signal is never submitted can
//! stall composition.
//!
//! Implemented: macOS (IOSurface slots, `MTLSharedEvent` sync); with feature
//! `wgpu`, Linux (opaque-fd Vulkan memory, timeline semaphore) and Windows
//! (shared NT-handle D3D12 textures and fence, opened by the D3D11
//! compositor). The compositor waits for each frame on the GPU.
//!
//! The engine must run on the compositor's GPU; otherwise the surface reports
//! [`Unsupported`]. On a machine with two GPUs, nothing yet helps an engine pick
//! the right one (issue #253).
use gpui::{DevicePixels, Size, size};

pub(crate) mod heavy;
// Only the surface uses the ring; Linux and Windows have none without `wgpu`.
#[cfg_attr(
    not(any(
        target_os = "macos",
        all(any(target_os = "linux", target_os = "windows"), feature = "wgpu")
    )),
    allow(dead_code)
)]
mod ring;
pub use heavy::{HeavyEngine, HeavyStatus};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(all(target_os = "linux", feature = "wgpu"))]
mod linux;
#[cfg(all(target_os = "linux", feature = "wgpu"))]
use linux as platform;
#[cfg(all(target_os = "linux", feature = "wgpu"))]
pub use linux::*;

#[cfg(all(target_os = "windows", feature = "wgpu"))]
mod d3d;
#[cfg(all(target_os = "windows", feature = "wgpu"))]
use d3d as platform;
#[cfg(all(target_os = "windows", feature = "wgpu"))]
pub use d3d::*;

#[cfg(any(
    target_os = "macos",
    all(any(target_os = "linux", target_os = "windows"), feature = "wgpu")
))]
mod surface;
#[cfg(any(
    target_os = "macos",
    all(any(target_os = "linux", target_os = "windows"), feature = "wgpu")
))]
pub use surface::{EngineSurface, SurfaceSlot};

/// The wgpu version engine adapters are built against; use it for engine devices.
#[cfg(all(any(target_os = "macos", target_os = "windows"), feature = "wgpu"))]
pub use ::wgpu;
/// The wgpu version engine adapters are built against; use it for engine devices.
#[cfg(all(target_os = "linux", feature = "wgpu"))]
pub use gpui_wgpu::wgpu;

/// Pixel storage and color interpretation of every slot in a surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceFormat {
    /// BGRA8, straight alpha, sRGB-encoded channels. Works in every window.
    Bgra8Srgb,
    /// RGBA16 float, associated alpha, linear Display P3/D65. Shown in wide
    /// gamut in a window that negotiated `WindowColorMode::WideGamutSdr`
    /// (macOS and Windows), and as a mapped sRGB preview in any other window
    /// and on Linux (#146). Slots also allow storage (compute) writes.
    Rgba16FloatLinearP3,
}
impl SurfaceFormat {
    /// Bytes per pixel.
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Bgra8Srgb => 4,
            Self::Rgba16FloatLinearP3 => 8,
        }
    }
    /// The color interpretation the host applies when compositing into a
    /// window whose effective color mode is (`wide`) or is not wide-gamut SDR.
    pub fn color_mode(self, wide: bool) -> gpui::ImageColorMode {
        match self {
            Self::Bgra8Srgb => gpui::ImageColorMode::EncodedSrgb,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::Rgba16FloatLinearP3 if wide => gpui::ImageColorMode::LinearP3Wide,
            Self::Rgba16FloatLinearP3 => {
                let _ = wide;
                gpui::ImageColorMode::LinearP3Preview
            }
        }
    }
    /// Whether engines may bind slots as storage textures (compute writes).
    pub fn supports_storage(self) -> bool {
        matches!(self, Self::Rgba16FloatLinearP3)
    }
}

/// Fixed properties of a surface's slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDesc {
    pub width: u32,
    pub height: u32,
    pub format: SurfaceFormat,
    /// Ring length: 2..=8. Use at least 3 for continuous animation, plus
    /// one per additional window showing the surface: a window that is not
    /// drawing (for example minimized) keeps the frame it last showed in its
    /// retained scene, which pins that slot until the window draws again.
    pub slots: usize,
}
impl SurfaceDesc {
    /// Validate dimensions, ring length and the per-surface memory budget.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=16384).contains(&self.width) && (1..=16384).contains(&self.height),
            "surface dimensions must be 1..=16384"
        );
        anyhow::ensure!(
            (2..=8).contains(&self.slots),
            "surface ring must hold 2..=8 slots"
        );
        let bytes = self.width as u64
            * self.height as u64
            * self.format.bytes_per_pixel() as u64
            * self.slots as u64;
        anyhow::ensure!(bytes <= 512 * 1024 * 1024, "surface ring exceeds 512 MiB");
        Ok(())
    }
    /// Slot extent in physical pixels.
    pub fn extent(&self) -> Size<DevicePixels> {
        size(
            DevicePixels(self.width as i32),
            DevicePixels(self.height as i32),
        )
    }
}

/// Why `acquire` returned no slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcquireError {
    /// Every slot is being written, or still referenced by retained scenes or
    /// in-flight composition. Retry after the next presented frame.
    WouldBlock,
    /// The surface was invalidated (its engine device was lost). Create a new
    /// surface on a new engine device; the last frame keeps displaying.
    Invalidated,
}
impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WouldBlock => f.write_str("every surface slot is in use"),
            Self::Invalidated => f.write_str("the surface was invalidated"),
        }
    }
}
impl std::error::Error for AcquireError {}

/// Returned when the platform or configuration cannot support a surface.
#[derive(Debug)]
pub struct Unsupported(pub String);
impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "engine surface unsupported: {}", self.0)
    }
}
impl std::error::Error for Unsupported {}

/// Counters for diagnostics and pressure tests.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceStats {
    /// Slots available to `acquire`.
    pub free: usize,
    /// Slots acquired and not yet presented or dropped.
    pub writing: usize,
    /// Slots presented and still referenced (latest frame, retained scenes or
    /// in-flight composition).
    pub published: usize,
    /// Total successful presents.
    pub presents: u64,
    /// Total `acquire` calls that returned `WouldBlock`.
    pub would_block: u64,
    /// Presented frames not shown yet because their sync point has not been
    /// observed reached (see `EngineSurface::present`).
    pub waiting: usize,
    /// Presented frames the compositor has composited at least once.
    pub composited: u64,
    /// Presented frames dropped without ever being composited: the engine
    /// produced them faster than the compositor showed them.
    pub unshown: u64,
    /// Delay from `present` to the first composition of the latest
    /// composited frame. Rising values mean the compositor is falling
    /// behind (for example, the GPU is saturated); engines can throttle.
    pub composite_latency_ms: f32,
}

/// How engine output reaches the compositor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    /// The compositor samples the engine's memory directly.
    ZeroCopy,
}

/// Engine-side helpers shared by the platform tests.
#[cfg(all(
    test,
    feature = "wgpu",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
pub(crate) mod test_engine {
    use crate::wgpu;

    /// Encode a compute pass that writes `color` to every texel of an
    /// RGBA16F slot through a storage binding.
    pub fn compute_fill(
        device: &wgpu::Device,
        texture: &wgpu::Texture,
        color: [f32; 4],
    ) -> wgpu::CommandBuffer {
        let [r, g, b, a] = color;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("engine_compute_fill"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "@group(0) @binding(0) var slot: texture_storage_2d<rgba16float, write>;
                     @compute @workgroup_size(8, 8)
                     fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
                         let size = textureDimensions(slot);
                         if (id.x < size.x && id.y < size.y) {{
                             textureStore(slot, vec2<i32>(id.xy), vec4<f32>({r:?}, {g:?}, {b:?}, {a:?}));
                         }}
                     }}"
                )
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("engine_compute_fill"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("engine_compute_fill"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            let size = texture.size();
            pass.dispatch_workgroups(size.width.div_ceil(8), size.height.div_ceil(8), 1);
        }
        encoder.finish()
    }
}

/// Independent CPU reference for compositor colour tests (#197 acceptance:
/// near-black, saturated out-of-sRGB, fractional alpha, opacity and sRGB
/// controls, preview and wide).
#[cfg(test)]
// Used by the platform tests; Linux and Windows have none without `wgpu`.
#[cfg_attr(
    not(any(
        target_os = "macos",
        all(any(target_os = "linux", target_os = "windows"), feature = "wgpu")
    )),
    allow(dead_code)
)]
pub(crate) mod colour_reference {
    /// Stripe width in pixels; the strip is `PATCHES.len() * STRIPE` wide.
    pub const STRIPE: u32 = 8;
    pub const HEIGHT: u32 = 8;

    /// Associated (premultiplied) linear Display P3, exactly representable
    /// in binary16.
    pub const PATCHES: [(&str, [f32; 4]); 5] = [
        ("near-black", [0.001953125, 0.001953125, 0.001953125, 1.]),
        ("P3 green (outside sRGB)", [0., 1., 0., 1.]),
        ("P3 red (outside sRGB)", [1., 0., 0., 1.]),
        ("half alpha", [0.25, 0.125, 0., 0.5]),
        ("mid grey", [0.5, 0.5, 0.5, 1.]),
    ];

    /// Straight-alpha encoded sRGB BGRA8 controls (as R, G, B, A).
    pub const SRGB_CONTROLS: [(&str, [u8; 4]); 3] = [
        ("sRGB colour", [128, 64, 200, 255]),
        ("sRGB near-black", [3, 3, 3, 255]),
        ("sRGB half alpha", [255, 128, 0, 128]),
    ];

    pub fn width() -> u32 {
        PATCHES.len() as u32 * STRIPE
    }

    /// Centre x of stripe `i`.
    pub fn centre(i: usize) -> u32 {
        i as u32 * STRIPE + STRIPE / 2
    }

    /// IEEE binary16 bits, round to nearest even (normal and subnormal).
    pub fn f16_bits(value: f32) -> u16 {
        let bits = value.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
        let mantissa = bits & 0x7f_ffff;
        if value == 0. {
            return sign;
        }
        if exponent >= 31 {
            return sign | 0x7c00;
        }
        if exponent <= 0 {
            let shift = 14 - exponent;
            let m = (mantissa | 0x80_0000) as u64;
            let half = 1u64 << (shift - 1);
            let rounded = (m + half - 1 + ((m >> shift) & 1)) >> shift;
            return sign | rounded as u16;
        }
        let rounded = (mantissa + 0xfff + ((mantissa >> 13) & 1)) >> 13;
        sign | (((exponent as u32) << 10) + rounded) as u16
    }

    /// The patch strip as RGBA16F rows.
    pub fn strip_rgba16f() -> Vec<u8> {
        let mut bytes = Vec::new();
        for _ in 0..HEIGHT {
            for (_, patch) in PATCHES {
                for _ in 0..STRIPE {
                    for channel in patch {
                        bytes.extend_from_slice(&f16_bits(channel).to_ne_bytes());
                    }
                }
            }
        }
        bytes
    }

    /// The sRGB controls as BGRA8 rows, one stripe each.
    pub fn controls_bgra8() -> Vec<u8> {
        let mut bytes = Vec::new();
        for _ in 0..HEIGHT {
            for (_, [r, g, b, a]) in SRGB_CONTROLS {
                for _ in 0..STRIPE {
                    bytes.extend_from_slice(&[b, g, r, a]);
                }
            }
        }
        bytes
    }

    /// Linear Display P3 (D65) to linear sRGB (D65), from the primaries.
    const P3_TO_SRGB: [[f64; 3]; 3] = [
        [1.224_940_2, -0.224_940_2, 0.],
        [-0.042_056_95, 1.042_056_95, 0.],
        [-0.019_637_55, -0.078_636_05, 1.098_273_6],
    ];

    fn encode(linear: f64) -> f64 {
        let v = linear.clamp(0., 1.);
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        }
    }

    /// Expected 8-bit readback of a patch shown as an sRGB preview (clipped
    /// to sRGB) at `opacity`: the encoded straight colour times coverage, as
    /// composited over black (alpha is coverage over a transparent target).
    pub fn preview(patch: [f32; 4], opacity: f32) -> [f64; 4] {
        let alpha = patch[3] as f64;
        let straight: Vec<f64> = (0..3)
            .map(|c| {
                if alpha > 0. {
                    patch[c] as f64 / alpha
                } else {
                    0.
                }
            })
            .collect();
        let coverage = alpha * opacity as f64;
        let mut out = [0.; 4];
        for (row, value) in P3_TO_SRGB.iter().zip(out.iter_mut()) {
            let linear: f64 = row.iter().zip(&straight).map(|(m, v)| m * v).sum();
            *value = encode(linear) * 255. * coverage;
        }
        out[3] = coverage * 255.;
        out
    }

    /// Expected 8-bit readback of a straight sRGB control at `opacity`.
    pub fn control(rgba: [u8; 4], opacity: f32) -> [f64; 4] {
        let coverage = rgba[3] as f64 / 255. * opacity as f64;
        [
            rgba[0] as f64 * coverage,
            rgba[1] as f64 * coverage,
            rgba[2] as f64 * coverage,
            coverage * 255.,
        ]
    }

    /// Max colour-channel distance between a readback pixel and a reference.
    /// Headless compositors draw onto the window background (opaque black on
    /// macOS), so alpha is not compared; associated colour over black equals
    /// the reference either way.
    pub fn distance(actual: [u8; 4], expected: [f64; 4]) -> f64 {
        (0..3)
            .map(|c| (actual[c] as f64 - expected[c]).abs())
            .fold(0., f64::max)
    }
}

/// binary16 bits to f32 (macOS FP16 composition readbacks).
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn colour_reference_f16(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1. } else { 1. };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let mantissa = (bits & 0x3ff) as f32;
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1. + mantissa / 1024.) * 2f32.powi(exponent - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_descriptions() {
        let ok = SurfaceDesc {
            width: 64,
            height: 32,
            format: SurfaceFormat::Rgba16FloatLinearP3,
            slots: 3,
        };
        assert!(ok.validate().is_ok());
        for bad in [
            SurfaceDesc { width: 0, ..ok },
            SurfaceDesc {
                height: 16385,
                ..ok
            },
            SurfaceDesc { slots: 1, ..ok },
            SurfaceDesc { slots: 9, ..ok },
            SurfaceDesc {
                width: 16384,
                height: 16384,
                ..ok
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }
}
