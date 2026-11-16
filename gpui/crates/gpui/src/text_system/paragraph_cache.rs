use super::*;
use crate::{
    ParagraphRequest, ParagraphShapeOptions, PlatformParagraphLayout, PlatformParagraphShape,
    TextDirection,
};

/// Paragraph cache key. Separate from the flat-line `CacheKey` so paragraph
/// options never touch upstream line-layout code. `width` is `None` for shapes.
#[derive(Clone, Debug, Eq)]
pub(super) struct ParagraphKey {
    text: SharedString,
    font_size: Pixels,
    runs: SmallVec<[FontRun; 1]>,
    options: ParagraphShapeOptions,
    width: Option<Pixels>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct ParagraphKeyRef<'a> {
    text: &'a str,
    font_size: Pixels,
    runs: &'a [FontRun],
    direction: TextDirection,
    language: Option<&'a str>,
    width: Option<Pixels>,
}

trait AsParagraphKeyRef {
    fn key_ref(&self) -> ParagraphKeyRef<'_>;
}

impl ParagraphKey {
    fn new(
        text: &SharedString,
        font_size: Pixels,
        runs: &[FontRun],
        options: &ParagraphShapeOptions,
        width: Option<Pixels>,
    ) -> Arc<Self> {
        Arc::new(Self {
            text: text.clone(),
            font_size,
            runs: SmallVec::from(runs),
            options: options.clone(),
            width,
        })
    }
}

impl<'a> ParagraphKeyRef<'a> {
    fn new(
        text: &'a str,
        font_size: Pixels,
        runs: &'a [FontRun],
        options: &'a ParagraphShapeOptions,
        width: Option<Pixels>,
    ) -> Self {
        Self {
            text,
            font_size,
            runs,
            direction: options.direction,
            language: options.language.as_deref(),
            width,
        }
    }
}

impl AsParagraphKeyRef for ParagraphKey {
    fn key_ref(&self) -> ParagraphKeyRef<'_> {
        ParagraphKeyRef::new(
            &self.text,
            self.font_size,
            &self.runs,
            &self.options,
            self.width,
        )
    }
}

impl AsParagraphKeyRef for ParagraphKeyRef<'_> {
    fn key_ref(&self) -> ParagraphKeyRef<'_> {
        *self
    }
}

impl PartialEq for dyn AsParagraphKeyRef + '_ {
    fn eq(&self, other: &dyn AsParagraphKeyRef) -> bool {
        self.key_ref() == other.key_ref()
    }
}

impl Eq for dyn AsParagraphKeyRef + '_ {}

impl Hash for dyn AsParagraphKeyRef + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key_ref().hash(state)
    }
}

impl PartialEq for ParagraphKey {
    fn eq(&self, other: &Self) -> bool {
        self.key_ref() == other.key_ref()
    }
}

impl Hash for ParagraphKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key_ref().hash(state)
    }
}

impl<'a> Borrow<dyn AsParagraphKeyRef + 'a> for Arc<ParagraphKey> {
    fn borrow(&self) -> &(dyn AsParagraphKeyRef + 'a) {
        self.as_ref() as &dyn AsParagraphKeyRef
    }
}

#[derive(Clone, Default)]
pub(super) struct ParagraphIndex {
    shapes: usize,
    layouts: usize,
}

#[derive(Default)]
pub(super) struct ParagraphFrameCache {
    shapes: FxHashMap<Arc<ParagraphKey>, Arc<dyn PlatformParagraphShape>>,
    layouts: FxHashMap<Arc<ParagraphKey>, Arc<dyn PlatformParagraphLayout>>,
    used_shapes: Vec<Arc<ParagraphKey>>,
    used_layouts: Vec<Arc<ParagraphKey>>,
}

