use super::*;
use crate::test_support::fonts;
use gpui::{
    InteractiveElement as _, ParentElement as _, PlatformParagraphLayout as _,
    PlatformTextSystem as _, Styled as _, TestAppContext, TestDispatcher, VisualTestContext, div,
};

const LINE_HEIGHT: Pixels = px(20.0);
const FOX: &str = "The quick brown fox jumps over the lazy dog";

fn backend() -> Arc<ParleyTextSystem> {
    let mut fonts = fonts();
    for bytes in [
        include_bytes!("../../tests/fonts/NotoSansArabic.ttf").as_slice(),
        include_bytes!("../../tests/fonts/NotoSansHebrew.ttf").as_slice(),
    ] {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(bytes.to_vec())), None);
    }
    Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans").unwrap())
}

fn font() -> gpui::Font {
    let mut font = gpui::font("IBM Plex Sans");
    font.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
        "Noto Sans Hebrew".into(),
        "Noto Sans Arabic".into(),
    ]));
    font
}

struct Fixture {
    engine: TypesetEngine,
    font: FontId,
}

impl Fixture {
    fn new() -> Self {
        let parley = backend();
        let font = parley.font_id(&font()).unwrap();
        Self {
            engine: TypesetEngine::new(parley),
            font,
        }
    }

    fn spec(&self, text: &str) -> TypesetSpec {
        TypesetSpec {
            text: text.to_owned().into(),
            fonts: Arc::new([FontRun {
                len: text.len(),
                font_id: self.font,
            }]),
            font_size: px(16.0),
            line_height: LINE_HEIGHT,
            options: ParagraphOptions {
                line_height: Some(LINE_HEIGHT),
                ..Default::default()
            },
            wrap: true,
            overflow: None,
            line_clamp: None,
            align: AlignKey::Start,
        }
    }

    fn fit(&self, spec: &TypesetSpec, width: AvailableSpace) -> Fitted {
        self.engine
            .fit(
                spec,
                size(None, None),
                size(width, AvailableSpace::MaxContent),
            )
            .unwrap()
    }

    fn shape_calls(&self) -> usize {
        self.engine.parley.shape_calls()
    }
}

fn nowrap(mut spec: TypesetSpec, overflow: Option<OverflowKey>) -> TypesetSpec {
    spec.wrap = false;
    spec.overflow = overflow;
    spec
}

fn ellipsis() -> SharedString {
    "…".into()
}

#[test]
fn wrapped_text_reports_min_and_max_content_like_taffys_parley_example() {
    let fixture = Fixture::new();
    let spec = fixture.spec("alpha beta gamma delta");
    let (min, max) = fixture
        .engine
        .shape(&spec, &spec.text, spec.fonts.clone())
        .unwrap()
        .shape
        .content_widths();
    assert!(min < max);

    let narrow = fixture.fit(&spec, AvailableSpace::MinContent);
    assert_eq!(narrow.size.width, min.ceil());
    assert_eq!(narrow.size.height, LINE_HEIGHT * 4.0, "one word per line");

    let wide = fixture.fit(&spec, AvailableSpace::MaxContent);
    assert_eq!(wide.size.width, max.ceil());
    assert_eq!(wide.size.height, LINE_HEIGHT);

    // Definite space is clamped between the content widths.
    let roomy = fixture.fit(&spec, AvailableSpace::Definite(max + px(200.0)));
    assert_eq!(roomy.size.width, max.ceil());
    let tight = fixture.fit(&spec, AvailableSpace::Definite(px(1.0)));
    assert_eq!(tight.size.width, min.ceil());
    let between = (min + max) / 2.0;
    let middle = fixture.fit(&spec, AvailableSpace::Definite(between));
    assert_eq!(middle.size.width, between.ceil());
    assert!(middle.size.height > LINE_HEIGHT);
}

#[test]
fn unwrapped_text_keeps_one_line_per_hard_break() {
    let fixture = Fixture::new();
    // Two runs: run lengths follow the normalized text.
    let style = gpui::TextStyle::default();
    let (text, runs) = hard_breaks(
        &"one\ntwo\r\nthree".into(),
        vec![
            style.to_run("one\n".len()),
            style.to_run("two\r\nthree".len()),
        ],
    );
    assert_eq!(text.as_ref(), "one\u{2028}two\u{2028}three");
    assert_eq!(
        runs.iter().map(|run| run.len).collect::<Vec<_>>(),
        ["one\u{2028}".len(), "two\u{2028}three".len()]
    );
    let fitted = fixture.fit(
        &nowrap(fixture.spec(&text), None),
        AvailableSpace::Definite(px(10.0)),
    );
    assert_eq!(fitted.size.height, LINE_HEIGHT * 3.0);
    // Without truncation an unbroken line keeps its natural width and may overflow.
    assert!(fitted.size.width > px(10.0));
}

