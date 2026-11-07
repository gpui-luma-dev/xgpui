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
            font: self.font,
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
        .shape(&spec, &spec.text)
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
    let text = hard_breaks(&"one\ntwo\r\nthree".into());
    assert_eq!(text.as_ref(), "one\u{2028}two\u{2028}three");
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
