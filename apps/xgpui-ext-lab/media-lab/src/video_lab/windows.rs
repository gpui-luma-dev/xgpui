//! A CPU-generated immutable source, initialized once on the compositor device.
//! This tests native DXGI presentation, not decoding or external-device sharing.
use ::windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device,
        },
        Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
    },
    core::Interface,
};
use anyhow::{Context, Result, ensure};
use gpui::*;
use gpui_windows::custom_draw::{Dx11CustomContext, dxgi::DxgiFrame};
use std::{
    any::Any,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use xgpui_media::VideoFrame;

use super::windows_pattern::{self, COLORS, HEIGHT, WIDTH};

#[derive(Debug)]
struct Imported {
    // Retaining the device prevents identity reuse while its frame is cached.
    device: ID3D11Device,
    frame: DxgiFrame,
}

#[derive(Debug)]
pub(super) struct Snapshot {
    pixels: Vec<u8>,
    imported: Mutex<Option<Imported>>,
    initializations: AtomicUsize,
}
impl Snapshot {
    pub(super) fn new(index: u64) -> Arc<Self> {
        Arc::new(Self {
            pixels: windows_pattern::pixels(index),
            imported: Mutex::new(None),
            initializations: AtomicUsize::new(0),
        })
    }
    pub(super) fn video_frame(self: &Arc<Self>, timestamp: Duration) -> Result<VideoFrame> {
        VideoFrame::new(extent(), timestamp, self.clone())
    }
}
fn extent() -> Size<DevicePixels> {
    size(DevicePixels(WIDTH as i32), DevicePixels(HEIGHT as i32))
}
impl CustomContent for Snapshot {
    fn image_source_size(&self) -> Option<Size<DevicePixels>> {
        Some(extent())
    }

    fn prepare(&self, backend: &dyn Any, size: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let context = backend
            .downcast_ref::<Dx11CustomContext>()
            .context("Windows video snapshots require native D3D11, not wgpu")?;
        let mut cache = self
            .imported
            .lock()
            .map_err(|_| anyhow::anyhow!("video snapshot cache poisoned"))?;
        if cache
            .as_ref()
            .is_none_or(|cached| cached.device.as_raw() != context.device.as_raw())
        {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: WIDTH,
                Height: HEIGHT,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                ..Default::default()
            };
            let initial = D3D11_SUBRESOURCE_DATA {
                pSysMem: self.pixels.as_ptr().cast(),
                SysMemPitch: WIDTH * 4,
                SysMemSlicePitch: WIDTH * HEIGHT * 4,
            };
            let mut texture = None;
            // SAFETY: bytes cover every row and remain alive until CreateTexture2D
            // has copied the initial data. The device is supplied by the renderer.
            unsafe {
                context
                    .device
                    .CreateTexture2D(&desc, Some(&initial), Some(&mut texture))?;
            }
            let texture = texture.context("D3D11 did not create a video snapshot")?;
            // SAFETY: this allocation is never mutated, pooled or recycled. Only
            // the immutable DXGI adapter receives it; prepared outputs retain COM
            // ownership independently of this source/cache and the producer.
            let frame = unsafe { DxgiFrame::from_bgra(texture) }?;
            *cache = Some(Imported {
                device: context.device.clone(),
                frame,
            });
            self.initializations.fetch_add(1, Ordering::Relaxed);
        }
        cache.as_ref().unwrap().frame.prepare(context, size)
    }
}