#[test]
fn end_start_and_middle_truncation_fit_and_keep_their_sides() {
    let fixture = Fixture::new();
    let limit = px(140.0);
    let cases = [
        (OverflowKey::End(ellipsis()), true, false),
        (OverflowKey::Start(ellipsis()), false, true),
        (OverflowKey::Middle(ellipsis()), true, true),
    ];
    for (overflow, keeps_start, keeps_end) in cases {
        let spec = nowrap(fixture.spec(FOX), Some(overflow.clone()));
        let fitted = fixture.fit(&spec, AvailableSpace::Definite(limit));
        let shown = fitted.shaped.shape.source();
        assert_eq!(fitted.size.width, limit, "{overflow:?}");
        assert!(fitted.shaped.shape.content_widths().1 <= limit, "{shown}");
        assert!(shown.contains('…'), "{shown}");
        assert!(shown.len() < FOX.len());
        assert_eq!(shown.starts_with("The"), keeps_start, "{shown}");
        assert_eq!(shown.ends_with("dog"), keeps_end, "{shown}");
        assert!(
            !shown.contains(" …") && !shown.contains("… "),
            "trim at the cut: {shown}"
        );
    }
    // Text that fits is never truncated, and its width is its natural width.
    let spec = nowrap(fixture.spec("short"), Some(OverflowKey::End(ellipsis())));
    let fitted = fixture.fit(&spec, AvailableSpace::Definite(limit));
    assert_eq!(fitted.shaped.shape.source(), "short");
    assert!(fitted.size.width < limit);
}

#[test]
fn truncation_uses_real_shaping_for_joined_scripts() {
    let fixture = Fixture::new();
    let text = "مرحبا بالعالم هذا نص عربي طويل جدا يحتاج إلى اقتطاع";
    let spec = nowrap(fixture.spec(text), Some(OverflowKey::End(ellipsis())));
    let fitted = fixture.fit(&spec, AvailableSpace::Definite(px(120.0)));
    assert!(fitted.shaped.shape.content_widths().1 <= px(120.0));
    assert!(fitted.shaped.shape.source().ends_with('…'));
    assert!(fitted.shaped.shape.is_rtl());
}

#[test]
fn rtl_truncation_shows_the_ellipsis_on_the_left_and_aligns_to_the_right() {
    let fixture = Fixture::new();
    let mut spec = nowrap(
        fixture.spec("שלום עולם זה טקסט ארוך מאוד שצריך לקצר"),
        Some(OverflowKey::End(ellipsis())),
    );
    spec.options.direction = TextDirection::RightToLeft;
    let fitted = fixture.fit(&spec, AvailableSpace::Definite(px(120.0)));
    let layout = fixture
        .engine
        .layout(&fitted.shaped, None, fitted.align)
        .unwrap();
    let source = layout.source();
    let ellipsis_index = source.len() - "…".len();
    let glyphs: Vec<_> = layout.runs().iter().flat_map(|run| &run.glyphs).collect();
    let ellipsis_x = glyphs
        .iter()
        .filter(|glyph| glyph.index == ellipsis_index)
        .map(|glyph| glyph.position.x)
        .fold(px(f32::MAX), Pixels::min);
    let others_x = glyphs
        .iter()
        .filter(|glyph| glyph.index != ellipsis_index)
        .map(|glyph| glyph.position.x)
        .fold(px(f32::MAX), Pixels::min);
    assert!(ellipsis_x < others_x, "logical end is the visual left");
    // Start alignment places an unbroken RTL line at the right edge.
    assert_eq!(AlignKey::Start.offset(px(30.0), true), px(30.0));
    assert_eq!(AlignKey::Start.offset(px(30.0), false), px(0.0));
    assert_eq!(AlignKey::End.offset(px(30.0), true), px(0.0));
    assert_eq!(AlignKey::Center.offset(px(30.0), true), px(15.0));
}

#[test]
fn line_clamp_limits_height_and_truncates_the_last_line() {
    let fixture = Fixture::new();
    let width = AvailableSpace::Definite(px(120.0));
    let mut spec = fixture.spec(&format!("{FOX}. {FOX}."));
    spec.line_clamp = Some(2);
    let clipped = fixture.fit(&spec, width);
    assert_eq!(clipped.size.height, LINE_HEIGHT * 2.0);
    assert_eq!(
        clipped.shaped.shape.source(),
        spec.text.as_ref(),
        "no affix without overflow"
    );

    spec.overflow = Some(OverflowKey::End(ellipsis()));
    let truncated = fixture.fit(&spec, width);
    assert_eq!(truncated.size.height, LINE_HEIGHT * 2.0);
    let shown = truncated.shaped.shape.source();
    assert!(shown.ends_with('…'), "{shown}");
    let lines = fixture
        .engine
        .layout(
            &truncated.shaped,
            Some(truncated.size.width),
            truncated.align,
        )
        .unwrap()
        .line_count();
    assert!(lines <= 2, "{shown}");
}

