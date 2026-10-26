//! Decoder-independent latest-frame presentation. No decoder or playback clock.
use anyhow::{Result, ensure};
use gpui::{
    App, Bounds, Corners, CustomContent, CustomGeometry, DevicePixels, Element, ElementId,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Size, Style,
    StyleRefinement, Styled, Window,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// Immutable producer content. Native producers must return a source-sized
/// backend output and keep external allocations stable through their lifetime.
#[derive(Clone, Debug)]
pub struct VideoFrame {
    content: Arc<dyn CustomContent>,
    size: Size<DevicePixels>,
    timestamp: Duration,
}
impl VideoFrame {
    /// Advanced backend adapter. Timestamp is source metadata, not a scheduling
    /// deadline. This channel presents the latest frame without clocking audio.
    pub fn new(
        size: Size<DevicePixels>,
        timestamp: Duration,
        content: Arc<dyn CustomContent>,
    ) -> Result<Self> {
        ensure!(
            size.width.0 > 0
                && size.height.0 > 0
                && size.width.0 <= 16384
                && size.height.0 <= 16384,
            "video dimensions must be 1..16384"
        );
        Ok(Self {
            content,
            size,
            timestamp,
        })
    }
    #[cfg(target_os = "macos")]
    pub fn core_video(
        frame: gpui_apple::custom_draw::core_video::CoreVideoFrame,
        timestamp: Duration,
    ) -> Self {
        Self {
            size: frame.size(),
            content: Arc::new(frame),
            timestamp,
        }
    }
    #[cfg(target_os = "windows")]
    pub fn dxgi(frame: gpui_windows::custom_draw::dxgi::DxgiFrame, timestamp: Duration) -> Self {
        Self {
            size: frame.size(),
            content: Arc::new(frame),
            timestamp,
        }
    }
    #[cfg(all(unix, feature = "wgpu"))]
    pub fn dma_buf(
        frame: gpui_wgpu::custom_draw::dma_buf::DmaBufFrame,
        timestamp: Duration,
    ) -> Self {
        Self {
            size: frame.size(),
            content: Arc::new(frame),
            timestamp,
        }
    }
    pub fn size(&self) -> Size<DevicePixels> {
        self.size
    }
    pub fn timestamp(&self) -> Duration {
        self.timestamp
    }
    /// Low-level painting uses source-sized content, native mask, corners and
    /// inherited opacity. The host supplies layout/aspect-ratio policy.
    pub fn paint(&self, window: &mut Window, bounds: Bounds<Pixels>, radii: Corners<Pixels>) {
        if bounds.is_empty()
            || bounds.intersect(&window.content_mask().bounds).is_empty()
            || window.element_opacity() <= 0.
        {
            return;
        }
        window.paint_custom(
            CustomGeometry::RoundedRect {
                bounds,
                corner_radii: radii,
            },
            self.content.clone(),
        );
    }
    pub fn content(&self) -> Arc<dyn CustomContent> {
        self.content.clone()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoStats {
    pub published: u64,
    /// Pending frames superseded before any presentation snapshot observed them.
    pub replaced: u64,
    pub displayed: u64,
    pub pending: bool,
    pub closed: bool,
    pub playing: bool,
}
struct Mailbox {
    pending: Option<VideoFrame>,
    displayed: Option<VideoFrame>,
    stats: VideoStats,
    receivers: usize,
}
/// One producer endpoint. Dropping it closes the stream; the last presented
/// frame remains visible. Publish replaces at most one pending snapshot.
pub struct VideoSender(Arc<Mutex<Mailbox>>);
impl VideoSender {
    pub fn publish(&self, frame: VideoFrame) -> Result<()> {
        let mut m = self.0.lock().unwrap();
        ensure!(!m.stats.closed, "video stream is closed");
        if m.pending.replace(frame).is_some() {
            m.stats.replaced = m.stats.replaced.saturating_add(1);
        }
        m.stats.published = m.stats.published.saturating_add(1);
        m.stats.pending = true;
        Ok(())
    }
    pub fn close(&self) {
        self.0.lock().unwrap().stats.closed = true;
    }
}
impl Drop for VideoSender {
    fn drop(&mut self) {
        self.close();
    }
}
/// Shared presentation state. Clones display the same snapshot and share pause.
/// There is one pending frame plus one displayed frame; GPU in-flight lifetimes
/// are separate and managed by the backend. No unbounded frame FIFO is built.
pub struct VideoStream(Arc<Mutex<Mailbox>>);
impl Clone for VideoStream {
    fn clone(&self) -> Self {
        self.0.lock().unwrap().receivers += 1;
        Self(self.0.clone())
    }
}
impl Drop for VideoStream {
    fn drop(&mut self) {
        let mut m = self.0.lock().unwrap();
        m.receivers -= 1;
        if m.receivers == 0 {
            m.stats.closed = true;
            m.stats.pending = false;
            m.pending = None;
            m.displayed = None;
        }
    }
}
impl VideoStream {
    pub fn channel() -> (VideoSender, Self) {
        let m = Arc::new(Mutex::new(Mailbox {
            pending: None,
            displayed: None,
            stats: VideoStats {
                playing: true,
                ..Default::default()
            },
            receivers: 1,
        }));
        (VideoSender(m.clone()), Self(m))
    }
    /// While paused, publishers can replace pending frames; displayed content is
    /// retained. The host should notify its view after changing presentation state.
    pub fn set_playing(&self, playing: bool) {
        self.0.lock().unwrap().stats.playing = playing;
    }
    pub fn stats(&self) -> VideoStats {
        self.0.lock().unwrap().stats
    }
    pub fn snapshot(&self) -> Option<VideoFrame> {
        let mut m = self.0.lock().unwrap();
        if m.stats.playing
            && let Some(frame) = m.pending.take()
        {
            m.displayed = Some(frame);
            m.stats.displayed = m.stats.displayed.saturating_add(1);
            m.stats.pending = false;
        }
        m.displayed.clone()
    }
    fn live(&self) -> bool {
        let s = self.stats();
        s.playing && !s.closed
    }
}
/// A styled latest-frame viewport. Explicit bounds stretch the frame; provide
/// source-aspect dimensions. Live visible elements poll at host animation cadence.
/// Paused/closed, fully clipped, empty and zero-opacity elements request no frames.
pub fn video(stream: VideoStream) -> VideoView {
    VideoView {
        stream: Some(stream),
        frame: None,
        style: StyleRefinement::default(),
    }
}
/// Paint one immutable snapshot without requesting live presentation frames.
/// Hosts can capture one stream snapshot and reuse it across several placements.
pub fn video_frame(frame: VideoFrame) -> VideoView {
    VideoView {
        stream: None,
        frame: Some(frame),
        style: StyleRefinement::default(),
    }
}
pub struct VideoView {
    stream: Option<VideoStream>,
    frame: Option<VideoFrame>,
    style: StyleRefinement,
}
impl Styled for VideoView {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl IntoElement for VideoView {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for VideoView {
    type RequestLayoutState = Style;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Style) {
        use refineable::Refineable;
        let mut style = Style::default();
        style.refine(&self.style);
        (window.request_layout(style.clone(), [], cx), style)
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Style,
        _: &mut Window,
        _: &mut App,
    ) {
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Style,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let radii = style
            .corner_radii
            .to_pixels(window.rem_size())
            .clamp_radii_for_quad_size(bounds.size);
        window.with_element_opacity(style.opacity, |window| {
            style.paint(bounds, window, cx, |window, _| {
                if bounds.is_empty()
                    || bounds.intersect(&window.content_mask().bounds).is_empty()
                    || window.element_opacity() <= 0.
                {
                    return;
                }
                if let Some(frame) = self
                    .frame
                    .clone()
                    .or_else(|| self.stream.as_ref().and_then(VideoStream::snapshot))
                {
                    frame.paint(window, bounds, radii);
                }
                if self.stream.as_ref().is_some_and(VideoStream::live) {
                    window.request_animation_frame();
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug)]
    struct Empty;
    impl CustomContent for Empty {
        fn prepare(
            &self,
            _: &dyn std::any::Any,
            _: Size<DevicePixels>,
        ) -> Result<Box<dyn std::any::Any>> {
            anyhow::bail!("test content")
        }
    }
    fn frame(n: u64) -> VideoFrame {
        VideoFrame::new(
            gpui::size(DevicePixels(16), DevicePixels(9)),
            Duration::from_millis(n),
            Arc::new(Empty),
        )
        .unwrap()
    }
    #[test]
    fn bounded_replacement_pause_resume_and_close() {
        let (sender, stream) = VideoStream::channel();
        for n in 0..100 {
            sender.publish(frame(n)).unwrap();
        }
        assert_eq!(stream.stats().replaced, 99);
        assert_eq!(
            stream.snapshot().unwrap().timestamp(),
            Duration::from_millis(99)
        );
        assert_eq!(stream.stats().displayed, 1);
        assert!(!stream.stats().pending);
        stream.set_playing(false);
        sender.publish(frame(100)).unwrap();
        sender.publish(frame(101)).unwrap();
        assert_eq!(
            stream.snapshot().unwrap().timestamp(),
            Duration::from_millis(99)
        );
        assert!(!stream.live());
        stream.set_playing(true);
        assert_eq!(
            stream.snapshot().unwrap().timestamp(),
            Duration::from_millis(101)
        );
        drop(sender);
        assert!(stream.stats().closed);
        assert!(!stream.live());
        assert_eq!(
            stream.snapshot().unwrap().timestamp(),
            Duration::from_millis(101)
        );
    }
    #[test]
    fn closed_stream_keeps_final_pending_frame_and_rejects_publish() {
        let (sender, stream) = VideoStream::channel();
        sender.publish(frame(1)).unwrap();
        sender.close();
        assert!(sender.publish(frame(2)).is_err());
        assert_eq!(
            stream.snapshot().unwrap().timestamp(),
            Duration::from_millis(1)
        );
        assert!(
            VideoFrame::new(
                gpui::size(DevicePixels(0), DevicePixels(1)),
                Duration::ZERO,
                Arc::new(Empty)
            )
            .is_err()
        );
    }
    #[test]
    fn snapshots_release_with_replacement_and_last_consumer_drop() {
        let (sender, stream) = VideoStream::channel();
        let content = Arc::new(Empty);
        let weak = Arc::downgrade(&content);
        sender
            .publish(
                VideoFrame::new(
                    gpui::size(DevicePixels(16), DevicePixels(9)),
                    Duration::ZERO,
                    content.clone(),
                )
                .unwrap(),
            )
            .unwrap();
        drop(content);
        let snapshot = stream.snapshot().unwrap();
        sender.publish(frame(1)).unwrap();
        stream.snapshot();
        assert!(
            weak.upgrade().is_some(),
            "external snapshot retains old content"
        );
        drop(snapshot);
        assert!(weak.upgrade().is_none());
        let clone = stream.clone();
        drop(stream);
        sender.publish(frame(2)).unwrap();
        assert!(clone.snapshot().is_some());
        drop(clone);
        assert!(
            sender.publish(frame(3)).is_err(),
            "last consumer closes producer admission"
        );
    }
}
