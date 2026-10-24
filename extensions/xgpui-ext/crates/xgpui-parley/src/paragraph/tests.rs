use super::*;
use crate::{
    TextPosition, VisualDirection,
    test_support::{INTER, fonts},
};
use gpui::TextAffinity as Affinity;

fn backend() -> Result<ParleyTextSystem> {
    let mut fonts = fonts();
    for bytes in [
        include_bytes!("../../tests/fonts/NotoSansArabic.ttf").as_slice(),
        include_bytes!("../../tests/fonts/NotoSansHebrew.ttf").as_slice(),
    ] {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(bytes.to_vec())), None);
    }
    ParleyTextSystem::new(fonts, "IBM Plex Sans")
}
fn runs(backend: &ParleyTextSystem, text: &str) -> Result<[FontRun; 1]> {
    let mut font = gpui::font("IBM Plex Sans");
    font.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
        "Noto Sans Hebrew".into(),
        "Noto Sans Arabic".into(),
    ]));
    Ok([FontRun {
        len: text.len(),
        font_id: backend.font_id(&font)?,
    }])
}
fn at(byte_index: usize, affinity: Affinity) -> TextPosition {
    TextPosition {
        byte_index,
        affinity,
    }
}

#[test]
fn mixed_scripts_reflow_without_reshaping_or_mutating_prior_results() -> Result<()> {
    let backend = backend()?;
    let text = "office café אבג 123 مرحبا e\u{301} words wrap around more words";
    let shape = backend.prepare_paragraph(text, px(24.0), &runs(&backend, text)?)?;
    let calls = backend.state.lock().unwrap().shape_calls;
    let wide = shape.layout(px(1200.0))?;
    let narrow = shape.layout(px(180.0))?;
    assert_eq!(wide.parley_layout().len(), 1);
    assert!(narrow.parley_layout().len() > 1);
    assert_eq!(backend.state.lock().unwrap().shape_calls, calls);
    assert_eq!(narrow.source(), text);
    assert!(narrow.resolved_faces().len() >= 3);
    assert_eq!(wide.resolved_faces(), narrow.resolved_faces());
    let mut expected_faces = Vec::new();
    for line in wide.parley_layout().lines() {
        for run in line.runs() {
            let id = backend.faces.write().unwrap().register(&run)?;
            if !expected_faces.contains(&id) {
                expected_faces.push(id);
            }
        }
    }
    assert_eq!(
        narrow.resolved_faces(),
        expected_faces,
        "preserve first-seen face order"
    );
    let mut rtl = false;
    let mut covered = 0;
    for line in narrow.parley_layout().lines() {
        assert_eq!(line.text_range().start, covered);
        covered = line.text_range().end;
        assert!(line.metrics().advance <= 180.01);
        for run in line.runs() {
            rtl |= run.is_rtl();
            for cluster in run.visual_clusters() {
                assert!(text.is_char_boundary(cluster.text_range().start));
                assert!(cluster.glyphs().all(|g| g.id != 0));
            }
        }
    }
    assert!(rtl);
    assert_eq!(covered, text.len());
    let selection = narrow.selection_bounds(
        at(0, Affinity::Downstream),
        at(text.len(), Affinity::Upstream),
    )?;
    assert!(selection.len() >= narrow.parley_layout().len());
    assert!(
        selection
            .iter()
            .all(|b| b.size.width >= px(0.0) && b.size.height > px(0.0))
    );
    backend.add_fonts(vec![Cow::Borrowed(INTER)])?;
    // Old prepared data reflows with old retained faces after font generation changes.
    let old = shape.layout(px(180.0))?;
    assert_eq!(old.resolved_faces(), narrow.resolved_faces());
    for &id in old.resolved_faces() {
        assert!(backend.font_metrics(id).units_per_em > 0);
    }
    Ok(())
}