#[test]
fn direction_and_language_reach_shaping() {
    let fixture = Fixture::new();
    let mut spec = fixture.spec("abc אבג");
    assert!(
        !fixture
            .fit(&spec, AvailableSpace::MaxContent)
            .shaped
            .shape
            .is_rtl()
    );
    spec.options.direction = TextDirection::RightToLeft;
    assert!(
        fixture
            .fit(&spec, AvailableSpace::MaxContent)
            .shaped
            .shape
            .is_rtl()
    );

    let digits = |language: &str| {
        let mut spec = fixture.spec("۴۵۶۷");
        spec.options.language = Some(TextLanguage::parse(language).unwrap());
        let fitted = fixture.fit(&spec, AvailableSpace::MaxContent);
        let layout = fixture
            .engine
            .layout(&fitted.shaped, None, fitted.align)
            .unwrap();
        layout
            .runs()
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.id))
            .collect::<Vec<_>>()
    };
    assert_ne!(digits("fa"), digits("ur"));
}

#[test]
fn repeated_fits_and_layouts_reuse_cached_shaping() {
    let fixture = Fixture::new();
    let spec = nowrap(fixture.spec(FOX), Some(OverflowKey::End(ellipsis())));
    let first = fixture.fit(&spec, AvailableSpace::Definite(px(140.0)));
    let calls = fixture.shape_calls();
    let again = fixture.fit(&spec, AvailableSpace::Definite(px(140.0)));
    assert!(Arc::ptr_eq(&first.shaped.shape, &again.shaped.shape));
    let a = fixture
        .engine
        .layout(&first.shaped, None, first.align)
        .unwrap();
    let b = fixture
        .engine
        .layout(&again.shaped, None, again.align)
        .unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    // A different width reuses the full shape and only shapes new candidates.
    fixture.fit(&spec, AvailableSpace::Definite(px(200.0)));
    assert_eq!(
        fixture
            .fit(&spec, AvailableSpace::Definite(px(140.0)))
            .size
            .width,
        px(140.0)
    );
    assert!(fixture.shape_calls() >= calls);
    let after = fixture.shape_calls();
    fixture.fit(&spec, AvailableSpace::Definite(px(200.0)));
    assert_eq!(fixture.shape_calls(), after, "cached fit");
}

#[test]
fn lru_drops_the_older_half_when_full() {
    let mut lru = Lru::new(4);
    for key in 0..4 {
        lru.insert(key, key);
    }
    assert_eq!(lru.get(&0), Some(0)); // Refresh the oldest entry.
    lru.insert(4, 4);
    assert_eq!(lru.get(&0), Some(0));
    assert_eq!(lru.get(&4), Some(4));
    assert!(lru.entries.len() < 5);
}

fn draw_cells(visual: &mut VisualTestContext) {
    visual.draw(
        point(px(0.0), px(0.0)),
        size(
            AvailableSpace::Definite(px(400.0)),
            AvailableSpace::MinContent,
        ),
        |_, _| {
            let cell = |selector: &'static str, width: Pixels| {
                div()
                    .debug_selector(move || selector.into())
                    .w(width)
                    .font_family("IBM Plex Sans")
                    .text_size(px(16.0))
                    .line_height(LINE_HEIGHT)
            };
            div()
                .flex()
                .items_start()
                .child(
                    cell("cell", px(140.0))
                        .truncate()
                        .child(typeset(FOX).with_id("fox")),
                )
                .child(cell("rtl", px(200.0)).child(typeset("abc").rtl()))
                .child(cell("wrap", px(120.0)).child(typeset(FOX)))
        },
    );
}

#[test]
fn element_inherits_text_style_truncates_wraps_and_reuses_caches() {
    let parley = backend();
    let mut cx =
        TestAppContext::build_with_text_system(TestDispatcher::new(0), None, parley.clone());
    cx.update(|cx| install(parley.clone(), cx));
    let visual = cx.add_empty_window();
    draw_cells(visual);
    assert_eq!(
        visual.debug_bounds("cell").unwrap().size,
        size(px(140.0), LINE_HEIGHT),
        "truncated to the cell width on one line"
    );
    assert_eq!(visual.debug_bounds("rtl").unwrap().size.height, LINE_HEIGHT);
    let wrapped = visual.debug_bounds("wrap").unwrap();
    assert!(
        wrapped.size.height >= LINE_HEIGHT * 3.0,
        "wraps within its width"
    );
    // Drawing the same frame again hits the caches and does not reshape.
    let calls = parley.shape_calls();
    draw_cells(visual);
    assert_eq!(parley.shape_calls(), calls);
}

