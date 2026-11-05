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
        .faces
        .read()
        .unwrap()
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

#[test]
fn metrics_and_rasterization_do_not_wait_for_shaping_state() -> Result<()> {
    let backend = Arc::new(ParleyTextSystem::new(fonts(), "IBM Plex Sans")?);
    let id = backend.font_id(&font("IBM Plex Sans"))?;
    let glyph = backend.glyph_for_char(id, 'H').unwrap();
    let state = backend.state.lock().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let worker = {
        let backend = backend.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                assert!(backend.font_metrics(id).units_per_em > 0);
                assert_eq!(backend.glyph_for_char(id, 'H'), Some(glyph));
                assert!(backend.advance(id, glyph)?.width > 0.);
                backend.typographic_bounds(id, glyph)?;
                let params = RenderGlyphParams {
                    font_id: id,
                    glyph_id: glyph,
                    font_size: px(24.),
                    scale_factor: 1.,
                    subpixel_variant: point(0, 0),
                    is_emoji: false,
                    subpixel_rendering: false,
                    dilation: 0,
                };
                let bounds = backend.glyph_raster_bounds(&params)?;
                assert!(!backend.rasterize_glyph(&params, bounds)?.1.is_empty());
                Ok(())
            })();
            send.send(result).unwrap();
        })
    };
    let result = receive.recv_timeout(std::time::Duration::from_secs(2));
    // Release before checking the result so a regression cannot strand the worker.
    drop(state);
    worker.join().unwrap();
    result.context("metrics/rasterization blocked on shaping state")??;
    Ok(())
}

#[test]
fn descriptor_index_reuses_ids_and_invalidates_only_after_successful_font_loading() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    let descriptor = font("IBM Plex Sans");
    let old = backend.font_id(&descriptor)?;
    let calls = backend.state.lock().unwrap().shape_calls;
    assert_eq!(backend.font_id(&descriptor)?, old);
    assert_eq!(backend.state.lock().unwrap().shape_calls, calls);

    assert!(backend.add_fonts(vec![Cow::Borrowed(b"invalid")]).is_err());
    assert_eq!(backend.font_id(&descriptor)?, old);
    assert_eq!(backend.state.lock().unwrap().shape_calls, calls);

    backend.add_fonts(vec![Cow::Borrowed(PLEX)])?;
    assert!(backend.state.lock().unwrap().descriptors.is_empty());
    let new = backend.font_id(&descriptor)?;
    assert_eq!(
        backend.state.lock().unwrap().shape_calls,
        calls + 1,
        "font registration must re-resolve the descriptor, even if its face is unchanged"
    );
    let calls = backend.state.lock().unwrap().shape_calls;
    assert_eq!(backend.font_id(&descriptor)?, new);
    assert_eq!(backend.state.lock().unwrap().shape_calls, calls);
    assert_eq!(backend.state.lock().unwrap().descriptors.len(), 1);

    // Retained layouts may still ask to shape with the original descriptor ID.
    let line = backend.try_layout_line(
        "H",
        px(16.),
        &[FontRun {
            len: 1,
            font_id: old,
        }],
    )?;
    assert!(!line.runs.is_empty());
    assert_eq!(
        backend.font_metrics(old).units_per_em,
        backend.font_metrics(new).units_per_em
    );
    Ok(())
}

