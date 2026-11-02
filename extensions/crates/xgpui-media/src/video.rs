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

#[cfg(any(target_os = "windows", all(unix, feature = "wgpu")))]
#[derive(Debug)]
struct AdjustedFrame {
    source: Arc<dyn CustomContent>,
    adjustments: crate::pipeline::ImageAdjustments,
}
#[cfg(any(target_os = "windows", all(unix, feature = "wgpu")))]
impl CustomContent for AdjustedFrame {
    fn image_color_mode(&self) -> gpui::ImageColorMode {
        self.source.image_color_mode()
    }
    fn image_source_size(&self) -> Option<Size<DevicePixels>> {
        self.source.image_source_size()
    }
    fn image_sample_opacity(&self) -> f32 {
        self.source.image_sample_opacity()
    }
    fn image_linear_adjustments(&self) -> Option<gpui::LinearImagePaint> {
        Some(self.adjustments.paint())
    }
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        size: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        self.source.prepare(backend, size)
    }
    fn prepare_with_request(
        &self,
        backend: &dyn std::any::Any,
        request: &gpui::PreparationRequest,
    ) -> Result<Box<dyn std::any::Any>> {
        self.source.prepare_with_request(backend, request)
    }
    fn prepare_in_frame(
        &self,
        backend: &dyn std::any::Any,
        request: &gpui::PreparationRequest,
        frame: &gpui::PreparationFrame,
    ) -> Result<Box<dyn std::any::Any>> {
        self.source.prepare_in_frame(backend, request, frame)
    }
}

