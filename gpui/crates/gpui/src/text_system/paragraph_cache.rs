use super::*;
use crate::{ParagraphRequest, PlatformParagraphLayout, PlatformParagraphShape};

#[derive(Clone, Default)]
pub(super) struct ParagraphIndex {
    shapes: usize,
    layouts: usize,
}

#[derive(Default)]
pub(super) struct ParagraphFrameCache {
    shapes: FxHashMap<Arc<CacheKey>, Arc<dyn PlatformParagraphShape>>,
    layouts: FxHashMap<Arc<CacheKey>, Arc<dyn PlatformParagraphLayout>>,
    used_shapes: Vec<Arc<CacheKey>>,
    used_layouts: Vec<Arc<CacheKey>>,
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
        width: Pixels,
    ) -> anyhow::Result<Arc<dyn PlatformParagraphLayout>> {
        anyhow::ensure!(
            f32::from(font_size).is_finite() && font_size > px(0.0),
            "invalid font size"
        );
        anyhow::ensure!(
            f32::from(width).is_finite() && width >= px(0.0),
            "invalid paragraph width"
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
        // These are the complete supported parameters: auto direction/start alignment.
        let shape_ref = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width: None,
            force_width: None,
        } as &dyn AsCacheKeyRef;
        let layout_ref = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width: Some(width),
            force_width: None,
        } as &dyn AsCacheKeyRef;
        loop {
            let generation = self.clear_if_font_generation_changed();
            // Keep previous-frame entries until normal frame eviction. A failed
            // backend/reflow request must not consume reusable cached results.
            let (shape, layout) = {
                let current = self.current_frame.read();
                let previous = self.previous_frame.lock();
                let shape = current
                    .paragraphs
                    .shapes
                    .get_key_value(shape_ref)
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .or_else(|| {
                        previous
                            .paragraphs
                            .shapes
                            .get_key_value(shape_ref)
                            .map(|(key, value)| (key.clone(), value.clone()))
                    });
                let layout = current
                    .paragraphs
                    .layouts
                    .get_key_value(layout_ref)
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .or_else(|| {
                        previous
                            .paragraphs
                            .layouts
                            .get_key_value(layout_ref)
                            .map(|(key, value)| (key.clone(), value.clone()))
                    });
                (shape, layout)
            };
            // Neither backend shaping nor reflow executes under a GPUI cache lock.
            // Owned cache keys are created only on misses, not repeated requests.
            let (shape_key, shape) = match shape {
                Some(entry) => entry,
                None => {
                    let shape = self
                        .platform_text_system
                        .shape_paragraph(ParagraphRequest {
                            text: text.as_ref(),
                            font_size,
                            runs,
                        })?;
                    (
                        Arc::new(CacheKey {
                            text: text.clone(),
                            font_size,
                            runs: SmallVec::from(runs),
                            wrap_width: None,
                            force_width: None,
                        }),
                        shape,
                    )
                }
            };
            let (layout_key, layout) = match layout {
                Some(entry) => entry,
                None => {
                    let layout = shape.layout(width)?;
                    (
                        Arc::new(CacheKey {
                            text: text.clone(),
                            font_size,
                            runs: SmallVec::from(runs),
                            wrap_width: Some(width),
                            force_width: None,
                        }),
                        layout,
                    )
                }
            };
            let mut current = self.current_frame.write();
            // Do not publish a result built across a concurrent font registration.
            if self.font_generation.load(Ordering::Acquire) != generation
                || self.cached_font_generation.load(Ordering::Acquire) != generation
            {
                continue;
            }
            current
                .paragraphs
                .shapes
                .entry(shape_key.clone())
                .or_insert(shape);
            let layout = current
                .paragraphs
                .layouts
                .entry(layout_key.clone())
                .or_insert(layout)
                .clone();
            // Record hits too, so retained-element reuse ranges retain both levels.
            current.paragraphs.used_shapes.push(shape_key.clone());
            current.paragraphs.used_layouts.push(layout_key.clone());
            return Ok(layout);
        }
    }
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
        let shape_key = Arc::new(CacheKey {
            text: "probe".into(),
            font_size: px(16.0),
            runs: SmallVec::new(),
            wrap_width: None,
            force_width: None,
        });
        let layout_key = Arc::new(CacheKey {
            text: "probe".into(),
            font_size: px(16.0),
            runs: SmallVec::new(),
            wrap_width: Some(px(100.0)),
            force_width: None,
        });
        let layout: Arc<dyn PlatformParagraphLayout> = Arc::new(Probe);
        {
            let mut frame = cache.current_frame.write();
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
        assert_eq!(cache.current_frame.read().paragraphs.index().layouts, 0);
        cache
            .current_frame
            .write()
            .paragraphs
            .used_shapes
            .push(shape_key);
        cache
            .current_frame
            .write()
            .paragraphs
            .used_layouts
            .push(layout_key);
        cache.finish_frame();
        cache.reuse_layouts(start.clone()..end.clone());
        let cached = cache
            .layout_paragraph("probe".into(), px(16.0), &[], px(100.0))
            .unwrap();
        assert!(Arc::ptr_eq(&layout, &cached));
        cache.finish_frame();
        let cached = cache
            .layout_paragraph("probe".into(), px(16.0), &[], px(100.0))
            .unwrap();
        assert!(Arc::ptr_eq(&layout, &cached));
        generation.fetch_add(1, Ordering::AcqRel);
        cache.reuse_layouts(start..end); // Old ranges must never resurrect old layouts.
        assert!(cache.current_frame.read().paragraphs.layouts.is_empty());
        assert!(
            cache
                .layout_paragraph("probe".into(), px(16.0), &[], px(100.0))
                .is_err()
        );
    }
    #[test]
    fn failed_reflow_preserves_previous_frame_shaping() {
        let cache = LineLayoutCache::new(Arc::new(NoopTextSystem), Arc::new(AtomicUsize::new(0)));
        let key = Arc::new(CacheKey {
            text: "probe".into(),
            font_size: px(16.0),
            runs: SmallVec::new(),
            wrap_width: None,
            force_width: None,
        });
        cache
            .current_frame
            .write()
            .paragraphs
            .shapes
            .insert(key, Arc::new(Probe));
        cache.finish_frame();
        assert!(
            cache
                .layout_paragraph("probe".into(), px(16.0), &[], px(200.0))
                .is_err()
        );
        // Noop cannot rebuild this shape: success requires retaining the cached one.
        assert!(
            cache
                .layout_paragraph("probe".into(), px(16.0), &[], px(100.0))
                .is_ok()
        );
    }

    #[test]
    fn reflow_runs_without_locks_and_does_not_publish_across_font_registration() {
        struct ChangingShape {
            cache: std::sync::Weak<LineLayoutCache>,
            generation: Arc<AtomicUsize>,
        }
        impl PlatformParagraphShape for ChangingShape {
            fn layout(&self, _: Pixels) -> anyhow::Result<Arc<dyn PlatformParagraphLayout>> {
                let cache = self.cache.upgrade().unwrap();
                assert!(cache.current_frame.try_write().is_some());
                assert!(cache.previous_frame.try_lock().is_some());
                self.generation.fetch_add(1, Ordering::AcqRel);
                Ok(Arc::new(Probe))
            }
        }
        let generation = Arc::new(AtomicUsize::new(0));
        let cache = Arc::new(LineLayoutCache::new(
            Arc::new(NoopTextSystem),
            generation.clone(),
        ));
        let key = Arc::new(CacheKey {
            text: "probe".into(),
            font_size: px(16.0),
            runs: SmallVec::new(),
            wrap_width: None,
            force_width: None,
        });
        cache.current_frame.write().paragraphs.shapes.insert(
            key,
            Arc::new(ChangingShape {
                cache: Arc::downgrade(&cache),
                generation,
            }),
        );
        // The first reflow crosses a generation; retry reaches Noop's Unsupported
        // hook. Its obsolete successful result must never enter either frame map.
        assert!(
            cache
                .layout_paragraph("probe".into(), px(16.0), &[], px(100.0))
                .is_err()
        );
        assert!(cache.current_frame.read().paragraphs.layouts.is_empty());
        assert!(cache.previous_frame.lock().paragraphs.layouts.is_empty());
    }
}
