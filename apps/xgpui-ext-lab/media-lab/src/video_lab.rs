#[cfg(target_os = "windows")]
mod windows;

use crate::{model::Lab, ui::button};
use anyhow::Result;
use gpui::{prelude::*, *};
use std::path::Path as FilePath;

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceFormat {
    #[default]
    Bgra,
    #[cfg(target_os = "macos")]
    Full601,
    #[cfg(target_os = "macos")]
    Video601,
    #[cfg(target_os = "macos")]
    Full709,
    #[cfg(target_os = "macos")]
    Video709,
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SourceFormat {
    const ALL: &'static [Self] = &[
        Self::Bgra,
        #[cfg(target_os = "macos")]
        Self::Full601,
        #[cfg(target_os = "macos")]
        Self::Video601,
        #[cfg(target_os = "macos")]
        Self::Full709,
        #[cfg(target_os = "macos")]
        Self::Video709,
    ];
    fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|f| *f == self).unwrap() + 1) % Self::ALL.len()]
    }
    fn label(self) -> &'static str {
        match self {
            Self::Bgra => "BGRA8",
            #[cfg(target_os = "macos")]
            Self::Full601 => "NV12 full · BT.601",
            #[cfg(target_os = "macos")]
            Self::Video601 => "NV12 video · BT.601",
            #[cfg(target_os = "macos")]
            Self::Full709 => "NV12 full · BT.709",
            #[cfg(target_os = "macos")]
            Self::Video709 => "NV12 video · BT.709",
        }
    }
    #[cfg(target_os = "macos")]
    fn slug(self) -> &'static str {
        match self {
            Self::Bgra => "bgra",
            Self::Full601 => "nv12-full-601",
            Self::Video601 => "nv12-video-601",
            Self::Full709 => "nv12-full-709",
            Self::Video709 => "nv12-video-709",
        }
    }
    #[cfg(target_os = "macos")]
    fn frame(self, index: u64) -> Result<xgpui_media::CoreVideoFrame> {
        use gpui_apple::custom_draw::core_video::{Nv12Matrix, test_frame, test_nv12_frame};
        match self {
            Self::Bgra => test_frame(320, 180, index),
            Self::Full601 => test_nv12_frame(320, 180, index, Nv12Matrix::Bt601, false),
            Self::Video601 => test_nv12_frame(320, 180, index, Nv12Matrix::Bt601, true),
            Self::Full709 => test_nv12_frame(320, 180, index, Nv12Matrix::Bt709, false),
            Self::Video709 => test_nv12_frame(320, 180, index, Nv12Matrix::Bt709, true),
        }
    }
    fn video_frame(self, index: u64) -> Result<xgpui_media::VideoFrame> {
        let timestamp = std::time::Duration::from_micros(index * 33_333);
        #[cfg(target_os = "macos")]
        {
            Ok(xgpui_media::VideoFrame::core_video(
                self.frame(index)?,
                timestamp,
            ))
        }
        #[cfg(target_os = "windows")]
        {
            windows::Snapshot::new(index).video_frame(timestamp)
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native {
    use super::*;
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };
    #[cfg(target_os = "macos")]
    use xgpui_media::VideoFrame;
    use xgpui_media::{VideoStream, video_frame};
    pub struct Demo {
        pub stream: VideoStream,
        stop: Arc<AtomicBool>,
        burst: mpsc::SyncSender<()>,
        error: Arc<Mutex<Option<String>>>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Demo {
        pub fn new(format: SourceFormat) -> Result<Self> {
            let (sender, stream) = VideoStream::channel();
            sender.publish(format.video_frame(0)?)?;
            let stop = Arc::new(AtomicBool::new(false));
            let done = stop.clone();
            let error = Arc::new(Mutex::new(None));
            let report = error.clone();
            let (burst, requests) = mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name("media-lab-video".into())
                .spawn(move || {
                    let mut index = 1;
                    while !done.load(Ordering::Relaxed) {
                        let start = Instant::now();
                        let count = if requests.try_recv().is_ok() { 120 } else { 1 };
                        for _ in 0..count {
                            if done.load(Ordering::Relaxed) {
                                break;
                            }
                            let result = format.video_frame(index).and_then(|f| sender.publish(f));
                            if let Err(e) = result {
                                *report.lock().unwrap() = Some(e.to_string());
                                return;
                            }
                            index += 1;
                        }
                        std::thread::park_timeout(
                            Duration::from_millis(33).saturating_sub(start.elapsed()),
                        );
                    }
                })?;
            Ok(Self {
                stream,
                stop,
                burst,
                error,
                worker: Some(worker),
            })
        }
    }
    impl Drop for Demo {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(worker) = self.worker.take() {
                worker.thread().unpark();
                if worker.join().is_err() {
                    eprintln!("video source worker panicked during shutdown");
                }
            }
        }
    }
    pub fn render(lab: &mut Lab, window: &mut Window, cx: &mut Context<Lab>) -> Stateful<Div> {
        let root = div()
            .id("video-lab")
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .bg(rgb(0x111619))
            .text_color(rgb(0xe8edeb))
            .font_family("Helvetica");
        if lab.video_demo.is_none() {
            match Demo::new(lab.video_format) {
                Ok(d) => lab.video_demo = Some(d),
                Err(e) => return root.child(format!("Video source failed: {e}")),
            }
        }
        let demo = lab.video_demo.as_ref().unwrap();
        let snapshot = demo.stream.snapshot();
        let s = demo.stream.stats();
        if s.playing && !s.closed {
            window.request_animation_frame();
        }
        let stamp = snapshot
            .as_ref()
            .map(|f| f.timestamp().as_secs_f64())
            .unwrap_or(0.);
        let width = (1020. / window.scale_factor()
            - 2. * (16. + 8. / window.scale_factor()) * MAX_LAYER_GROUPS as f32)
            .clamp(80., 640.)
            .min(((f32::from(window.viewport_size().width) - 68.) / 2.).max(80.));
        let viewport = div()
            .flex_shrink_0()
            .relative()
            .w(px(width))
            .h(px(width * 9. / 16.))
            .rounded(px(lab.radius))
            .overflow_hidden()
            .child(crate::ui::checker())
            .children(snapshot.clone().map(|frame| {
                video_frame(frame)
                    .adjustments(lab.image_adjustments())
                    .blur(lab.video_blurs[lab.video_blur].clone())
                    .size_full()
                    .rounded(px(lab.radius))
                    .opacity(lab.opacity)
            }));
        let reference = div()
            .relative()
            .w(px(width))
            .h(px(width * 9. / 16.))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded(px(lab.radius))
            .child(crate::ui::checker())
            .children(snapshot.clone().map(|frame| {
                video_frame(frame)
                    .size_full()
                    .rounded(px(lab.radius))
                    .opacity(lab.opacity)
            }));
        let panel = |title: &'static str, caption: &'static str, preview: Div| {
            div()
                .w(px(width))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(preview)
                .child(div().text_sm().text_color(rgb(0x94a19f)).child(caption))
        };
        let previews = div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap_5()
            .child(panel(
                "Adjusted preview",
                "Linear adjustments, vignette and bounded preview blur",
                viewport,
            ))
            .child(panel(
                "Original reference",
                "Same immutable source snapshot without effects",
                reference,
            ));
        let diagnostic = div()
            .w_full()
            .p_4()
            .rounded_md()
            .bg(rgb(0x191f23))
            .flex()
            .flex_col()
            .gap_2()
            .text_sm()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Frame tracking · shared by both previews"),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_5()
                    .child(format!("Published: {}", s.published))
                    .child(format!("Replaced before display: {}", s.replaced))
                    .child(format!("Observed snapshots: {}", s.displayed))
                    .child(format!("Pending: {} / 1", usize::from(s.pending))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_5()
                    .child(if s.closed {
                        "Source closed · last frame retained"
                    } else {
                        "Source live"
                    })
                    .child(format!("Source timestamp: {stamp:.3}s"))
                    .child(format!(
                        "320 × 180 · {} {}",
                        source_label(),
                        lab.video_format.label()
                    )),
            )
            .child(div().text_color(rgb(0x94a19f)).child(source_note()));
        let controls = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap_2()
            .child(
                button("video-back", "← Images", false).on_click(cx.listener(|this, _, _, cx| {
                    this.video_mode = false;
                    this.video_demo = None;
                    cx.notify();
                })),
            )
            .child(
                button(
                    "video-play",
                    if s.playing {
                        "Pause presentation"
                    } else {
                        "Resume presentation"
                    },
                    s.playing,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(d) = &this.video_demo {
                        d.stream.set_playing(!d.stream.stats().playing);
                    }
                    cx.notify();
                })),
            )
            .child(
                button(
                    "video-format",
                    format!("Format: {}", lab.video_format.label()),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.video_format = this.video_format.next();
                    this.video_demo = None;
                    cx.notify();
                })),
            )
            .child(
                button("video-burst", "Burst 120 frames", false).on_click(cx.listener(
                    |this, _, _, cx| {
                        if let Some(d) = &this.video_demo {
                            let _ = d.burst.try_send(());
                        }
                        cx.notify();
                    },
                )),
            )
            .child(
                button(
                    "video-opacity",
                    format!("Opacity {:.0}%", lab.opacity * 100.),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.opacity = if this.opacity > 0.9 { 0.5 } else { 1. };
                    cx.notify();
                })),
            )
            .child(
                button("video-radius", format!("Corners {:.0}", lab.radius), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.radius = if this.radius < 30. { 48. } else { 0. };
                        cx.notify();
                    }),
                ),
            )
            .child(
                button("video-stop", "Stop source", false).on_click(cx.listener(
                    |this, _, _, cx| {
                        if let Some(d) = &this.video_demo {
                            d.stop.store(true, Ordering::Relaxed);
                        }
                        cx.notify();
                    },
                )),
            )
            .child(
                button("video-restart", "Restart source", false).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.video_demo = None;
                        cx.notify();
                    },
                )),
            );
        let controls =
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(controls)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            button(
                                "video-ev",
                                format!("Exposure: {} EV", [0, -1, 1, 2][lab.adjustments[0]]),
                                false,
                            )
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.adjustments[0] = (s.adjustments[0] + 1) % 4;
                                cx.notify();
                            })),
                        )
                        .child(
                            button(
                                "video-saturation",
                                format!("Saturation: {}", [1., 0., 1.8][lab.adjustments[2]]),
                                false,
                            )
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.adjustments[2] = (s.adjustments[2] + 1) % 3;
                                cx.notify();
                            })),
                        )
                        .child(
                            button(
                                "video-vignette",
                                format!("Vignette: {}", lab.adjustments[3]),
                                false,
                            )
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.adjustments[3] = (s.adjustments[3] + 1) % 3;
                                cx.notify();
                            })),
                        )
                        .child(
                            button(
                                "video-blur",
                                format!("Blur: {} px", [0, 4, 8, 16][lab.video_blur]),
                                false,
                            )
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.video_blur = (s.video_blur + 1) % 4;
                                cx.notify();
                            })),
                        )
                        .child(button("video-reset", "Reset effects", false).on_click(
                            cx.listener(|s, _, _, cx| {
                                s.adjustments = [0; 4];
                                s.video_blur = 0;
                                cx.notify();
                            }),
                        )),
                );
        root.child(div().text_size(px(28.)).font_weight(FontWeight::SEMIBOLD).child("Video surfaces"))
            .child(format!("Synthetic 30 fps source · {} · bounded latest-frame presentation", source_label()))
            .child(controls).child(previews).child(diagnostic)
            .children(demo.error.lock().unwrap().clone().map(|e|div().text_color(rgb(0xffaa99)).child(e)))
            .child(div().text_sm().text_color(rgb(0x94a19f)).child("Pause holds the displayed snapshot while the producer continues replacing pending frames. Resume shows the newest frame. Source timestamps are diagnostic metadata; this is not a decoder or A/V clock."))
            .child(div().text_sm().text_color(rgb(0x94a19f)).child(source_note()).child("In-flight GPU frames have separate lifetimes; channel capacity is not a total memory limit."))
    }
    #[cfg(target_os = "macos")]
    pub fn capture(png: &FilePath) -> Result<()> {
        let mut renderer = gpui_platform::current_headless_renderer()?
            .ok_or_else(|| anyhow::anyhow!("native renderer unavailable"))?;
        for &format in SourceFormat::ALL {
            let path = if format == SourceFormat::Bgra {
                png.to_path_buf()
            } else {
                png.with_file_name(format!(
                    "{}-{}.png",
                    png.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("video-lab"),
                    format.slug()
                ))
            };
            capture_format(&mut *renderer, &path, format)?;
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    fn capture_format(
        renderer: &mut dyn PlatformHeadlessRenderer,
        png: &FilePath,
        format: SourceFormat,
    ) -> Result<()> {
        use anyhow::ensure;
        let frame = format.frame(0)?;
        let video = VideoFrame::core_video(frame.clone(), Duration::ZERO);
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
        let extent = size(DevicePixels(328), DevicePixels(188));
        let pixels = renderer.render_scene_to_image(&scene, extent)?;
        for (x, expected) in [
            (24, [224u8, 64, 32]),
            (104, [64, 208, 64]),
            (184, [32, 64, 224]),
            (264, [224, 192, 96]),
        ] {
            let pixel = pixels.get_pixel(x, 24).0;
            ensure!(
                (0..3).all(|i| pixel[i].abs_diff(expected[i]) <= 3),
                "{} RGB/range/matrix at {x}: {pixel:?}",
                format.label()
            );
        }
        ensure!(
            pixels.get_pixel(4, 4).0[..3] == [0, 0, 0],
            "rounded corner coverage"
        );
        scene.custom_draws[0].opacity = 0.5;
        let alpha = renderer.render_scene_to_image(&scene, extent)?;
        ensure!(
            alpha.get_pixel(24, 24).0[0].abs_diff(112) <= 2,
            "inherited video opacity"
        );
        scene.custom_draws[0].content_mask.bounds.origin.x = ScaledPixels(20.);
        scene.custom_draws[0].content_mask.bounds.size.width = ScaledPixels(304.);
        let clipped = renderer.render_scene_to_image(&scene, extent)?;
        ensure!(
            clipped.get_pixel(10, 50).0[..3] == [0, 0, 0],
            "native video content mask"
        );
        scene.custom_draws[0].bounds.size = size(ScaledPixels(160.), ScaledPixels(90.));
        renderer.render_scene_to_image(&scene, extent)?;
        ensure!(
            frame.import_count() == 1,
            "resize/replay must reuse imported texture"
        );
        pixels.save(png)?;
        println!(
            "Saved {}: {} IOSurface; RGB/range/matrix, clipping, corners, opacity and resize reuse passed.",
            png.display(),
            format.label()
        );
        Ok(())
    }

    #[cfg(test)]
    mod lifecycle_tests {
        use super::*;

        #[::core::prelude::v1::test]
        fn source_shutdown_retains_last_frame_and_restart_is_independent() -> Result<()> {
            let demo = Demo::new(SourceFormat::Bgra)?;
            let stream = demo.stream.clone();
            let shown = stream.snapshot().expect("synchronous initial frame");
            stream.set_playing(false);
            drop(demo); // Joins the worker; no delayed producer survives a restart.
            anyhow::ensure!(
                stream.stats().closed,
                "source shutdown must close the producer"
            );
            anyhow::ensure!(
                stream.snapshot().unwrap().timestamp() == shown.timestamp(),
                "closed paused stream retains its displayed frame"
            );
            let restarted = Demo::new(SourceFormat::Bgra)?;
            anyhow::ensure!(!restarted.stream.stats().closed, "new source must be live");
            anyhow::ensure!(
                restarted.stream.snapshot().is_some(),
                "restart has an initial frame"
            );
            anyhow::ensure!(
                stream.stats().closed,
                "restart cannot revive the old stream"
            );
            Ok(())
        }
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use native::{Demo, render};
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn render(_: &mut Lab, _: &mut Window, cx: &mut Context<Lab>) -> Stateful<Div> {
    div()
        .id("video-unavailable")
        .p_6()
        .child("This interactive synthetic source requires macOS Metal or Windows D3D11. Linux DMA-BUF adapters have separate native checks.")
        .child(
            button("video-back", "← Images", false).on_click(cx.listener(|this, _, _, cx| {
                this.video_mode = false;
                cx.notify();
            })),
        )
}
pub fn capture(png: &FilePath, wgpu: bool) -> Result<()> {
    anyhow::ensure!(
        !wgpu,
        "Native video surfaces require Metal or D3D11, not wgpu"
    );
    #[cfg(target_os = "macos")]
    {
        native::capture(png)
    }
    #[cfg(target_os = "windows")]
    {
        windows::capture(png)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = png;
        anyhow::bail!("native video capture requires macOS Metal or Windows D3D11")
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn source_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Metal / IOSurface"
    }
    #[cfg(target_os = "windows")]
    {
        "D3D11 / DXGI"
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn source_note() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Fresh CPU pixels in IOSurfaces; native texture views without a media staging upload."
    }
    #[cfg(target_os = "windows")]
    {
        "CPU-generated BGRA initializes one immutable D3D11 texture per displayed source frame; native DXGI sampling reuses it for both previews and redraws."
    }
}