/// Pixel/lifetime checks also used by the Windows CI test. No window or decoder.
pub(super) fn capture(png: &Path) -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?
        .context("native Windows D3D11 headless renderer unavailable")?;
    let source = Snapshot::new(0);
    let video = source.video_frame(Duration::ZERO)?;
    let bounds = Bounds::new(
        point(ScaledPixels(4.), ScaledPixels(4.)),
        size(ScaledPixels(320.), ScaledPixels(180.)),
    );
    let mut scene = Scene::default();
    scene.insert_primitive(CustomDraw {
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::all(ScaledPixels(12.)),
        content: video.content(),
        coverage: None,
        opacity: 1.,
        order: Default::default(),
    });
    scene.finish();
    let output_size = size(DevicePixels(328), DevicePixels(188));
    let pixels = renderer.render_scene_to_image(&scene, output_size)?;
    for (x, expected) in [24, 104, 184, 264].into_iter().zip(COLORS) {
        let pixel = pixels.get_pixel(x, 24).0;
        ensure!(
            (0..3).all(|i| pixel[i].abs_diff(expected[i]) <= 3),
            "DXGI BGRA color at {x}: {pixel:?}"
        );
    }
    let translucent = pixels.get_pixel(264, 124).0;
    ensure!(
        (0..3).all(|i| translucent[i].abs_diff((u16::from(COLORS[3][i]) * 128 / 255) as u8) <= 3),
        "DXGI straight alpha: {translucent:?}"
    );
    ensure!(
        pixels.get_pixel(4, 4).0[..3] == [0, 0, 0],
        "DXGI rounded corners"
    );
    let next = Snapshot::new(8);
    scene.custom_draws[0].content = next.video_frame(Duration::from_millis(264))?.content();
    let moving = renderer.render_scene_to_image(&scene, output_size)?;
    let changed = pixels
        .pixels()
        .zip(moving.pixels())
        .filter(|(a, b)| a != b)
        .count();
    ensure!(
        changed > (WIDTH * HEIGHT / 4) as usize,
        "DXGI playback must update the animated region: {changed} pixels changed"
    );
    scene.custom_draws[0].content = video.content();
    drop(next);
    use xgpui_media::pipeline::{ColorMatrix, ImageAdjustments, ImagePrimaries};
    scene.custom_draws[0].content = video
        .adjusted(ImageAdjustments::new(
            ColorMatrix::identity(ImagePrimaries::Srgb).exposure(-1.)?,
            None,
        )?)
        .content();
    let adjusted = renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        (150..=180).contains(&adjusted.get_pixel(24, 24).0[0]),
        "DXGI linear exposure must affect the adjusted preview"
    );
    scene.custom_draws[0].content = video.content();
    scene.custom_draws[0].opacity = 0.5;
    let opacity = renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        opacity.get_pixel(24, 24).0[0].abs_diff(112) <= 3,
        "DXGI inherited opacity"
    );
    scene.custom_draws[0].content_mask.bounds.origin.x = ScaledPixels(20.);
    scene.custom_draws[0].content_mask.bounds.size.width = ScaledPixels(304.);
    let clipped = renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        clipped.get_pixel(10, 50).0[..3] == [0, 0, 0],
        "DXGI content mask"
    );
    let mut duplicate = scene.custom_draws[0].clone();
    duplicate.bounds.size = size(ScaledPixels(160.), ScaledPixels(90.));
    scene.custom_draws.push(duplicate);
    renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        source.initializations.load(Ordering::Relaxed) == 1,
        "both placements and resize must reuse one DXGI texture"
    );

    // Dropping the producer/cache does not invalidate already prepared outputs.
    // Scene content retains this source until the last presentation drops it.
    let weak = Arc::downgrade(&source);
    drop(source);
    drop(video);
    renderer.render_scene_to_image(&scene, output_size)?;
    ensure!(
        weak.upgrade().is_some(),
        "scene must retain its source after producer drop"
    );
    scene.custom_draws.clear();
    drop(scene);
    drop(renderer);
    ensure!(
        weak.upgrade().is_none(),
        "source must retire after scene/renderer teardown"
    );
    pixels.save(png)?;
    println!(
        "Saved {}: DXGI BGRA colors, alpha, corners, opacity, clipping, placement reuse and teardown passed.",
        png.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgpui_media::VideoStream;

    #[::core::prelude::v1::test]
    fn dxgi_video_capture_pixels_and_lifetimes() -> Result<()> {
        let png = std::env::temp_dir().join(format!("media-lab-dxgi-{}.png", std::process::id()));
        capture(&png)
    }

    #[::core::prelude::v1::test]
    fn dxgi_video_snapshots_survive_pause_replacement_and_producer_shutdown() -> Result<()> {
        let (sender, stream) = VideoStream::channel();
        let first = Snapshot::new(0);
        let weak = Arc::downgrade(&first);
        sender.publish(first.video_frame(Duration::ZERO)?)?;
        drop(first);
        let shown = stream.snapshot().context("first video frame")?;
        stream.set_playing(false);
        for i in 1..=120 {
            sender.publish(Snapshot::new(i).video_frame(Duration::from_millis(i))?)?;
        }
        ensure!(
            stream.snapshot().unwrap().timestamp() == Duration::ZERO,
            "pause must retain first frame"
        );
        ensure!(
            stream.stats().replaced == 119 && stream.stats().pending,
            "burst must replace one pending slot"
        );
        stream.set_playing(true);
        ensure!(
            stream.snapshot().unwrap().timestamp() == Duration::from_millis(120),
            "resume takes latest frame"
        );
        ensure!(
            weak.upgrade().is_some(),
            "external snapshot keeps old source alive"
        );
        drop(shown);
        ensure!(
            weak.upgrade().is_none(),
            "old source retires after last snapshot drops"
        );
        drop(sender);
        ensure!(stream.stats().closed, "producer shutdown closes stream");
        ensure!(
            stream.snapshot().unwrap().timestamp() == Duration::from_millis(120),
            "closed stream retains last frame"
        );
        let frame = Snapshot::new(0);
        ensure!(
            frame.prepare(&(), extent()).is_err(),
            "unsupported backends must fail explicitly"
        );
        Ok(())
    }
}