#[derive(Debug)]
struct FrameSource {
    content: gpui::content::Content,
    request: gpui::content::ContentRequest,
}
impl gpui::content::ContentSource for FrameSource {
    fn describe(
        &self,
        cx: &mut gpui::content::ContentBuilder,
        _: &gpui::content::ContentRequest,
    ) -> std::result::Result<gpui::content::ImageRef, gpui::content::ContentError> {
        let image = cx.source(&self.content, &self.request)?;
        if cx.image_descriptor(image)?.extent() != self.request.extent() {
            return Err(gpui::content::ContentError::new(
                gpui::content::ContentErrorKind::InvalidDescriptor,
                "video content differs from its declared source extent",
            ));
        }
        Ok(image)
    }
}
#[derive(Clone, Debug)]
enum FrameContent {
    Portable(gpui::content::ContentDraw),
    #[cfg(any(target_os = "windows", all(unix, feature = "wgpu")))]
    Native(Arc<dyn CustomContent>),
}
/// Immutable frame pixels/description, independent of decoder and playback clock.
#[derive(Clone, Debug)]
pub struct VideoFrame {
    content: FrameContent,
    size: Size<DevicePixels>,
    timestamp: Duration,
}
impl VideoFrame {
    /// Construct a portable frame. Timestamp is source metadata, not a scheduling
    /// deadline. Content must describe immutable pixels at the supplied source size.
    pub fn new(
        size: Size<DevicePixels>,
        timestamp: Duration,
        content: impl Into<gpui::content::ContentDraw>,
    ) -> Result<Self> {
        ensure!(
            size.width.0 > 0
                && size.height.0 > 0
                && size.width.0 <= 16384
                && size.height.0 <= 16384,
            "video dimensions must be 1..16384"
        );
        let mut draw = content.into();
        let request = gpui::content::ContentRequest::new(
            size.map(|v| gpui::px(v.0 as f32)),
            gpui::content::ImageExtent::try_from(size)?,
            1.,
        )?;
        draw.content = gpui::content::Content::new(FrameSource {
            content: draw.content,
            request,
        });
        Ok(Self {
            content: FrameContent::Portable(draw),
            size,
            timestamp,
        })
    }
    /// Portable imported frame; every placement shares its immutable acquisition
    /// identity while retaining independent geometry and presentation history.
    pub fn external(image: gpui::content::ExternalImage, timestamp: Duration) -> Self {
        let extent = image.descriptor().extent();
        Self {
            size: gpui::size(
                DevicePixels(i32::try_from(extent.width()).expect("validated extent")),
                DevicePixels(i32::try_from(extent.height()).expect("validated extent")),
            ),
            timestamp,
            content: FrameContent::Portable(
                gpui::content::ContentDraw::new(gpui::content::Content::external(image))
                    .color_output(gpui::content::ContentColorOutput::SrgbPreview),
            ),
        }
    }
    #[cfg(target_os = "macos")]
    pub fn core_video(
        frame: gpui_apple::custom_draw::core_video::CoreVideoFrame,
        timestamp: Duration,
    ) -> Self {
        Self::external(
            frame
                .external_image()
                .expect("validated CoreVideo descriptor"),
            timestamp,
        )
    }
    #[cfg(target_os = "windows")]
    pub fn dxgi(frame: gpui_windows::custom_draw::dxgi::DxgiFrame, timestamp: Duration) -> Self {
        Self {
            size: frame.size(),
            content: FrameContent::Native(Arc::new(frame)),
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
            content: FrameContent::Native(Arc::new(frame)),
            timestamp,
        }
    }
    /// Replace placement adjustments without changing frame/source identity.
    pub fn adjusted(&self, adjustments: crate::pipeline::ImageAdjustments) -> Self {
        let content = match &self.content {
            FrameContent::Portable(draw) => FrameContent::Portable(draw.clone().color_transform(
                gpui::content::ImageColorTransform::linear(adjustments.paint()),
            )),
            #[cfg(any(target_os = "windows", all(unix, feature = "wgpu")))]
            FrameContent::Native(source) => FrameContent::Native(Arc::new(AdjustedFrame {
                source: source.clone(),
                adjustments,
            })),
        };
        Self {
            content,
            size: self.size,
            timestamp: self.timestamp,
        }
    }
    pub fn size(&self) -> Size<DevicePixels> {
        self.size
    }
    pub fn timestamp(&self) -> Duration {
        self.timestamp
    }
    /// Source-sized scene bridge. UI callers use video/video_frame with a stable
    /// element ID so pending frames retain the placement's previous valid output.
    pub fn content(&self) -> Arc<dyn CustomContent> {
        match &self.content {
            FrameContent::Portable(draw) => draw.clone().into_scene_content(),
            #[cfg(any(target_os = "windows", all(unix, feature = "wgpu")))]
            FrameContent::Native(content) => content.clone(),
        }
    }
    pub fn paint(&self, window: &mut Window, bounds: Bounds<Pixels>, radii: Corners<Pixels>) {
        self.paint_placed(window, bounds, radii, None);
    }
    fn paint_placed(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radii: Corners<Pixels>,
        slot: Option<gpui::content::PresentationSlotId>,
    ) {
        if bounds.is_empty()
            || bounds.intersect(&window.content_mask().bounds).is_empty()
            || window.element_opacity() <= 0.
        {
            return;
        }
        let geometry = CustomGeometry::RoundedRect {
            bounds,
            corner_radii: radii,
        };
        match &self.content {
            FrameContent::Portable(draw) => {
                let draw = slot.map_or_else(|| draw.clone(), |slot| draw.clone().slot(slot));
                window.paint_content(geometry, draw);
            }
            #[cfg(any(target_os = "windows", all(unix, feature = "wgpu")))]
            FrameContent::Native(content) => window.paint_custom(geometry, content.clone()),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoStats {
    pub published: u64,
    /// Pending frames superseded before any presentation snapshot observed them.
    pub replaced: u64,
    /// CPU snapshots adopted by consumers, not GPU presentation acknowledgements.
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
/// A distinct stable ID per placement preserves its last-ready frame during GPU
/// work. Paused/closed, fully clipped and zero-opacity elements do not poll the
/// producer; renderer completion may still schedule a final paced redraw.
pub fn video(id: impl Into<ElementId>, stream: VideoStream) -> VideoView {
    VideoView {
        id: id.into(),
        stream: Some(stream),
        frame: None,
        style: StyleRefinement::default(),
        adjustments: None,
        blur: None,
    }
}
/// Paint one immutable snapshot without requesting live presentation frames.
/// Hosts can reuse a snapshot across placements with distinct stable IDs.
pub fn video_frame(id: impl Into<ElementId>, frame: VideoFrame) -> VideoView {
    VideoView {
        id: id.into(),
        stream: None,
        frame: Some(frame),
        style: StyleRefinement::default(),
        adjustments: None,
        blur: None,
    }
}
pub struct VideoView {
    id: ElementId,
    adjustments: Option<crate::pipeline::ImageAdjustments>,
    blur: Option<crate::pipeline::ImageBlur>,
    stream: Option<VideoStream>,
    frame: Option<VideoFrame>,
    style: StyleRefinement,
}
impl VideoView {
    /// Apply linear working-color adjustments after native format conversion.
    pub fn adjustments(mut self, adjustments: crate::pipeline::ImageAdjustments) -> Self {
        self.adjustments = Some(adjustments);
        self
    }
    /// Blur the completed sRGB preview using shared retained layer resources.
    /// Oversized captures fall back to the adjusted frame. Use ImageBlur::paint
    /// directly to receive an explicit bounds error instead.
    pub fn blur(mut self, blur: crate::pipeline::ImageBlur) -> Self {
        self.blur = Some(blur);
        self
    }
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
    type PrepaintState = gpui::content::PresentationSlotId;
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
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
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Style,
        window: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        window.with_element_state(
            global_id.expect("video has a stable element ID"),
            |state: Option<gpui::content::PresentationSlotId>, _| {
                let slot = state.unwrap_or_default();
                (slot, slot)
            },
        )
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Style,
        slot: &mut Self::PrepaintState,
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
                    let frame = self
                        .adjustments
                        .map_or_else(|| frame.clone(), |a| frame.adjusted(a));
                    if let Some(blur) = &self.blur {
                        // Fall back to the adjusted unblurred frame for oversized styled views.
                        if blur
                            .paint(bounds, window, |window| {
                                frame.paint_placed(window, bounds, radii, Some(*slot))
                            })
                            .is_err()
                        {
                            frame.paint_placed(window, bounds, radii, Some(*slot));
                        }
                    } else {
                        frame.paint_placed(window, bounds, radii, Some(*slot));
                    }
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
    impl gpui::content::ContentSource for Empty {
        fn describe(
            &self,
            cx: &mut gpui::content::ContentBuilder,
            request: &gpui::content::ContentRequest,
        ) -> std::result::Result<gpui::content::ImageRef, gpui::content::ContentError> {
            cx.solid(request.extent(), gpui::content::ColorValue::srgb([1.; 4])?)
        }
    }
    #[derive(Debug)]
    struct Held(Arc<Empty>);
    impl gpui::content::ContentSource for Held {
        fn describe(
            &self,
            cx: &mut gpui::content::ContentBuilder,
            request: &gpui::content::ContentRequest,
        ) -> std::result::Result<gpui::content::ImageRef, gpui::content::ContentError> {
            self.0.describe(cx, request)
        }
    }
    fn frame(n: u64) -> VideoFrame {
        VideoFrame::new(
            gpui::size(DevicePixels(16), DevicePixels(9)),
            Duration::from_millis(n),
            gpui::content::Content::new(Empty),
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
                gpui::content::Content::new(Empty)
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
                    gpui::content::Content::new(Held(content.clone())),
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
    #[test]
    fn portable_frames_keep_natural_extent_and_adjustments_keep_source_identity() -> Result<()> {
        use gpui::content::*;
        let frame = VideoFrame::new(
            gpui::size(DevicePixels(16), DevicePixels(9)),
            Duration::from_millis(17),
            Content::solid(ColorValue::srgb([1., 0., 0., 1.])?),
        )?;
        let adjusted = frame.adjusted(crate::pipeline::ImageAdjustments::new(
            crate::pipeline::ColorMatrix::identity(crate::pipeline::ImagePrimaries::Srgb)
                .exposure(1.)?,
            None,
        )?);
        let mut keys = Vec::new();
        for side in [32, 128] {
            let request = gpui::PreparationRequest::new(
                gpui::size(gpui::px(side as f32), gpui::px(side as f32)),
                gpui::size(DevicePixels(side), DevicePixels(side)),
                1.,
            )?;
            let plain = frame.content().portable_content(&request)?.unwrap();
            let adjusted = adjusted.content().portable_content(&request)?.unwrap();
            assert_eq!(plain.draw.content.id(), adjusted.draw.content.id());
            assert!(adjusted.draw.color_transform.linear_paint().is_some());
            let graph = ContentGraph::describe(
                &[ContentRoot {
                    content: plain.draw.content,
                    request: plain.request,
                }],
                ContentCapabilities::default(),
                GraphLimits::default(),
            )?;
            assert_eq!(graph.nodes()[0].outputs()[0].byte_len(), 16 * 9 * 4);
            keys.push(graph.nodes()[0].output_key(0).unwrap());
        }
        assert_eq!(keys[0], keys[1]);
        assert_eq!(frame.size(), adjusted.size());
        assert_eq!(frame.timestamp(), adjusted.timestamp());
        Ok(())
    }
    #[test]
    fn portable_frame_description_rejects_mismatched_source_pixels() -> Result<()> {
        use gpui::content::*;
        let pixels = ImagePixels::new(
            ImageDescriptor::new(
                ImageExtent::new(1, 1)?,
                PixelFormat::Rgba8,
                ImageInterpretation {
                    color: ColorEncoding::Srgb,
                    alpha: AlphaMode::Straight,
                },
                1,
            )?,
            Arc::from([255; 4]),
            4,
        )?;
        let frame = VideoFrame::new(
            gpui::size(DevicePixels(16), DevicePixels(9)),
            Duration::ZERO,
            Content::image(ImageAsset::ready(pixels)),
        )?;
        let request = gpui::PreparationRequest::new(
            gpui::size(gpui::px(32.), gpui::px(32.)),
            gpui::size(DevicePixels(32), DevicePixels(32)),
            1.,
        )?;
        let draw = frame.content().portable_content(&request)?.unwrap();
        let error = ContentGraph::describe(
            &[ContentRoot {
                content: draw.draw.content,
                request: draw.request,
            }],
            ContentCapabilities::default(),
            GraphLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind, ContentErrorKind::InvalidDescriptor);
        Ok(())
    }
}