#[test]
fn bidi_affinity_visual_movement_and_discontiguous_selection() -> Result<()> {
    let backend = backend()?;
    let text = "abc אבג def";
    let layout = backend
        .prepare_paragraph(text, px(24.0), &runs(&backend, text)?)?
        .layout(px(500.0))?;
    let upstream = at(4, Affinity::Upstream);
    let downstream = at(4, Affinity::Downstream);
    let left = layout.caret_bounds(upstream)?;
    let right = layout.caret_bounds(downstream)?;
    assert!(right.origin.x > left.origin.x);
    // Selecting the Latin prefix and the first logical Hebrew letter leaves a gap.
    let rects = layout.selection_bounds(
        at(0, Affinity::Downstream),
        at("abc א".len(), Affinity::Upstream),
    )?;
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0].origin.y, rects[1].origin.y);
    assert!(rects[0].origin.x + rects[0].size.width < rects[1].origin.x);
    let moved = layout.move_visual(downstream, VisualDirection::Left)?;
    assert!(moved.byte_index > downstream.byte_index); // Visual left goes logically forward.
    assert!(layout.caret_bounds(moved)?.origin.x < right.origin.x);
    // Hit the center of the Hebrew glyph's right edge and preserve logical boundary.
    let hit = layout.hit_test(point(
        right.origin.x,
        right.origin.y + right.size.height / 2.0,
    ))?;
    assert_eq!(hit.byte_index, 4);
    assert_eq!(hit.affinity, Affinity::Downstream);
    let rtl_text = "אבג abc 123 مرحبا";
    let rtl = backend
        .prepare_paragraph(rtl_text, px(24.0), &runs(&backend, rtl_text)?)?
        .layout(px(180.0))?;
    assert!(rtl.parley_layout().is_rtl());
    assert!(rtl.parley_layout().len() > 1);
    let start = rtl.caret_bounds(at(0, Affinity::Downstream))?;
    assert!(start.origin.x > px(0.0));
    let hit = rtl.hit_test(point(
        start.origin.x,
        start.origin.y + start.size.height / 2.0,
    ))?;
    assert_eq!(hit.byte_index, 0);
    Ok(())
}

#[test]
fn paragraph_boundaries_empty_layout_and_invalid_geometry_are_explicit() -> Result<()> {
    let backend = backend()?;
    let shape = backend.prepare_paragraph("", px(24.0), &[])?;
    let empty = shape.layout(px(100.0))?;
    assert_eq!(empty.source(), "");
    assert!(empty.caret_bounds(at(0, Affinity::Downstream))?.size.height > px(0.0));
    assert!(
        empty
            .selection_bounds(at(0, Affinity::Downstream), at(0, Affinity::Upstream))?
            .is_empty()
    );
    assert!(shape.layout(px(f32::NAN)).is_err());
    assert!(shape.layout(px(-1.0)).is_err());
    let text = "a\u{2028}אבג\u{2028}";
    let hard = backend
        .prepare_paragraph(text, px(24.0), &runs(&backend, text)?)?
        .layout(px(500.0))?;
    assert_eq!(hard.parley_layout().len(), 3);
    assert!(
        hard.caret_bounds(at(text.len(), Affinity::Upstream))?
            .origin
            .y
            > px(0.0)
    );
    assert!(
        hard.caret_bounds(at(usize::MAX, Affinity::Upstream))
            .is_err()
    );
    assert!(hard.caret_bounds(at(2, Affinity::Upstream)).is_err());
    assert!(hard.hit_test(point(px(f32::NAN), px(0.0))).is_err());
    for text in ["a\nb", "a\rb", "a\u{2029}b"] {
        assert!(backend.prepare_paragraph(text, px(24.0), &[]).is_err());
    }
    Ok(())
}