#[test]
fn unrelated_font_registration_does_not_accumulate_descriptor_aliases() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    let normal = font("IBM Plex Sans");
    let mut no_ligatures = normal.clone();
    no_ligatures.features = gpui::FontFeatures(Arc::new(vec![("liga".into(), 0)]));
    let normal_id = backend.font_id(&normal)?;
    let feature_id = backend.font_id(&no_ligatures)?;
    assert_ne!(normal_id, feature_id);
    let bindings = backend.state.lock().unwrap().bindings.len();
    let next_face = FontId(feature_id.0 + 1);
    assert!(backend.faces.read().unwrap().get(next_face).is_err());
    for _ in 0..10 {
        // Changes the collection generation without replacing either Plex face.
        backend.add_fonts(vec![Cow::Borrowed(INTER)])?;
        assert_eq!(backend.font_id(&normal)?, normal_id);
        assert_eq!(backend.font_id(&no_ligatures)?, feature_id);
        let state = backend.state.lock().unwrap();
        assert_eq!(state.bindings.len(), bindings);
        assert_eq!(state.aliases[&normal].len(), 1);
        assert_eq!(state.aliases[&no_ligatures].len(), 1);
        drop(state);
        assert!(backend.faces.read().unwrap().get(next_face).is_err());
    }
    // An old requested ID still carries its features after all invalidations.
    let on = backend.try_layout_line(
        "office",
        px(16.),
        &[FontRun {
            len: 6,
            font_id: normal_id,
        }],
    )?;
    let off = backend.try_layout_line(
        "office",
        px(16.),
        &[FontRun {
            len: 6,
            font_id: feature_id,
        }],
    )?;
    assert!(
        on.runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
            < off.runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
    );
    Ok(())
}

#[test]
fn duplicate_font_batches_do_not_retain_more_collection_entries() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    let descriptor = font("IBM Plex Sans");
    let id = backend.font_id(&descriptor)?;
    let font_runs = [FontRun {
        len: 6,
        font_id: id,
    }];
    let paragraph = backend.prepare_paragraph("office", px(24.), &font_runs)?;
    let old_layout = paragraph.layout(px(150.))?;
    let glyph = backend.glyph_for_char(id, 'H').unwrap();
    let params = RenderGlyphParams {
        font_id: id,
        glyph_id: glyph,
        font_size: px(24.),
        scale_factor: 1.,
        subpixel_variant: point(0, 0),
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    };
    let bounds = backend.glyph_raster_bounds(&params)?;
    let original_mask = backend.rasterize_glyph(&params, bounds)?;
    let counts = || {
        let mut state = backend.state.lock().unwrap();
        ["IBM Plex Sans", "Inter Variable"].map(|name| {
            state
                .fonts
                .collection
                .family_by_name(name)
                .unwrap()
                .fonts()
                .len()
        })
    };
    let original_counts = counts();
    for _ in 0..10 {
        backend.add_fonts(vec![
            Cow::Borrowed(PLEX),
            Cow::Borrowed(INTER),
            Cow::Borrowed(PLEX),
        ])?;
        assert_eq!(
            counts(),
            original_counts,
            "identical font blobs must not accumulate"
        );
        assert_eq!(backend.font_id(&descriptor)?, id);
    }
    assert!(
        backend
            .add_fonts(vec![Cow::Borrowed(PLEX), Cow::Borrowed(b"invalid")])
            .is_err()
    );
    assert_eq!(counts(), original_counts);
    assert_eq!(backend.rasterize_glyph(&params, bounds)?, original_mask);
    assert_eq!(old_layout.source(), "office");
    assert_eq!(
        paragraph.layout(px(150.))?.parley_layout().height(),
        old_layout.parley_layout().height()
    );
    assert_eq!(
        paragraph.layout(px(90.))?.resolved_faces(),
        old_layout.resolved_faces()
    );
    Ok(())
}

#[test]
fn new_font_data_is_registered_and_only_identical_copies_are_skipped() -> Result<()> {
    let mut context = FontContext::new();
    context
        .collection
        .register_fonts(parley::fontique::Blob::new(Arc::new(PLEX.to_vec())), None);
    let backend = ParleyTextSystem::new(context, "IBM Plex Sans")?;
    assert!(backend.font_id(&font("Inter Variable")).is_err());
    backend.add_fonts(vec![Cow::Borrowed(INTER), Cow::Owned(INTER.to_vec())])?;
    let id = backend.font_id(&font("Inter Variable"))?;
    assert!(backend.glyph_for_char(id, 'H').is_some());
    {
        let mut state = backend.state.lock().unwrap();
        assert_eq!(
            state
                .fonts
                .collection
                .family_by_name("Inter Variable")
                .unwrap()
                .fonts()
                .len(),
            1
        );
    }
    // A distinct valid blob may use the same family, weight and face index.
    // Never deduplicate solely by those attributes: its outlines could differ.
    let mut distinct = INTER.to_vec();
    distinct.extend_from_slice(&[0; 4]);
    backend.add_fonts(vec![Cow::Owned(distinct)])?;
    let mut state = backend.state.lock().unwrap();
    assert_eq!(
        state
            .fonts
            .collection
            .family_by_name("Inter Variable")
            .unwrap()
            .fonts()
            .len(),
        2
    );
    drop(state);
    assert!(backend.font_metrics(id).units_per_em > 0);
    Ok(())
}