#[test]
fn unbroken_rtl_text_is_placed_at_the_right_edge() {
    let fixture = Fixture::new();
    let width = px(200.0);
    let mut spec = nowrap(fixture.spec("abc"), None);
    spec.options.direction = TextDirection::RightToLeft;
    let fitted = fixture.fit(&spec, AvailableSpace::Definite(width));
    let layout = fixture
        .engine
        .layout(&fitted.shaped, None, fitted.align)
        .unwrap();
    assert!(fitted.shaped.shape.is_rtl());
    // The layout itself starts at the left; the element adds the start offset.
    let first_x = layout.runs()[0].glyphs[0].position.x;
    assert!(first_x < px(1.0));
    let free = width - layout.content_width();
    assert_eq!(AlignKey::Start.offset(free, true), free);
    assert!(free > px(150.0));
}

#[test]
fn without_an_engine_the_element_renders_as_gpui_text() {
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        backend() as Arc<dyn gpui::PlatformTextSystem>,
    );
    let visual = cx.add_empty_window();
    visual.draw(
        point(px(0.0), px(0.0)),
        size(
            AvailableSpace::Definite(px(300.0)),
            AvailableSpace::MinContent,
        ),
        |_, _| {
            div()
                .debug_selector(|| "plain".into())
                .font_family("IBM Plex Sans")
                .text_size(px(16.0))
                .child(typeset("plain text"))
        },
    );
    let plain = visual.debug_bounds("plain").unwrap();
    assert!(plain.size.width > px(0.0) && plain.size.height > px(0.0));
}

#[test]
fn truncation_keeps_the_full_texts_auto_direction() {
    let fixture = Fixture::new();
    let width = AvailableSpace::Definite(px(150.0));
    // Left-to-right text whose start cut leaves Arabic first (the lab's mixed row),
    // and right-to-left text whose start cut leaves Latin first.
    for (text, rtl) in [
        ("office אבג 123 مرحبا — mixed cell label", false),
        ("שלום עולם and then a long Latin tail", true),
    ] {
        for overflow in [
            OverflowKey::Start(ellipsis()),
            OverflowKey::Middle(ellipsis()),
        ] {
            let spec = nowrap(fixture.spec(text), Some(overflow.clone()));
            let full = fixture.fit(&spec, AvailableSpace::MaxContent);
            assert_eq!(full.shaped.shape.is_rtl(), rtl, "{text}");
            let fitted = fixture.fit(&spec, width);
            let shown = fitted.shaped.shape.source();
            assert!(shown.contains('…'), "{shown}");
            assert_eq!(fitted.shaped.shape.is_rtl(), rtl, "{overflow:?}: {shown}");
        }
    }
    // An explicit direction is never replaced.
    let mut spec = nowrap(
        fixture.spec("office אבג 123 مرحبا — mixed cell label"),
        Some(OverflowKey::Start(ellipsis())),
    );
    spec.options.direction = TextDirection::RightToLeft;
    assert!(fixture.fit(&spec, width).shaped.shape.is_rtl());
}

#[test]
fn ltr_isolates_keep_values_with_units_together_in_rtl_text() {
    let fixture = Fixture::new();
    // Visual x of the first glyph for `target` in a single-line RTL layout.
    let x_of = |text: &str, target: char| {
        let mut spec = nowrap(fixture.spec(text), None);
        spec.options.direction = TextDirection::RightToLeft;
        let fitted = fixture.fit(&spec, AvailableSpace::MaxContent);
        let layout = fixture
            .engine
            .layout(&fitted.shaped, None, fitted.align)
            .unwrap();
        let index = text.find(target).unwrap();
        let glyphs: Vec<_> = layout.runs().iter().flat_map(|run| &run.glyphs).collect();
        assert!(
            glyphs.iter().all(|glyph| glyph.id.0 != 0),
            "no missing glyphs in {text:?}"
        );
        glyphs
            .iter()
            .find(|glyph| glyph.index == index)
            .unwrap()
            .position
            .x
    };
    // Unisolated, the bidi algorithm splits "536.6 us" around the space: "us 536.6".
    let plain = "ראשון 536.6 us";
    assert!(x_of(plain, 'u') < x_of(plain, '5'));
    // A left-to-right isolate (U+2066…U+2069) keeps the value in reading order.
    let isolated = "ראשון \u{2066}536.6 us\u{2069}";
    assert!(x_of(isolated, '5') < x_of(isolated, 'u'));
}

/// Inter is variable, so bold is a real instance (synthetic bold is unsupported).
fn inter(fixture: &Fixture, weight: gpui::FontWeight) -> FontId {
    let mut font = gpui::font("Inter Variable");
    font.weight = weight;
    fixture.engine.parley.font_id(&font).unwrap()
}

fn bold_font(fixture: &Fixture) -> FontId {
    inter(fixture, gpui::FontWeight::BOLD)
}

fn two_runs(fixture: &Fixture, text: &str, split: usize) -> TypesetSpec {
    let mut spec = fixture.spec(text);
    spec.fonts = Arc::new([
        FontRun {
            len: split,
            font_id: inter(fixture, gpui::FontWeight::NORMAL),
        },
        FontRun {
            len: text.len() - split,
            font_id: bold_font(fixture),
        },
    ]);
    spec
}

