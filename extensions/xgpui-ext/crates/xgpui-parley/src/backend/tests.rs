use super::*;
use crate::test_support::{INTER, PLEX, fonts};
use gpui::{TextRun, TextSystem, WindowTextSystem, font};

#[test]
fn gpui_reuses_layouts_and_invalidates_after_font_loading() -> Result<()> {
    let backend = Arc::new(ParleyTextSystem::new(fonts(), "IBM Plex Sans")?);
    let text = Arc::new(TextSystem::new(backend.clone()));
    let window = WindowTextSystem::new(text.clone());
    let runs = [TextRun {
        len: 5,
        font: font("IBM Plex Sans"),
        ..Default::default()
    }];
    let first = window.layout_line("hello", px(16.0), &runs, None);
    let calls = backend.state.lock().unwrap().shape_calls;
    let again = window.layout_line("hello", px(16.0), &runs, None);
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(backend.state.lock().unwrap().shape_calls, calls);
    // A bad batch must not partially mutate the collection or generation.
    let generation = backend.state.lock().unwrap().generation;
    assert!(
        text.add_fonts(vec![Cow::Borrowed(PLEX), Cow::Borrowed(b"invalid")])
            .is_err()
    );
    assert_eq!(backend.state.lock().unwrap().generation, generation);
    text.add_fonts(vec![Cow::Borrowed(PLEX)])?;
    let after = window.layout_line("hello", px(16.0), &runs, None);
    assert!(!Arc::ptr_eq(&first, &after));
    assert!(backend.state.lock().unwrap().shape_calls > calls);
    // Already-shaped output retains valid font IDs across registration.
    let p = gpui::RenderGlyphParams {
        font_id: first.runs[0].font_id,
        glyph_id: first.runs[0].glyphs[0].id,
        font_size: px(16.0),
        scale_factor: 1.0,
        subpixel_variant: gpui::point(0, 0),
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    };
    let bounds = backend.glyph_raster_bounds(&p)?;
    let (extent, bytes) = backend.rasterize_glyph(&p, bounds)?;
    assert_eq!(
        bytes.len(),
        extent.width.0 as usize * extent.height.0 as usize
    );
    assert_eq!(extent, bounds.size);
    Ok(())
}

#[test]
fn descriptor_features_do_not_alias_resolved_face_identity() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    let normal = font("IBM Plex Sans");
    let mut plain = normal.clone();
    plain.features = gpui::FontFeatures(Arc::new(vec![("liga".into(), 0)]));
    let on = backend.font_id(&normal)?;
    let off = backend.font_id(&plain)?;
    assert_eq!(backend.font_id(&normal)?, on);
    assert_ne!(on, off);
    let enabled = backend.try_layout_line(
        "office",
        px(16.0),
        &[FontRun {
            len: 6,
            font_id: on,
        }],
    )?;
    let disabled = backend.try_layout_line(
        "office",
        px(16.0),
        &[FontRun {
            len: 6,
            font_id: off,
        }],
    )?;
    assert!(
        enabled.runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
            < disabled.runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
    );
    assert_eq!(enabled.runs[0].font_id, disabled.runs[0].font_id);
    assert_eq!(
        backend.font_id(&font(".SystemUIFont"))?,
        backend.font_id(&font(".SystemUIFont"))?
    );
    assert!(backend.font_id(&font("not installed")).is_err());
    assert!(backend.try_layout_line("שלום", px(16.0), &[]).is_err());
    assert!(
        backend
            .try_layout_line(
                "é",
                px(16.0),
                &[FontRun {
                    len: 1,
                    font_id: on
                }]
            )
            .is_err()
    );
    assert_eq!(backend.try_layout_line("", px(16.0), &[])?.len, 0);
    Ok(())
}

#[test]
fn descriptor_fallbacks_keep_actual_raster_faces() -> Result<()> {
    use skrifa::MetadataProvider;
    let primary = skrifa::FontRef::new(INTER)?;
    let fallback = skrifa::FontRef::new(PLEX)?;
    let ch = (0x00c0..0x0250)
        .filter_map(char::from_u32)
        .find(|&ch| primary.charmap().map(ch).is_none() && fallback.charmap().map(ch).is_some())
        .unwrap();
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    let mut descriptor = font("Inter Variable");
    descriptor.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
        "IBM Plex Sans".into(),
    ]));
    let id = backend.font_id(&descriptor)?;
    let text = format!("H{ch}");
    let line = backend.try_layout_line(
        &text,
        px(16.0),
        &[FontRun {
            len: text.len(),
            font_id: id,
        }],
    )?;
    assert_eq!(line.runs.len(), 2);
    assert_ne!(line.runs[0].font_id, line.runs[1].font_id);
    for run in &line.runs {
        let p = RenderGlyphParams {
            font_id: run.font_id,
            glyph_id: run.glyphs[0].id,
            font_size: px(16.0),
            scale_factor: 1.0,
            subpixel_variant: gpui::point(0, 0),
            is_emoji: false,
            subpixel_rendering: false,
            dilation: 0,
        };
        let bounds = backend.glyph_raster_bounds(&p)?;
        assert!(!backend.rasterize_glyph(&p, bounds)?.1.is_empty());
    }
    Ok(())
}