#[test]
fn rejected_synthesis_does_not_publish_partial_faces() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    let mut descriptor = font("Inter Variable");
    descriptor.weight = gpui::FontWeight::BLACK;
    descriptor.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
        "IBM Plex Sans".into(),
    ]));
    let id = backend.font_id(&descriptor)?;
    let count_faces = || {
        let faces = backend.faces.read().unwrap();
        (0..)
            .take_while(|&index| faces.get(FontId(index)).is_ok())
            .count()
    };
    let before = count_faces();
    for size in [24., 48., 72.] {
        let runs = [FontRun {
            len: "Hŉ".len(),
            font_id: id,
        }];
        assert!(backend.try_layout_line("Hŉ", px(size), &runs).is_err());
        assert_eq!(count_faces(), before, "rejected line published faces");
        assert!(backend.prepare_paragraph("Hŉ", px(size), &runs).is_err());
        assert_eq!(count_faces(), before, "rejected paragraph published faces");
    }
    Ok(())
}

#[test]
fn overflowing_line_geometry_is_rejected_and_fallback_stays_finite() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    backend.set_layout_error_handler(Arc::new(|_| {}));
    let result = backend.try_layout_line("HHHHHH", px(f32::MAX), &[]);
    assert!(result.is_err(), "unexpected geometry: {result:?}");
    let rejected = backend.layout_line("HHHHHH", px(f32::MAX), &[]);
    assert!(f32::from(rejected.width).is_finite());
    assert!(f32::from(rejected.ascent).is_finite());
    assert!(f32::from(rejected.descent).is_finite());
    assert!(
        rejected
            .runs
            .iter()
            .flat_map(|r| &r.glyphs)
            .all(|g| f32::from(g.position.x).is_finite() && f32::from(g.position.y).is_finite())
    );
    assert!(backend.try_layout_line("normal", px(16.), &[]).is_ok());
    Ok(())
}

#[test]
fn feature_introspection_uses_resolved_fixture_and_retains_numeric_controls() -> Result<()> {
    let backend = Arc::new(ParleyTextSystem::new(fonts(), "IBM Plex Sans")?);
    let text = TextSystem::new(backend.clone());
    let mut descriptor = font("Inter Variable");
    descriptor.features = gpui::FontFeatures(Arc::new(vec![("cv02".into(), 2)]));
    let id = backend.font_id(&descriptor)?;
    let gpui::FontQuery::Supported(features) = text.opentype_features(id)? else {
        panic!("Parley fixture queries must be supported")
    };
    assert!(
        features
            .iter()
            .any(|f| f.tag.to_string() == "cv02" && f.substitution)
    );
    assert!(
        features
            .iter()
            .any(|f| f.tag.to_string() == "kern" && f.positioning)
    );
    assert!(features.windows(2).all(|f| f[0].tag < f[1].tag));
    assert_eq!(
        backend.state.lock().unwrap().bindings[&id]
            .features
            .tag_value_list(),
        &[("cv02".into(), 2)]
    );
    let layout = backend.try_layout_line(
        "a",
        px(16.0),
        &[FontRun {
            len: 1,
            font_id: id,
        }],
    )?;
    assert!(!layout.runs[0].glyphs.is_empty());
    assert!(text.opentype_features(FontId(usize::MAX)).is_err());
    Ok(())
}

pub(super) fn axes(settings: &[(&str, f32)]) -> gpui::FontVariations {
    gpui::FontVariations::new(
        settings
            .iter()
            .map(|(tag, value)| (tag.parse().unwrap(), *value)),
    )
    .unwrap()
}