#[test]
fn styled_runs_shape_with_their_own_fonts_and_cache_separately() {
    let fixture = Fixture::new();
    let text = "regular then bold text";
    let mut plain = fixture.spec(text);
    plain.fonts = Arc::new([FontRun {
        len: text.len(),
        font_id: inter(&fixture, gpui::FontWeight::NORMAL),
    }]);
    let plain = fixture.fit(&nowrap(plain, None), AvailableSpace::MaxContent);
    let styled = fixture.fit(
        &nowrap(two_runs(&fixture, text, "regular ".len()), None),
        AvailableSpace::MaxContent,
    );
    assert!(!Arc::ptr_eq(&plain.shaped.shape, &styled.shaped.shape));
    assert!(
        styled.size.width > plain.size.width,
        "bold glyphs are wider"
    );
    let layout = fixture
        .engine
        .layout(&styled.shaped, None, styled.align)
        .unwrap();
    let fonts: Vec<_> = layout.runs().iter().map(|run| run.font_id).collect();
    assert!(fonts.len() >= 2 && fonts.windows(2).any(|pair| pair[0] != pair[1]));
}

#[test]
fn truncation_maps_shown_text_back_to_styled_runs() {
    let fixture = Fixture::new();
    let split = "The quick ".len();
    let bold = bold_font(&fixture);
    for overflow in [
        OverflowKey::End(ellipsis()),
        OverflowKey::Start(ellipsis()),
        OverflowKey::Middle(ellipsis()),
    ] {
        let spec = nowrap(two_runs(&fixture, FOX, split), Some(overflow.clone()));
        let fitted = fixture.fit(&spec, AvailableSpace::Definite(px(140.0)));
        let shown = fitted.shaped.shape.source().to_owned();
        let pieces = &fitted.pieces;
        // Pieces cover the shown text without gaps.
        let mut end = 0;
        for piece in pieces.iter() {
            assert_eq!(piece.shown.start, end, "{overflow:?}");
            end = piece.shown.end;
        }
        assert_eq!(end, shown.len());
        // Copied pieces reproduce the source bytes they claim.
        for piece in pieces.iter().filter(|piece| piece.copied) {
            assert_eq!(
                &shown[piece.shown.clone()],
                &FOX[piece.source..piece.source + piece.shown.len()]
            );
        }
        // Fonts follow the source runs: the affix takes its neighbor's font.
        let fonts = mapped_fonts(&spec.fonts, pieces);
        assert_eq!(fonts.iter().map(|run| run.len).sum::<usize>(), shown.len());
        let affix = shown.find('…').unwrap();
        let affix_font = font_at(&fonts, affix);
        assert_eq!(
            affix_font,
            font_at(&spec.fonts, source_index(pieces, affix))
        );
        // Every shown character keeps the font of the source character it maps to.
        for (index, _) in shown.char_indices() {
            assert_eq!(
                font_at(&fonts, index),
                font_at(&spec.fonts, source_index(pieces, index)),
                "{overflow:?}: {shown} at {index}"
            );
        }
        // Start and middle cuts keep the bold tail ("…lazy dog").
        if !matches!(overflow, OverflowKey::End(_)) {
            assert_eq!(font_at(&fonts, shown.len() - 1), bold, "{shown}");
        }
        // The bold source range maps to exactly the shown bytes in the bold font,
        // apart from an affix inside it.
        let bold_shown: usize = shown_ranges(pieces, split..FOX.len())
            .iter()
            .map(|range| range.len())
            .sum();
        let bold_bytes: usize = fonts
            .iter()
            .filter(|run| run.font_id == bold)
            .map(|run| run.len)
            .sum();
        assert_eq!(bold_shown, bold_bytes, "{overflow:?}: {shown}");
    }
}

#[test]
fn highlights_merge_over_the_inherited_style() {
    let base = gpui::TextStyle {
        color: gpui::black(),
        ..Default::default()
    };
    let red = gpui::red();
    let highlights = [
        (
            4..9,
            HighlightStyle {
                font_weight: Some(gpui::FontWeight::BOLD),
                ..Default::default()
            },
        ),
        (
            10..15,
            HighlightStyle {
                color: Some(red),
                underline: Some(UnderlineStyle {
                    thickness: px(1.0),
                    ..Default::default()
                }),
                background_color: Some(gpui::yellow()),
                ..Default::default()
            },
        ),
    ];
    let runs = typeset("The quick brown fox")
        .with_highlights(highlights)
        .source_runs(&base);
    let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
    assert_eq!(lens, [4, 5, 1, 5, 4]);
    assert_eq!(runs[1].font.weight, gpui::FontWeight::BOLD);
    assert_eq!(runs[0].font.weight, base.font_weight);
    assert_eq!(runs[3].color, base.color.blend(red));
    assert!(runs[3].underline.is_some() && runs[3].background_color.is_some());
    assert!(runs[2].underline.is_none());
    // Explicit runs replace the inherited style.
    let explicit = vec![
        base.to_run(3),
        base.clone()
            .highlight(HighlightStyle {
                color: Some(red),
                ..Default::default()
            })
            .to_run(16),
    ];
    assert_eq!(
        typeset("The quick brown fox")
            .with_runs(explicit.clone())
            .source_runs(&base),
        explicit
    );
}