impl ParagraphFrameCache {
    pub fn index(&self) -> ParagraphIndex {
        ParagraphIndex {
            shapes: self.used_shapes.len(),
            layouts: self.used_layouts.len(),
        }
    }
    pub fn truncate(&mut self, index: ParagraphIndex) {
        self.used_shapes.truncate(index.shapes);
        self.used_layouts.truncate(index.layouts);
    }
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.layouts.clear();
        self.used_shapes.clear();
        self.used_layouts.clear();
    }
    pub fn reuse_from(&mut self, previous: &mut Self, range: Range<ParagraphIndex>) {
        for key in &previous.used_shapes[range.start.shapes..range.end.shapes] {
            if let Some((key, shape)) = previous.shapes.remove_entry(key) {
                self.shapes.insert(key, shape);
            }
            self.used_shapes.push(key.clone());
        }
        for key in &previous.used_layouts[range.start.layouts..range.end.layouts] {
            if let Some((key, layout)) = previous.layouts.remove_entry(key) {
                self.layouts.insert(key, layout);
            }
            self.used_layouts.push(key.clone());
        }
    }
}

impl LineLayoutCache {
    pub fn layout_paragraph(
        &self,
        text: SharedString,
        font_size: Pixels,
        runs: &[FontRun],
        options: &ParagraphShapeOptions,
        width: Pixels,
    ) -> anyhow::Result<Arc<dyn PlatformParagraphLayout>> {
        validate_paragraph_request(&text, font_size, runs)?;
        anyhow::ensure!(
            f32::from(width).is_finite() && width >= px(0.0),
            "invalid paragraph width"
        );
        let layout_ref = &ParagraphKeyRef::new(&text, font_size, runs, options, Some(width))
            as &dyn AsParagraphKeyRef;
        loop {
            let generation = self.clear_if_font_generation_changed();
            // Keep previous-frame entries until normal frame eviction. A failed
            // backend/reflow request must not consume reusable cached results.
            let layout = {
                let state = self.state.borrow();
                state
                    .current_frame
                    .paragraphs
                    .layouts
                    .get_key_value(layout_ref)
                    .or_else(|| {
                        state
                            .previous_frame
                            .paragraphs
                            .layouts
                            .get_key_value(layout_ref)
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
            };
            // Neither backend shaping nor reflow executes under a GPUI cache lock.
            // Owned cache keys are created only on misses, not repeated requests.
            let (shape_key, shape) = self.paragraph_shape(&text, font_size, runs, options)?;
            let (layout_key, layout) = match layout {
                Some(entry) => entry,
                None => (
                    ParagraphKey::new(&text, font_size, runs, options, Some(width)),
                    shape.layout(width)?,
                ),
            };
            let mut state = self.state.borrow_mut();
            // Do not publish a result built across a concurrent font registration.
            if !self.generation_is_current(generation) {
                continue;
            }
            let current = &mut state.current_frame;
            Self::publish_paragraph_shape(current, shape_key, shape);
            let layout = current
                .paragraphs
                .layouts
                .entry(layout_key.clone())
                .or_insert(layout)
                .clone();
            // Record hits too, so retained-element reuse ranges retain both levels.
            current.paragraphs.used_layouts.push(layout_key);
            return Ok(layout);
        }
    }

    /// Min-content and max-content widths of one paragraph, from the same
    /// cached shape `layout_paragraph` reflows.
    pub fn paragraph_content_widths(
        &self,
        text: SharedString,
        font_size: Pixels,
        runs: &[FontRun],
        options: &ParagraphShapeOptions,
    ) -> anyhow::Result<(Pixels, Pixels)> {
        validate_paragraph_request(&text, font_size, runs)?;
        loop {
            let generation = self.clear_if_font_generation_changed();
            let (shape_key, shape) = self.paragraph_shape(&text, font_size, runs, options)?;
            let widths = shape.content_widths()?;
            let mut state = self.state.borrow_mut();
            if !self.generation_is_current(generation) {
                continue;
            }
            Self::publish_paragraph_shape(&mut state.current_frame, shape_key, shape);
            return Ok(widths);
        }
    }

    /// Cached shape for this request (current, then previous frame), or a new
    /// backend shape. Not yet published; the caller checks the font generation.
    fn paragraph_shape(
        &self,
        text: &SharedString,
        font_size: Pixels,
        runs: &[FontRun],
        options: &ParagraphShapeOptions,
    ) -> anyhow::Result<(Arc<ParagraphKey>, Arc<dyn PlatformParagraphShape>)> {
        let shape_ref =
            &ParagraphKeyRef::new(text, font_size, runs, options, None) as &dyn AsParagraphKeyRef;
        let cached = {
            let state = self.state.borrow();
            state
                .current_frame
                .paragraphs
                .shapes
                .get_key_value(shape_ref)
                .or_else(|| {
                    state
                        .previous_frame
                        .paragraphs
                        .shapes
                        .get_key_value(shape_ref)
                })
                .map(|(key, value)| (key.clone(), value.clone()))
        };
        if let Some(entry) = cached {
            return Ok(entry);
        }
        let shape = self
            .platform_text_system
            .shape_paragraph(ParagraphRequest::new(text, font_size, runs, options))?;
        Ok((
            ParagraphKey::new(text, font_size, runs, options, None),
            shape,
        ))
    }

    fn generation_is_current(&self, generation: usize) -> bool {
        self.font_generation.load(Ordering::Acquire) == generation
            && self.cached_font_generation.load(Ordering::Acquire) == generation
    }

    /// Publish a shape to the current frame. Records hits too, so retained-element
    /// reuse ranges keep it.
    fn publish_paragraph_shape(
        current: &mut FrameCache,
        key: Arc<ParagraphKey>,
        shape: Arc<dyn PlatformParagraphShape>,
    ) {
        current
            .paragraphs
            .shapes
            .entry(key.clone())
            .or_insert(shape);
        current.paragraphs.used_shapes.push(key);
    }
}

fn validate_paragraph_request(
    text: &str,
    font_size: Pixels,
    runs: &[FontRun],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        f32::from(font_size).is_finite() && font_size > px(0.0),
        "invalid font size"
    );
    let mut end = 0usize;
    for run in runs {
        end = end
            .checked_add(run.len)
            .ok_or_else(|| anyhow::anyhow!("font run overflow"))?;
        anyhow::ensure!(
            text.is_char_boundary(end),
            "invalid paragraph font run range"
        );
    }
    anyhow::ensure!(
        runs.is_empty() || end == text.len(),
        "font runs must cover paragraph"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, NoopTextSystem, ParagraphPosition, Point, Size, VisualDirection, size};

    struct Probe;
    impl PlatformParagraphShape for Probe {
        fn layout(&self, width: Pixels) -> anyhow::Result<Arc<dyn PlatformParagraphLayout>> {
            anyhow::ensure!(width != px(200.0), "unsupported probe reflow");
            Ok(Arc::new(Probe))
        }
    }
    impl PlatformParagraphLayout for Probe {
        fn source(&self) -> &str {
            "probe"
        }
        fn font_size(&self) -> Pixels {
            px(16.0)
        }
        fn size(&self) -> Size<Pixels> {
            size(px(100.0), px(20.0))
        }
        fn runs(&self) -> &[crate::ShapedRun] {
            &[]
        }
        fn hit_test(&self, _: Point<Pixels>) -> anyhow::Result<ParagraphPosition> {
            anyhow::bail!("unused")
        }
        fn caret_bounds(&self, _: ParagraphPosition) -> anyhow::Result<Bounds<Pixels>> {
            anyhow::bail!("unused")
        }
        fn move_visual(
            &self,
            _: ParagraphPosition,
            _: VisualDirection,
        ) -> anyhow::Result<ParagraphPosition> {
            anyhow::bail!("unused")
        }
        fn selection_bounds(
            &self,
            _: ParagraphPosition,
            _: ParagraphPosition,
        ) -> anyhow::Result<Vec<Bounds<Pixels>>> {
            anyhow::bail!("unused")
        }
    }

    #[test]
    fn paragraph_frame_ranges_retain_both_levels_and_obey_generation() {
        let generation = Arc::new(AtomicUsize::new(0));
        let cache = LineLayoutCache::new(Arc::new(NoopTextSystem), generation.clone());
        let start = cache.layout_index();
        let shape_key =
            ParagraphKey::new(&"probe".into(), px(16.0), &[], &Default::default(), None);
        let layout_key = ParagraphKey::new(
            &"probe".into(),
            px(16.0),
            &[],
            &Default::default(),
            Some(px(100.0)),
        );
        let layout: Arc<dyn PlatformParagraphLayout> = Arc::new(Probe);
        {
            let mut state = cache.state.borrow_mut();
            let frame = &mut state.current_frame;
            frame
                .paragraphs
                .shapes
                .insert(shape_key.clone(), Arc::new(Probe));
            frame
                .paragraphs
                .layouts
                .insert(layout_key.clone(), layout.clone());
            frame.paragraphs.used_shapes.push(shape_key.clone());
            frame.paragraphs.used_layouts.push(layout_key.clone());
        }
        let end = cache.layout_index();
        // A discarded speculative paint range does not become a reusable range.
        cache.truncate_layouts(start.clone());
        assert_eq!(
            cache
                .state
                .borrow()
                .current_frame
                .paragraphs
                .index()
                .layouts,
            0
        );
        cache
            .state
            .borrow_mut()
            .current_frame
            .paragraphs
            .used_shapes
            .push(shape_key);
        cache
            .state
            .borrow_mut()
            .current_frame
            .paragraphs
            .used_layouts
            .push(layout_key);
        cache.finish_frame();
        cache.reuse_layouts(start.clone()..end.clone());
        let cached = cache
            .layout_paragraph(
                "probe".into(),
                px(16.0),
                &[],
                &Default::default(),
                px(100.0),
            )
            .unwrap();
        assert!(Arc::ptr_eq(&layout, &cached));
        cache.finish_frame();
        let cached = cache
            .layout_paragraph(
                "probe".into(),
                px(16.0),
                &[],
                &Default::default(),
                px(100.0),
            )
            .unwrap();
        assert!(Arc::ptr_eq(&layout, &cached));
        generation.fetch_add(1, Ordering::AcqRel);
        cache.reuse_layouts(start..end); // Old ranges must never resurrect old layouts.
        assert!(
            cache
                .state
                .borrow()
                .current_frame
                .paragraphs
                .layouts
                .is_empty()
        );
        assert!(
            cache
                .layout_paragraph(
                    "probe".into(),
                    px(16.0),
                    &[],
                    &Default::default(),
                    px(100.0)
                )
                .is_err()
        );
    }
    #[test]
    fn content_widths_reuse_the_cached_shape_and_default_to_an_error() {
        struct Measured;
        impl PlatformParagraphShape for Measured {
            fn layout(&self, _: Pixels) -> anyhow::Result<Arc<dyn PlatformParagraphLayout>> {
                Ok(Arc::new(Probe))
            }
            fn content_widths(&self) -> anyhow::Result<(Pixels, Pixels)> {
                Ok((px(30.0), px(90.0)))
            }
        }
        let cache = LineLayoutCache::new(Arc::new(NoopTextSystem), Arc::new(AtomicUsize::new(0)));
        let key = |text: &'static str| {
            ParagraphKey::new(&text.into(), px(16.0), &[], &Default::default(), None)
        };
        {
            let mut state = cache.state.borrow_mut();
            let shapes = &mut state.current_frame.paragraphs.shapes;
            shapes.insert(key("measured"), Arc::new(Measured));
            shapes.insert(key("probe"), Arc::new(Probe));
        }
        cache.finish_frame();
        // Noop cannot shape: success means the previous frame's shape was reused.
        assert_eq!(
            cache
                .paragraph_content_widths("measured".into(), px(16.0), &[], &Default::default())
                .unwrap(),
            (px(30.0), px(90.0))
        );
        {
            let state = cache.state.borrow();
            let current = &state.current_frame.paragraphs;
            assert_eq!(current.shapes.len(), 1);
            assert_eq!(current.used_shapes.len(), 1);
        }
        assert!(
            cache
                .paragraph_content_widths("probe".into(), px(16.0), &[], &Default::default())
                .is_err()
        );
        assert!(
            cache
                .paragraph_content_widths("unshaped".into(), px(16.0), &[], &Default::default())
                .is_err()
        );
        assert!(
            cache
                .paragraph_content_widths("measured".into(), px(0.0), &[], &Default::default())
                .is_err()
        );
    }