#[test]
fn gpui_paragraph_cache_reuses_shapes_reflows_and_invalidates() -> Result<()> {
    use gpui::{PlatformParagraphLayout, TextSystem, WindowTextSystem};
    let backend = Arc::new(backend()?);
    let text_system = Arc::new(TextSystem::new(backend.clone()));
    let cache = WindowTextSystem::new(text_system.clone());
    let text = "abc אבג مرحبا words wrap into more lines";
    let runs = runs(&backend, text)?;
    let first = cache.layout_paragraph(text.into(), px(24.0), &runs, px(180.0))?;
    let shaped = backend.state.lock().unwrap().shape_calls;
    let again = cache.layout_paragraph(text.into(), px(24.0), &runs, px(180.0))?;
    assert!(Arc::ptr_eq(&first, &again));
    let wide = cache.layout_paragraph(text.into(), px(24.0), &runs, px(500.0))?;
    assert!(!Arc::ptr_eq(&first, &wide));
    assert_eq!(backend.state.lock().unwrap().shape_calls, shaped);
    assert!(first.size().height > wide.size().height);
    text_system.add_fonts(vec![Cow::Borrowed(INTER)])?;
    let fresh = cache.layout_paragraph(text.into(), px(24.0), &runs, px(180.0))?;
    assert!(!Arc::ptr_eq(&first, &fresh));
    assert_eq!(backend.state.lock().unwrap().shape_calls, shaped + 1);
    for run in first.runs() {
        for glyph in &run.glyphs {
            let params = gpui::RenderGlyphParams {
                font_id: run.font_id,
                glyph_id: glyph.id,
                font_size: first.font_size(),
                scale_factor: 1.0,
                subpixel_variant: point(0, 0),
                is_emoji: false,
                subpixel_rendering: false,
                dilation: 0,
            };
            let bounds = backend.glyph_raster_bounds(&params)?;
            backend.rasterize_glyph(&params, bounds)?;
        }
    }
    let empty = backend
        .prepare_paragraph("", px(24.0), &[])?
        .layout(px(180.0))?;
    assert!(empty.runs().is_empty());
    Ok(())
}

#[test]
fn visual_glyph_records_preserve_parley_positions_and_cluster_indices() -> Result<()> {
    use gpui::PlatformParagraphLayout;
    let backend = backend()?;
    let text = "office אבג مرحبا e\u{301} wrap words";
    let layout = backend
        .prepare_paragraph(text, px(24.0), &runs(&backend, text)?)?
        .layout(px(180.0))?;
    let mut converted = layout.runs().iter();
    let mut descending_indices = false;
    for line in layout.parley_layout().lines() {
        for item in line.items() {
            let parley::PositionedLayoutItem::GlyphRun(item) = item else {
                panic!("unexpected box")
            };
            let run = converted.next().unwrap();
            assert!(layout.resolved_faces().contains(&run.font_id));
            assert_eq!(run.glyphs.len(), item.positioned_glyphs().count());
            let expected_indices: Vec<_> = item
                .run()
                .visual_clusters()
                .flat_map(|cluster| {
                    let index = cluster.text_range().start;
                    cluster.glyphs().map(move |_| index)
                })
                .collect();
            for ((actual, expected), index) in run
                .glyphs
                .iter()
                .zip(item.positioned_glyphs())
                .zip(expected_indices)
            {
                assert_eq!(actual.position, point(px(expected.x), px(expected.y)));
                assert_eq!(actual.id, gpui::GlyphId(expected.id));
                assert_eq!(actual.index, index);
                assert!(text.is_char_boundary(actual.index));
            }
            descending_indices |= run.glyphs.windows(2).any(|g| g[0].index > g[1].index);
        }
    }
    assert!(descending_indices);
    assert!(converted.next().is_none());
    Ok(())
}