const HEBREW: &[u8] = include_bytes!("../../tests/fonts/NotoSansHebrew.ttf");
const SHALOM: &str = "שלום עולם";

fn has_missing_glyphs(layout: &ParagraphLayout) -> bool {
    layout
        .runs()
        .iter()
        .flat_map(|run| &run.glyphs)
        .any(|glyph| glyph.id.0 == 0)
}

/// A backend without the Hebrew fallback its text style names.
fn backend_without_hebrew() -> Arc<ParleyTextSystem> {
    Arc::new(ParleyTextSystem::new(fonts(), "IBM Plex Sans").unwrap())
}

#[test]
fn adding_a_fallback_font_reshapes_cached_text() {
    let parley = backend_without_hebrew();
    let font_id = parley.font_id(&font()).unwrap();
    let fixture = Fixture {
        engine: TypesetEngine::new(parley.clone()),
        font: font_id,
    };
    let spec = nowrap(fixture.spec(SHALOM), None);
    let layout_of = |fitted: &Fitted| {
        fixture
            .engine
            .layout(&fitted.shaped, None, fitted.align)
            .unwrap()
    };
    let before = fixture.fit(&spec, AvailableSpace::MaxContent);
    assert!(
        has_missing_glyphs(&layout_of(&before)),
        "no Hebrew font yet"
    );

    parley.add_fonts(vec![HEBREW.into()]).unwrap();
    // The descriptor keeps its FontId, so only the font generation can tell.
    assert_eq!(parley.font_id(&font()).unwrap(), font_id);
    let after = fixture.fit(&spec, AvailableSpace::MaxContent);
    assert!(!Arc::ptr_eq(&before.shaped.shape, &after.shaped.shape));
    assert!(!has_missing_glyphs(&layout_of(&after)), "fallback applies");

    // The new results are cached again until the next font change.
    let calls = fixture.shape_calls();
    fixture.fit(&spec, AvailableSpace::MaxContent);
    assert_eq!(fixture.shape_calls(), calls);
}

#[test]
fn element_redrawn_after_font_loading_uses_the_new_fallback() {
    let parley = backend_without_hebrew();
    let mut cx =
        TestAppContext::build_with_text_system(TestDispatcher::new(0), None, parley.clone());
    cx.update(|cx| install(parley.clone(), cx));
    let visual = cx.add_empty_window();
    let draw = |visual: &mut VisualTestContext| {
        visual.draw(
            point(px(0.0), px(0.0)),
            size(
                AvailableSpace::Definite(px(400.0)),
                AvailableSpace::MinContent,
            ),
            |_, _| {
                div().flex().items_start().child(
                    div()
                        .debug_selector(|| "hebrew".into())
                        .font(font())
                        .text_size(px(16.0))
                        .line_height(LINE_HEIGHT)
                        .whitespace_nowrap()
                        .child(typeset(SHALOM)),
                )
            },
        );
        visual.debug_bounds("hebrew").unwrap().size.width
    };
    let missing = draw(visual);
    let calls = parley.shape_calls();
    visual.update(|_, cx| cx.text_system().add_fonts(vec![HEBREW.into()]).unwrap());
    let loaded = draw(visual);
    assert!(parley.shape_calls() > calls, "redraw reshapes");
    assert_ne!(missing, loaded, "Hebrew glyphs replace .notdef boxes");
}

#[test]
fn a_crlf_split_by_a_style_boundary_is_one_hard_break() {
    let style = gpui::TextStyle::default();
    let bold = style.clone().highlight(HighlightStyle {
        font_weight: Some(gpui::FontWeight::BOLD),
        ..Default::default()
    });
    // Boundaries between `\r` and `\n`, and a run holding only `\n`.
    for lens in [[4, 4].as_slice(), &[3, 1, 1, 3], &[3, 2, 3]] {
        let runs = lens
            .iter()
            .enumerate()
            .map(|(i, len)| if i % 2 == 0 { &style } else { &bold }.to_run(*len))
            .collect();
        let (text, runs) = hard_breaks(&"one\r\ntwo".into(), runs);
        assert_eq!(text.as_ref(), "one\u{2028}two", "{lens:?}");
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), text.len());
        assert!(runs.iter().all(|run| run.len > 0), "{lens:?}");
    }
    // The break takes the style of the run holding `\r`.
    let (_, runs) = hard_breaks(
        &"one\r\ntwo".into(),
        vec![style.to_run(3), bold.to_run(1), style.to_run(4)],
    );
    let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
    assert_eq!(lens, [3, LINE_SEPARATOR.len_utf8(), 3]);
    assert_eq!(runs[1].font.weight, gpui::FontWeight::BOLD);

    // A highlight never changes paragraph structure.
    let fixture = Fixture::new();
    let plain = typeset("one\r\ntwo").source_runs(&style);
    let highlighted = typeset("one\r\ntwo")
        .with_highlights([(3..4, HighlightStyle::default())])
        .source_runs(&style);
    for runs in [plain, highlighted] {
        let (text, _) = hard_breaks(&"one\r\ntwo".into(), runs);
        let fitted = fixture.fit(
            &nowrap(fixture.spec(&text), None),
            AvailableSpace::MaxContent,
        );
        assert_eq!(fitted.size.height, LINE_HEIGHT * 2.0);
    }
}