#[test]
fn static_axes_keep_precise_instances_metrics_and_rasterization_consistent() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    assert!(backend.font_capabilities().static_variations);
    let mut descriptor = font("Roboto Flex");
    let plain = backend.font_id(&descriptor)?;
    let gpui::FontQuery::Supported(available) = backend.font_axes(plain)? else {
        panic!("axes unsupported")
    };
    assert!(
        available
            .iter()
            .any(|axis| axis.tag.to_string() == "slnt" && axis.min_value < 0.0)
    );
    descriptor.weight = gpui::FontWeight::BOLD;
    descriptor.variations = axes(&[
        ("wght", 300.25),
        ("wdth", 75.0),
        ("slnt", -10.0),
        ("opsz", 14.0),
    ]);
    let narrow = backend.font_id(&descriptor)?;
    let canonical = descriptor.clone();
    descriptor.variations = axes(&[
        ("opsz", 14.0),
        ("slnt", -10.0),
        ("wdth", 75.0),
        ("wght", 300.25),
    ]);
    assert_eq!(backend.font_id(&descriptor)?, narrow);
    let face = backend.face(narrow)?;
    // Explicit wght wins over FontWeight::BOLD; slant remains negative.
    let font = skrifa::FontRef::from_index(face.data().data.data(), face.data().index)?;
    use skrifa::MetadataProvider;
    let expected = font.axes().location([
        ("wght", 300.25),
        ("wdth", 75.0),
        ("slnt", -10.0),
        ("opsz", 14.0),
    ]);
    let coords = expected.coords();
    assert_eq!(
        face.normalized_coords(),
        &coords[..face.normalized_coords().len()]
    );
    let h = backend.glyph_for_char(narrow, 'H').unwrap();
    let render = |id| -> Result<_> {
        let params = RenderGlyphParams {
            font_id: id,
            glyph_id: h,
            font_size: px(32.0),
            scale_factor: 1.0,
            subpixel_variant: point(0, 0),
            is_emoji: false,
            subpixel_rendering: false,
            dilation: 0,
        };
        let bounds = backend.glyph_raster_bounds(&params)?;
        Ok((bounds, backend.rasterize_glyph(&params, bounds)?.1))
    };
    let before = render(narrow)?;
    descriptor.variations = axes(&[
        ("wght", 900.0),
        ("wdth", 150.0),
        ("slnt", 0.0),
        ("opsz", 14.0),
    ]);
    let wide = backend.font_id(&descriptor)?;
    assert_ne!(wide, narrow);
    assert_ne!(
        backend.advance(narrow, h)?.width,
        backend.advance(wide, h)?.width
    );
    assert_ne!(before, render(wide)?);
    assert_eq!(
        before,
        render(narrow)?,
        "another instance cannot change retained raster output"
    );
    let line = backend.try_layout_line(
        "H",
        px(32.0),
        &[FontRun {
            len: 1,
            font_id: narrow,
        }],
    )?;
    let advance = backend.advance(line.runs[0].font_id, h)?.width * 32.0
        / backend.font_metrics(line.runs[0].font_id).units_per_em as f32;
    assert!((f32::from(line.width) - advance).abs() < 0.02);
    assert_eq!(backend.font_id(&canonical)?, narrow);
    descriptor.variations = axes(&[("wght", 5000.0)]);
    let clamped = backend.font_id(&descriptor)?;
    descriptor.variations = axes(&[("wght", 1000.0)]);
    let maximum = backend.font_id(&descriptor)?;
    assert_eq!(
        backend.face(clamped)?.normalized_coords(),
        backend.face(maximum)?.normalized_coords()
    );
    assert!(backend.font_axes(FontId(usize::MAX)).is_err());
    Ok(())
}