    #[test]
    fn shape_options_are_part_of_the_cache_key() {
        let cache = LineLayoutCache::new(Arc::new(NoopTextSystem), Arc::new(AtomicUsize::new(0)));
        let default = ParagraphShapeOptions::default();
        cache
            .state
            .borrow_mut()
            .current_frame
            .paragraphs
            .shapes
            .insert(
                ParagraphKey::new(&"probe".into(), px(16.0), &[], &default, None),
                Arc::new(Probe),
            );
        let layout = |options: &ParagraphShapeOptions| {
            cache.layout_paragraph("probe".into(), px(16.0), &[], options, px(100.0))
        };
        // Noop cannot shape, so only the cached default-options shape succeeds.
        assert!(layout(&default).is_ok());
        assert!(
            layout(&ParagraphShapeOptions::default().with_direction(TextDirection::Auto)).is_ok()
        );
        assert!(
            layout(&ParagraphShapeOptions::default().with_direction(TextDirection::RightToLeft))
                .is_err()
        );
        assert!(layout(&ParagraphShapeOptions::default().with_language("ar")).is_err());
        let a = ParagraphKey::new(
            &"x".into(),
            px(16.0),
            &[],
            &ParagraphShapeOptions::default().with_language("ar"),
            None,
        );
        let b = ParagraphKey::new(
            &"x".into(),
            px(16.0),
            &[],
            &ParagraphShapeOptions::default().with_language("he"),
            None,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn failed_reflow_preserves_previous_frame_shaping() {
        let cache = LineLayoutCache::new(Arc::new(NoopTextSystem), Arc::new(AtomicUsize::new(0)));
        let key = ParagraphKey::new(&"probe".into(), px(16.0), &[], &Default::default(), None);
        cache
            .state
            .borrow_mut()
            .current_frame
            .paragraphs
            .shapes
            .insert(key, Arc::new(Probe));
        cache.finish_frame();
        assert!(
            cache
                .layout_paragraph(
                    "probe".into(),
                    px(16.0),
                    &[],
                    &Default::default(),
                    px(200.0)
                )
                .is_err()
        );
        // Noop cannot rebuild this shape: success requires retaining the cached one.
        assert!(
            cache
                .layout_paragraph(
                    "probe".into(),
                    px(16.0),
                    &[],
                    &Default::default(),
                    px(100.0)
                )
                .is_ok()
        );
    }

    #[test]
    fn reflow_releases_cache_borrow_and_does_not_publish_across_font_registration() {
        thread_local! {
            static REFLOW_CACHE: std::cell::RefCell<std::rc::Weak<LineLayoutCache>> = Default::default();
        }
        struct ChangingShape {
            generation: Arc<AtomicUsize>,
        }
        impl PlatformParagraphShape for ChangingShape {
            fn layout(&self, _: Pixels) -> anyhow::Result<Arc<dyn PlatformParagraphLayout>> {
                REFLOW_CACHE.with(|slot| {
                    let cache = slot.borrow().upgrade().unwrap();
                    assert!(cache.state.try_borrow_mut().is_ok());
                });
                self.generation.fetch_add(1, Ordering::AcqRel);
                Ok(Arc::new(Probe))
            }
        }
        let generation = Arc::new(AtomicUsize::new(0));
        let cache = std::rc::Rc::new(LineLayoutCache::new(
            Arc::new(NoopTextSystem),
            generation.clone(),
        ));
        let key = ParagraphKey::new(&"probe".into(), px(16.0), &[], &Default::default(), None);
        REFLOW_CACHE.with(|slot| *slot.borrow_mut() = std::rc::Rc::downgrade(&cache));
        cache
            .state
            .borrow_mut()
            .current_frame
            .paragraphs
            .shapes
            .insert(key, Arc::new(ChangingShape { generation }));
        // The first reflow crosses a generation; retry reaches Noop's Unsupported
        // hook. Its obsolete successful result must never enter either frame map.
        assert!(
            cache
                .layout_paragraph(
                    "probe".into(),
                    px(16.0),
                    &[],
                    &Default::default(),
                    px(100.0)
                )
                .is_err()
        );
        assert!(
            cache
                .state
                .borrow()
                .current_frame
                .paragraphs
                .layouts
                .is_empty()
        );
        assert!(
            cache
                .state
                .borrow()
                .previous_frame
                .paragraphs
                .layouts
                .is_empty()
        );
    }
}