const THREE_LINES: &str = "one\u{2028}two\u{2028}three";

#[test]
fn unwrapped_text_honors_the_line_clamp() {
    let fixture = Fixture::new();
    let width = AvailableSpace::Definite(px(200.0));
    let mut spec = nowrap(fixture.spec(THREE_LINES), None);
    spec.line_clamp = Some(1);
    let clipped = fixture.fit(&spec, width);
    assert_eq!(clipped.size.height, LINE_HEIGHT, "one allowed line");
    assert_eq!(clipped.shaped.shape.source(), THREE_LINES, "no affix");
    // Paint masks to the allowed lines, not to the element's height.
    let layout = fixture
        .engine
        .layout(&clipped.shaped, None, clipped.align)
        .unwrap();
    assert_eq!(layout.line_count(), 3);
    assert_eq!(clamp_height(&layout, 1), Some(LINE_HEIGHT));
    assert_eq!(clamp_height(&layout, 2), Some(LINE_HEIGHT * 2.0));
    assert_eq!(clamp_height(&layout, 3), None, "nothing hidden");

    // With an overflow affix, hidden lines are dropped and the last kept line
    // ends in the affix, as for wrapped text.
    spec.overflow = Some(OverflowKey::End(ellipsis()));
    spec.line_clamp = Some(2);
    let truncated = fixture.fit(&spec, width);
    assert_eq!(truncated.shaped.shape.source(), "one\u{2028}two…");
    assert_eq!(truncated.size.height, LINE_HEIGHT * 2.0);
    for overflow in [
        OverflowKey::Start(ellipsis()),
        OverflowKey::Middle(ellipsis()),
    ] {
        spec.overflow = Some(overflow);
        let fitted = fixture.fit(&spec, width);
        assert_eq!(fitted.shaped.shape.source(), "one\u{2028}two…");
    }
}

/// Inline start and advance of each laid-out line.
fn line_extents(layout: &ParagraphLayout) -> Vec<(f32, f32)> {
    layout
        .parley_layout()
        .lines()
        .map(|line| (line.metrics().offset, line.metrics().advance))
        .collect()
}

#[test]
fn unwrapped_lines_align_individually() {
    let fixture = Fixture::new();
    let layout_for = |text: &str, align, direction| {
        let mut spec = nowrap(fixture.spec(text), None);
        spec.align = align;
        spec.options.direction = direction;
        let fitted = fixture.fit(&spec, AvailableSpace::MaxContent);
        fixture
            .engine
            .layout(&fitted.shaped, None, fitted.align)
            .unwrap()
    };
    let close = |a: f32, b: f32| (a - b).abs() < 0.5;

    let centered = layout_for(
        "long heading\u{2028}x",
        AlignKey::Center,
        TextDirection::Auto,
    );
    let width = f32::from(centered.content_width());
    let lines = line_extents(&centered);
    for (offset, advance) in &lines {
        assert!(close(offset + advance / 2.0, width / 2.0), "{lines:?}");
    }
    assert!(lines[1].0 > lines[0].0 + 10.0, "short line is inset");

    let rtl = layout_for(
        "שלום עולם ארוך\u{2028}אב",
        AlignKey::Start,
        TextDirection::RightToLeft,
    );
    let width = f32::from(rtl.content_width());
    let lines = line_extents(&rtl);
    for (offset, advance) in &lines {
        assert!(close(offset + advance, width), "right edge: {lines:?}");
    }

    let ltr_end = layout_for("long heading\u{2028}x", AlignKey::End, TextDirection::Auto);
    let width = f32::from(ltr_end.content_width());
    for (offset, advance) in line_extents(&ltr_end) {
        assert!(close(offset + advance, width));
    }
    // Start-aligned left-to-right lines share the left edge.
    let ltr = layout_for(
        "long heading\u{2028}x",
        AlignKey::Start,
        TextDirection::Auto,
    );
    assert!(
        line_extents(&ltr)
            .iter()
            .all(|(offset, _)| close(*offset, 0.0))
    );
}