#[test]
fn unsupported_primary_axes_are_preserved_for_variable_fallback() -> Result<()> {
    let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
    let mut descriptor = font("IBM Plex Sans");
    descriptor.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec!["Roboto Flex".into()]));
    descriptor.variations = axes(&[("wght", 800.0), ("slnt", -10.0)]);
    let id = backend.font_id(&descriptor)?;
    // Find a fixture character only the fallback can render.
    let primary = backend.face(id)?;
    let flex = backend.font_id(&font("Roboto Flex"))?;
    let fallback = backend.face(flex)?;
    let ch = (0x20..0x3000)
        .filter_map(char::from_u32)
        .find(|&ch| primary.glyph_for_char(ch).is_none() && fallback.glyph_for_char(ch).is_some())
        .context("missing fallback-only fixture character")?;
    let text = ch.to_string();
    let line = backend.try_layout_line(
        &text,
        px(24.0),
        &[FontRun {
            len: text.len(),
            font_id: id,
        }],
    )?;
    let resolved = backend.face(line.runs[0].font_id)?;
    assert!(
        !resolved.normalized_coords().is_empty(),
        "fallback must receive author axes"
    );
    assert_ne!(resolved.data().data.id(), primary.data().data.id());
    Ok(())
}

#[test]
#[ignore = "CPU baseline diagnostic; run explicitly with --nocapture"]
fn multi_axis_sweep_baseline() -> Result<()> {
    use std::{collections::HashSet, time::Instant};
    for quantized in [false, true] {
        let backend = ParleyTextSystem::new(fonts(), "IBM Plex Sans")?;
        let base = font("Roboto Flex");
        let id = backend.font_id(&base)?;
        let gpui::FontQuery::Supported(available) = backend.font_axes(id)? else {
            unreachable!()
        };
        let policies = available.into_iter().filter_map(|axis| {
            let interval = match axis.tag.to_string().as_str() {
                "wght" => 400.0,
                "wdth" => 50.0,
                "slnt" => 5.0,
                _ => return None,
            };
            Some((axis, interval))
        });
        let mut preview = crate::VariationPreview::new(base.clone(), policies, 64)?;
        let mut glyph_keys = HashSet::new();
        let mut mask_bytes = 0;
        let mut previous = None;
        let now = Instant::now();
        for cycle in 0..3 {
            for w in 0..8 {
                for d in 0..8 {
                    for s in 0..4 {
                        let author = axes(&[
                            ("wght", 100.0 + w as f32 * 800.0 / 7.0),
                            ("wdth", 50.0 + d as f32 * 100.0 / 7.0),
                            ("slnt", -10.0 + s as f32 * 10.0 / 3.0),
                        ]);
                        let descriptor = if quantized {
                            preview.font(&author)?
                        } else {
                            let mut f = base.clone();
                            f.variations = author;
                            f
                        };
                        let id = backend.font_id(&descriptor)?;
                        let text = "Hamburgefonts 012345";
                        let line = backend.try_layout_line(
                            text,
                            px(28.0),
                            &[FontRun {
                                len: text.len(),
                                font_id: id,
                            }],
                        )?;
                        for run in line.runs {
                            for glyph in run.glyphs {
                                let params = RenderGlyphParams {
                                    font_id: run.font_id,
                                    glyph_id: glyph.id,
                                    font_size: px(28.0),
                                    scale_factor: 1.0,
                                    subpixel_variant: point(0, 0),
                                    is_emoji: false,
                                    subpixel_rendering: false,
                                    dilation: 0,
                                };
                                if glyph_keys.insert(params.clone()) {
                                    let bounds = backend.glyph_raster_bounds(&params)?;
                                    if bounds.size.width.0 > 0 && bounds.size.height.0 > 0 {
                                        mask_bytes +=
                                            backend.rasterize_glyph(&params, bounds)?.1.len();
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let state = backend.state.lock().unwrap();
            let snapshot = (
                state.descriptors.len(),
                backend.faces.read().unwrap().faces_count_for_tests(),
                glyph_keys.len(),
                mask_bytes,
            );
            println!(
                "quantized={quantized} cycle={cycle} requested={} registered={} raster_keys={} mask_bytes={} elapsed_ms={:.2}",
                snapshot.0,
                snapshot.1,
                snapshot.2,
                snapshot.3,
                now.elapsed().as_secs_f64() * 1000.0
            );
            if let Some(previous) = previous {
                assert_eq!(
                    snapshot, previous,
                    "identical sweeps must stop growing after warmup"
                );
            }
            previous = Some(snapshot);
        }
    }
    Ok(())
}