#[test]
fn missing_glyph_sink_can_reenter_without_deadlock() -> Result<()> {
    struct Sink {
        backend: std::sync::Weak<ParleyTextSystem>,
        reports: Mutex<Vec<gpui::MissingGlyph>>,
    }
    impl gpui::MissingGlyphSink for Sink {
        fn report(&self, missing: Vec<gpui::MissingGlyph>) {
            self.backend.upgrade().unwrap().all_font_names();
            self.reports.lock().unwrap().extend(missing);
        }
    }
    let backend = Arc::new(ParleyTextSystem::new(fonts(), "IBM Plex Sans")?);
    let sink = Arc::new(Sink {
        backend: Arc::downgrade(&backend),
        reports: Mutex::new(Vec::new()),
    });
    backend.set_missing_glyph_sink(Some(sink.clone()));
    backend.try_layout_line("\u{10ffff}", px(16.0), &[])?;
    assert_eq!(sink.reports.lock().unwrap()[0].grapheme(), "\u{10ffff}");
    Ok(())
}

#[test]
fn rejected_layouts_keep_source_indices_and_stable_cached_face() -> Result<()> {
    let backend = Arc::new(ParleyTextSystem::new(fonts(), "IBM Plex Sans")?);
    let reports = Arc::new(Mutex::new(0));
    let weak = Arc::downgrade(&backend);
    let count = reports.clone();
    backend.set_layout_error_handler(Arc::new(move |_| {
        weak.upgrade().unwrap().all_font_names(); // Callback must be reentrant.
        *count.lock().unwrap() += 1;
    }));
    let descriptor = font("IBM Plex Sans");
    let id = backend.font_id(&descriptor)?;
    let valid = backend.try_layout_line(
        "hello",
        px(16.0),
        &[FontRun {
            len: 5,
            font_id: id,
        }],
    )?;
    let window = WindowTextSystem::new(Arc::new(TextSystem::new(backend.clone())));
    let source = "aשלום";
    let runs = [TextRun {
        len: source.len(),
        font: descriptor.clone(),
        ..Default::default()
    }];
    let first = window.layout_line(source, px(16.0), &runs, None);
    let again = window.layout_line(source, px(16.0), &runs, None);
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(*reports.lock().unwrap(), 1);
    assert_eq!(first.len, source.len());
    assert_eq!(
        first.runs[0]
            .glyphs
            .iter()
            .map(|g| g.index)
            .collect::<Vec<_>>(),
        source.char_indices().map(|(i, _)| i).collect::<Vec<_>>()
    );
    assert!(first.runs[0].glyphs.iter().all(|g| g.id == GlyphId(0)));
    let face = first.runs[0].font_id;
    let params = RenderGlyphParams {
        font_id: face,
        glyph_id: GlyphId(0),
        font_size: px(16.0),
        scale_factor: 1.0,
        subpixel_variant: point(0, 0),
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    };
    let bounds = backend.glyph_raster_bounds(&params)?;
    assert!(!backend.rasterize_glyph(&params, bounds)?.1.is_empty());
    backend.add_fonts(vec![Cow::Borrowed(INTER)])?;
    for source in ["שלום", "a\nb", "é"] {
        // Invalid byte range must also avoid panicking in the infallible hook.
        let rejected = backend.layout_line(
            source,
            px(24.0),
            &[FontRun {
                len: 1,
                font_id: id,
            }],
        );
        assert_eq!(rejected.runs[0].font_id, face);
        assert_eq!(rejected.len, source.len());
    }
    let old_metrics = backend
        .state
        .lock()
        .unwrap()
        .faces
        .get(valid.runs[0].font_id)?
        .metrics()?;
    assert_eq!(
        old_metrics.units_per_em,
        backend.font_metrics(id).units_per_em
    );
    assert!(f32::from(backend.layout_line("x", px(f32::NAN), &[]).width).is_finite());
    Ok(())
}

#[test]
fn unsupported_synthesis_does_not_replace_requested_or_resolved_ids() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    backend.set_layout_error_handler(Arc::new(|_| {}));
    let mut descriptor = font("Inter Variable");
    descriptor.weight = gpui::FontWeight::BLACK;
    descriptor.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
        "IBM Plex Sans".into(),
    ]));
    let id = backend.font_id(&descriptor)?;
    let runs = [FontRun {
        len: 1,
        font_id: id,
    }];
    let before = backend.try_layout_line("H", px(24.0), &runs)?;
    // Inter lacks U+0149; our regular-only Plex fixture requires faux bold.
    let rejected_runs = [FontRun {
        len: "Hŉ".len(),
        font_id: id,
    }];
    assert!(
        backend
            .try_layout_line("Hŉ", px(24.0), &rejected_runs)
            .is_err()
    );
    let rejected = backend.layout_line("Hŉ", px(24.0), &rejected_runs);
    assert!(rejected.runs[0].glyphs.iter().all(|g| g.id == GlyphId(0)));
    assert_eq!(backend.font_id(&descriptor)?, id);
    let after = backend.try_layout_line("H", px(24.0), &runs)?;
    assert_eq!(before.runs[0].font_id, after.runs[0].font_id);
    assert_eq!(before.width, after.width);
    Ok(())
}