#[test]
fn paragraph_paint_reuses_the_existing_atlas_across_repaints_and_colors() -> Result<()> {
    use gpui::{
        TestAppContext, TestDispatcher, VisualTestContext, canvas, paint_paragraph, prelude::*,
        size,
    };
    let backend = Arc::new(backend()?);
    let text = "office אבג مرحبا";
    let runs = runs(&backend, text)?;
    let mut cx =
        TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend.clone());
    let window = cx.open_window(size(px(400.0), px(180.0)), |_, _| gpui::Empty);
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    let mut first = None;
    let mut first_raster_calls = 0;
    for pass in 0..2 {
        let layout = visual.update(|window, _| {
            window
                .text_system()
                .layout_paragraph(text.into(), px(24.0), &runs, px(240.0))
        })?;
        if let Some(first) = &first {
            assert!(Arc::ptr_eq(first, &layout));
        } else {
            first = Some(layout.clone());
        }
        visual.draw(
            point(px(50.0), px(30.0)),
            size(px(300.0), px(120.0)),
            |_, _| {
                canvas(
                    move |_, _, _| layout,
                    move |bounds, layout, window, _| {
                        paint_paragraph(
                            layout.as_ref(),
                            bounds.origin,
                            |index| {
                                // Logical source colors work even when Hebrew indices descend.
                                if index < 7 || pass == 1 {
                                    gpui::black()
                                } else {
                                    gpui::rgb(0xff0000).into()
                                }
                            },
                            window,
                        )
                        .unwrap();
                    },
                )
                .size_full()
            },
        );
        let calls = backend
            .raster_calls
            .load(std::sync::atomic::Ordering::Relaxed);
        if pass == 0 {
            assert!(calls > 0);
        } else {
            assert_eq!(calls, first_raster_calls);
        }
        // Save count outside the closure without holding a lock during painting.
        if pass == 0 {
            first_raster_calls = calls;
        }
    }
    Ok(())
}

#[test]
fn zero_width_reflows_preserve_content_and_work_through_gpui_cache() -> Result<()> {
    use gpui::{PlatformParagraphLayout, TextSystem, WindowTextSystem};
    let backend = Arc::new(backend()?);
    let cache = WindowTextSystem::new(Arc::new(TextSystem::new(backend.clone())));
    for text in ["", "office אבג e\u{301} words"] {
        let runs = runs(&backend, text)?;
        let shape = backend.prepare_paragraph(text, px(24.), &runs)?;
        let wide = shape.layout(px(500.))?;
        let zero = shape.layout(px(0.))?;
        assert_eq!(zero.size().width, px(0.));
        assert_eq!(zero.source(), text);
        assert!(f32::from(zero.size().height).is_finite());
        assert!(zero.size().height > px(0.));
        assert!(zero.caret_bounds(at(0, Affinity::Downstream))?.size.height > px(0.));
        if text.is_empty() {
            assert!(zero.runs().is_empty());
        } else {
            assert!(zero.runs().iter().any(|run| !run.glyphs.is_empty()));
            assert!(
                zero.runs()
                    .iter()
                    .flat_map(|run| &run.glyphs)
                    .all(|glyph| text.is_char_boundary(glyph.index))
            );
        }
        assert_eq!(wide.size().width, px(500.));
        assert_eq!(shape.layout(px(500.))?.size(), wide.size());

        let first = cache.layout_paragraph(text.into(), px(24.), &runs, px(0.))?;
        let calls = backend.state.lock().unwrap().shape_calls;
        let again = cache.layout_paragraph(text.into(), px(24.), &runs, px(0.))?;
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(first.size().width, px(0.));
        assert_eq!(first.source(), text);
        cache.layout_paragraph(text.into(), px(24.), &runs, px(500.))?;
        assert_eq!(backend.state.lock().unwrap().shape_calls, calls);
        for invalid in [-1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(shape.layout(px(invalid)).is_err());
            assert!(
                cache
                    .layout_paragraph(text.into(), px(24.), &runs, px(invalid))
                    .is_err()
            );
        }
    }
    Ok(())
}

#[test]
fn overflowing_paragraph_geometry_is_rejected_without_breaking_old_layouts() -> Result<()> {
    let backend = backend()?;
    let text = "HHHHHH";
    let runs = runs(&backend, text)?;
    let old_shape = backend.prepare_paragraph(text, px(24.), &runs)?;
    let old_layout = old_shape.layout(px(100.))?;
    assert!(
        backend
            .prepare_paragraph(text, px(f32::MAX), &runs)
            .is_err()
    );
    assert_eq!(old_layout.source(), text);
    assert_eq!(
        old_shape.layout(px(100.))?.parley_layout().height(),
        old_layout.parley_layout().height()
    );
    assert_eq!(
        old_shape.layout(px(200.))?.resolved_faces(),
        old_layout.resolved_faces()
    );
    Ok(())
}