#[test]
fn unwrapped_truncation_cuts_each_hard_line_on_its_own() {
    let fixture = Fixture::new();
    let limit = px(140.0);
    let source = "short\u{2028}very long second line that overflows";
    let cases = [
        (OverflowKey::End(ellipsis()), "short\u{2028}very", None),
        (
            OverflowKey::Start(ellipsis()),
            "short\u{2028}…",
            Some("overflows"),
        ),
        (
            OverflowKey::Middle(ellipsis()),
            "short\u{2028}very",
            Some("flows"),
        ),
    ];
    for (overflow, prefix, suffix) in cases {
        let spec = nowrap(fixture.spec(source), Some(overflow.clone()));
        let fitted = fixture.fit(&spec, AvailableSpace::Definite(limit));
        let shown = fitted.shaped.shape.source();
        assert!(shown.starts_with(prefix), "{overflow:?}: {shown}");
        if let Some(suffix) = suffix {
            assert!(shown.ends_with(suffix), "{overflow:?}: {shown}");
        }
        assert_eq!(shown.matches('…').count(), 1, "only the long line: {shown}");
        assert_eq!(fitted.size, size(limit, LINE_HEIGHT * 2.0));
        let layout = fixture
            .engine
            .layout(&fitted.shaped, None, fitted.align)
            .unwrap();
        assert_eq!(layout.line_count(), 2);
        assert!(layout.content_width() <= limit, "{shown}");
        // Pieces cover the shown text and copy the bytes they claim.
        let mut end = 0;
        for piece in fitted.pieces.iter() {
            assert_eq!(piece.shown.start, end);
            end = piece.shown.end;
            if piece.copied {
                assert_eq!(
                    &shown[piece.shown.clone()],
                    &source[piece.source..piece.source + piece.shown.len()]
                );
            }
        }
        assert_eq!(end, shown.len());
    }
    // Every line too long: each keeps its own start.
    let spec = nowrap(
        fixture.spec(&format!("{FOX}\u{2028}{FOX}")),
        Some(OverflowKey::End(ellipsis())),
    );
    let shown = fixture.fit(&spec, AvailableSpace::Definite(limit));
    let shown = shown.shaped.shape.source();
    let lines: Vec<_> = shown.split('\u{2028}').collect();
    assert_eq!(lines.len(), 2, "{shown}");
    assert!(
        lines
            .iter()
            .all(|line| line.starts_with("The") && line.ends_with('…'))
    );
}

fn draw_styled(visual: &mut VisualTestContext) {
    visual.draw(
        point(px(0.0), px(0.0)),
        size(
            AvailableSpace::Definite(px(600.0)),
            AvailableSpace::MinContent,
        ),
        |_, _| {
            let bold = HighlightStyle {
                font_weight: Some(gpui::FontWeight::BOLD),
                ..Default::default()
            };
            let decorated = HighlightStyle {
                color: Some(gpui::red()),
                background_color: Some(gpui::yellow()),
                underline: Some(UnderlineStyle {
                    thickness: px(1.0),
                    ..Default::default()
                }),
                strikethrough: Some(gpui::StrikethroughStyle {
                    thickness: px(1.0),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let cell = |selector: &'static str| {
                div()
                    .debug_selector(move || selector.into())
                    .font_family("Inter Variable")
                    .text_size(px(16.0))
                    .line_height(LINE_HEIGHT)
                    .whitespace_nowrap()
            };
            div()
                .flex()
                .flex_col()
                .items_start()
                .child(cell("plain").child(typeset(FOX)))
                .child(cell("bold").child(typeset(FOX).with_highlights([(4..15, bold)])))
                .child(
                    cell("decorated")
                        .w(px(140.0))
                        .truncate()
                        .child(typeset(FOX).with_highlights([(4..9, bold), (10..15, decorated)])),
                )
        },
    );
}

#[test]
fn element_shapes_and_paints_highlighted_runs() {
    let parley = backend();
    let mut cx =
        TestAppContext::build_with_text_system(TestDispatcher::new(0), None, parley.clone());
    cx.update(|cx| install(parley.clone(), cx));
    let visual = cx.add_empty_window();
    draw_styled(visual);
    let plain = visual.debug_bounds("plain").unwrap().size;
    let bold = visual.debug_bounds("bold").unwrap().size;
    assert!(bold.width > plain.width, "bold highlight reaches shaping");
    assert_eq!(bold.height, plain.height);
    assert_eq!(
        visual.debug_bounds("decorated").unwrap().size,
        size(px(140.0), LINE_HEIGHT),
        "decorated, truncated cell paints without errors"
    );
    // Without an engine the same highlights render through StyledText.
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        backend() as Arc<dyn gpui::PlatformTextSystem>,
    );
    let visual = cx.add_empty_window();
    draw_styled(visual);
    let fallback = visual.debug_bounds("bold").unwrap().size;
    assert!(fallback.width > visual.debug_bounds("plain").unwrap().size.width);
}
