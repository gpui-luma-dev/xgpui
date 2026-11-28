//! Local checks for gpui-luma-dev/xgpui#332. GPUI's own flat lines wrap
//! right-to-left text in painting order; with Parley and
//! `xgpui_parley::install_text_layout`, plain GPUI text wraps in logical order.
//!
//! A failure here means the right-to-left story changed, not necessarily that
//! something broke:
//! - `plain_text_wraps_rtl_in_painting_order_*`: GPUI's flat lines (or
//!   upstream) changed how right-to-left lines wrap.
//! - `install_text_layout_wraps_plain_rtl_in_logical_order`: plain text on the
//!   Parley engine no longer wraps in logical order.
//! - `typeset_covers_styled_text_api`: `StyledText`, `TextStyle` or
//!   `HighlightStyle` changed, so `typeset()` may have fallen behind.
//! - `bidi_page_wrapped_section_*`: the bidi lab's section 5 no longer shows
//!   what it documents.
use super::{bidi, paragraph, samples};
use gpui::{
    FontRun, ParagraphPosition, PlatformTextSystem, StyledText, TestAppContext, TestDispatcher,
    TextAffinity, VisualTestContext, div, point, prelude::*, px, size,
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use xgpui_parley::ParleyTextSystem;

const FAMILY: &str = "Noto Sans Hebrew";

fn parley() -> Arc<ParleyTextSystem> {
    let mut fonts = parley::FontContext::new();
    for data in std::iter::once(samples::PLEX).chain(paragraph::FONTS) {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
    }
    Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans").unwrap())
}

#[cfg(target_os = "macos")]
fn native() -> Arc<dyn PlatformTextSystem> {
    let backend: Arc<dyn PlatformTextSystem> = Arc::new(gpui_apple::AppleTextSystem::new());
    backend
        .add_fonts(
            std::iter::once(samples::PLEX)
                .chain(paragraph::FONTS)
                .map(std::borrow::Cow::Borrowed)
                .collect(),
        )
        .unwrap();
    backend
}

#[cfg(target_os = "macos")]
#[test]
fn native_flat_hebrew_label_hit_testing() {
    let text = "\u{05e9}\u{05dc}\u{05d5}\u{05dd}";
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, native());
    let window = cx.add_empty_window();
    let styled = StyledText::new(text);
    let layout = styled.layout().clone();
    let element = Mutex::new(Some(styled));
    window.draw(point(px(0.), px(0.)), size(px(300.), px(50.)), |_, _| {
        div()
            .font_family(FAMILY)
            .text_size(px(20.))
            .child(element.lock().unwrap().take().unwrap())
    });
    let start = layout.position_for_index(0).unwrap();
    let end = layout.position_for_index(text.len()).unwrap();
    assert!(start.x > end.x);
    let line = layout.line_layout_for_index(0).unwrap();
    let origin = layout.bounds().origin;
    assert_eq!(
        line.unwrapped_layout
            .closest_index_for_x(start.x - origin.x),
        0
    );
    assert_eq!(
        line.unwrapped_layout.closest_index_for_x(end.x - origin.x),
        text.len()
    );
    assert_eq!(
        layout.index_for_position(point(start.x - px(1.), start.y)),
        Ok(0)
    );
}

/// Plain GPUI text breaks right-to-left lines on the visual (painted) glyph
/// order, so its wrap boundaries run backwards through the text and the
/// sentence's first words land on the last line. The paragraph API, which
/// `typeset()` uses, breaks in logical order.
fn wrap_order_checks(backend: Arc<dyn PlatformTextSystem>) {
    let text = bidi::WRAPPED;
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend);
    let window = cx.add_empty_window();
    let styled = StyledText::new(text);
    let layout = styled.layout().clone();
    let element = Mutex::new(Some(styled));
    window.draw(point(px(0.), px(0.)), size(px(600.), px(600.)), |_, _| {
        div()
            .w(bidi::WRAP_WIDTH)
            .font_family(FAMILY)
            .text_size(px(15.))
            .child(element.lock().unwrap().take().unwrap())
    });

    // Plain text: byte offsets of the soft-wrap boundaries, in line order.
    let lines = layout.line_layouts();
    assert_eq!(lines.len(), 1, "one hard line");
    let line = &lines[0];
    let boundaries: Vec<usize> = line
        .wrap_boundaries
        .iter()
        .map(|b| line.unwrapped_layout.runs[b.run_ix].glyphs[b.glyph_ix].index)
        .collect();
    assert!(
        boundaries.len() >= 2,
        "the sample must wrap to at least three lines: {boundaries:?}"
    );
    assert!(
        boundaries.windows(2).all(|pair| pair[0] > pair[1]),
        "plain GPUI text now wraps right-to-left text in another order \
         (boundaries {boundaries:?}); revisit gpui-luma-dev/xgpui#332"
    );
    // `wrapped_text` takes those boundaries in text order instead of
    // panicking on them: one logical span per line, covering the whole text.
    let wrapped = layout.wrapped_text();
    assert_eq!(wrapped.lines().count(), boundaries.len() + 1, "{wrapped:?}");
    assert_eq!(wrapped.replace('\n', ""), text);
    assert!(wrapped.starts_with(text.split(' ').next().unwrap()));

    // Paragraph API: logical order, first character at the top right.
    window.update(|window, _| {
        let system = window.text_system();
        let runs = [FontRun {
            len: text.len(),
            font_id: system.resolve_font(&gpui::font(FAMILY)),
        }];
        let paragraph = system
            .layout_paragraph(text.into(), px(15.), &runs, bidi::WRAP_WIDTH)
            .unwrap();
        let caret = |byte_index| {
            paragraph
                .caret_bounds(ParagraphPosition {
                    byte_index,
                    affinity: TextAffinity::Downstream,
                })
                .unwrap()
                .origin
        };
        let first = caret(0);
        assert_eq!(first.y, px(0.), "paragraph starts on the first line");
        assert!(
            first.x > bidi::WRAP_WIDTH / 2.,
            "and at the right edge: {first:?}"
        );
        let words: Vec<_> = std::iter::once(0)
            .chain(text.match_indices(' ').map(|(ix, _)| ix + 1))
            .map(|ix| caret(ix).y)
            .collect();
        assert!(
            words.windows(2).all(|pair| pair[0] <= pair[1]),
            "paragraph lines follow logical order: {words:?}"
        );
        assert!(words.last().unwrap() > &px(0.), "paragraph wraps");
    });
}

#[test]
fn plain_text_wraps_rtl_in_painting_order_parley() {
    wrap_order_checks(parley());
}

#[cfg(target_os = "macos")]
#[test]
fn plain_text_wraps_rtl_in_painting_order_native() {
    wrap_order_checks(native());
}

/// Draws `text` as plain GPUI text in the section 5 width and returns its layout.
fn plain_layout(visual: &mut VisualTestContext, text: &'static str) -> gpui::TextLayout {
    let element = StyledText::new(text);
    let layout = element.layout().clone();
    let element = Mutex::new(Some(element));
    visual.draw(point(px(0.), px(0.)), size(px(600.), px(600.)), |_, _| {
        div().flex().items_start().child(
            div()
                .w(bidi::WRAP_WIDTH)
                .font_family(FAMILY)
                .text_size(px(15.))
                .child(element.lock().unwrap().take().unwrap()),
        )
    });
    layout
}

/// With the Parley engine laying out plain text, the first words start the
/// first line at the right edge and lines follow logical order.
#[test]
fn install_text_layout_wraps_plain_rtl_in_logical_order() {
    let text = bidi::WRAPPED;
    let parley = parley();
    let mut cx =
        TestAppContext::build_with_text_system(TestDispatcher::new(0), None, parley.clone());
    cx.update(|cx| xgpui_parley::install_text_layout(parley, cx));
    let visual = cx.add_empty_window();
    let layout = plain_layout(visual, text);
    let bounds = layout.bounds();
    assert!(layout.line_layouts().is_empty(), "laid out by the provider");
    let first = layout.position_for_index(0).unwrap();
    assert_eq!(first.y, bounds.top(), "first words on the first line");
    assert!(first.x > bounds.center().x, "at the right edge: {first:?}");
    let tops: Vec<_> = std::iter::once(0)
        .chain(text.match_indices(' ').map(|(ix, _)| ix + 1))
        .map(|ix| layout.position_for_index(ix).unwrap().y)
        .collect();
    assert!(tops.windows(2).all(|pair| pair[0] <= pair[1]), "{tops:?}");
    assert!(tops.last().unwrap() > &bounds.top(), "wraps");
    let first_word = text.split(' ').next().unwrap();
    assert!(layout.wrapped_text().starts_with(first_word));
}

/// How `typeset()` covers one public `StyledText` method or text style field.
enum Coverage {
    /// `typeset()` supports it; the text says how.
    Typeset(&'static str),
    /// A known gap: apps that need it use plain GPUI text or the paragraph API.
    Gap(&'static str),
}
use Coverage::{Gap, Typeset};

const STYLED_TEXT: &[(&str, Coverage)] = &[
    ("new", Typeset("typeset(text)")),
    (
        "layout",
        Gap("no index/pixel mapping; use the paragraph API for hit testing and carets"),
    ),
    (
        "with_default_highlights",
        Typeset("with_runs, built from the style and highlights"),
    ),
    ("with_highlights", Typeset("with_highlights")),
    (
        "with_font_family_overrides",
        Gap("not supported; use with_runs with per-run fonts"),
    ),
    ("with_runs", Typeset("with_runs")),
];

const TEXT_STYLE: &[(&str, Coverage)] = &[
    ("color", Typeset("inherited")),
    ("font_family", Typeset("inherited, resolved per run")),
    (
        "font_features",
        Typeset("inherited, part of the resolved font"),
    ),
    (
        "font_variations",
        Typeset("inherited, part of the resolved font"),
    ),
    (
        "font_fallbacks",
        Typeset("inherited, part of the resolved font"),
    ),
    ("font_size", Typeset("inherited")),
    ("line_height", Typeset("inherited")),
    ("font_weight", Typeset("inherited")),
    ("font_style", Typeset("inherited")),
    ("background_color", Typeset("inherited")),
    ("underline", Typeset("inherited")),
    ("strikethrough", Typeset("inherited")),
    ("white_space", Typeset("inherited")),
    ("text_overflow", Typeset("inherited")),
    (
        "text_align",
        Typeset("inherited, left/right read as start/end"),
    ),
    ("line_clamp", Typeset("inherited")),
];

const HIGHLIGHT_STYLE: &[(&str, Coverage)] = &[
    ("color", Typeset("with_highlights")),
    ("font_weight", Typeset("with_highlights")),
    ("font_style", Typeset("with_highlights")),
    ("background_color", Typeset("with_highlights")),
    ("underline", Typeset("with_highlights")),
    ("strikethrough", Typeset("with_highlights")),
    ("fade_out", Typeset("with_highlights")),
];

/// Lines inside the brace block that starts at `header`, at depth 1.
fn block_lines<'a>(source: &'a str, header: &str) -> Vec<&'a str> {
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("`{header}` not found; update rtl_checks"));
    let mut depth = 0i32;
    let mut lines = Vec::new();
    for line in source[start..].lines() {
        if depth == 1 {
            lines.push(line);
        }
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        if depth <= 0 && !lines.is_empty() {
            break;
        }
    }
    lines
}

fn public_names(source: &str, header: &str, prefix: &str) -> BTreeSet<String> {
    block_lines(source, header)
        .into_iter()
        .filter_map(|line| line.trim_start().strip_prefix(prefix))
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect()
        })
        .collect()
}

fn check_table(what: &str, found: BTreeSet<String>, table: &[(&str, Coverage)]) -> Vec<String> {
    let listed: BTreeSet<String> = table.iter().map(|(name, _)| name.to_string()).collect();
    let mut problems = Vec::new();
    for name in found.difference(&listed) {
        problems.push(format!("{what}: new `{name}`; does typeset() need it?"));
    }
    for name in listed.difference(&found) {
        problems.push(format!("{what}: `{name}` is gone; update the table"));
    }
    problems
}

/// Upstream changes to `StyledText`'s builder methods or to the text style
/// types fail this test, so `typeset()` can't fall behind unnoticed. Known gaps
/// are listed with `Gap` and printed.
#[test]
fn typeset_covers_styled_text_api() {
    let text = include_str!("../../../../gpui/crates/gpui/src/elements/text.rs");
    let style = include_str!("../../../../gpui/crates/gpui/src/style.rs");
    let mut problems = Vec::new();
    problems.extend(check_table(
        "StyledText",
        public_names(text, "impl StyledText {", "pub fn "),
        STYLED_TEXT,
    ));
    problems.extend(check_table(
        "TextStyle",
        public_names(style, "pub struct TextStyle {", "pub "),
        TEXT_STYLE,
    ));
    problems.extend(check_table(
        "HighlightStyle",
        public_names(style, "pub struct HighlightStyle {", "pub "),
        HIGHLIGHT_STYLE,
    ));
    for (table, entries) in [
        ("StyledText", STYLED_TEXT),
        ("TextStyle", TEXT_STYLE),
        ("HighlightStyle", HIGHLIGHT_STYLE),
    ] {
        for (name, coverage) in entries {
            match coverage {
                Typeset(how) => println!("typeset() covers {table}::{name}: {how}"),
                Gap(why) => println!("known typeset() gap: {table}::{name}: {why}"),
            }
        }
    }
    assert!(
        problems.is_empty(),
        "StyledText/text style API changed; revisit gpui-luma-dev/xgpui#332:\n{}",
        problems.join("\n")
    );
}

type SectionSizes = Box<dyn Fn(&str) -> gpui::Size<gpui::Pixels>>;

/// Draws the bidi lab, after `adjust`, and returns section 5's frame sizes and
/// the window for further drawing.
fn wrapped_section(
    backend: Arc<dyn PlatformTextSystem>,
    parley: Option<Arc<ParleyTextSystem>>,
    adjust: impl FnOnce(&mut bidi::BidiLab),
) -> (SectionSizes, VisualTestContext) {
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend);
    let window = cx.open_window(size(px(900.), px(2400.)), |_, cx| {
        let mut lab = bidi::BidiLab::new("test".into(), parley, cx);
        adjust(&mut lab);
        lab
    });
    let lab = window.root(&mut cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    visual.draw(point(px(0.), px(0.)), size(px(900.), px(2400.)), |_, _| {
        div().size_full().child(lab.clone())
    });
    let sizes: Vec<_> = ["wrap", "clamp"]
        .into_iter()
        .flat_map(|id| ["plain", "typeset"].map(move |kind| format!("rtl-{id}-{kind}")))
        .map(|selector| {
            let bounds = visual
                .debug_bounds(selector.clone().leak())
                .unwrap_or_else(|| panic!("{selector} not drawn"));
            (selector, bounds.size)
        })
        .collect();
    let size = move |selector: &str| sizes.iter().find(|(name, _)| name == selector).unwrap().1;
    (Box::new(size), visual)
}

#[test]
fn bidi_page_wrapped_section_parley() {
    // Default: plain text on the Parley engine, so both columns match.
    let parley = parley();
    let (size, _) = wrapped_section(parley.clone(), Some(parley.clone()), |_| {});
    for id in ["wrap", "clamp"] {
        assert_eq!(
            size(&format!("rtl-{id}-plain")),
            size(&format!("rtl-{id}-typeset")),
            "{id}"
        );
        assert_eq!(size(&format!("rtl-{id}-plain")).width, bidi::WRAP_WIDTH);
    }
    assert!(size("rtl-wrap-typeset").height > size("rtl-clamp-typeset").height);

    // Switched to GPUI lines: plain text is back on flat lines.
    let (size, mut visual) = wrapped_section(parley.clone(), Some(parley), |lab| {
        lab.set_plain_on_parley(false)
    });
    for kind in ["plain", "typeset"] {
        let wrap = size(&format!("rtl-wrap-{kind}"));
        let clamp = size(&format!("rtl-clamp-{kind}"));
        assert_eq!(wrap.width, bidi::WRAP_WIDTH, "{kind}");
        assert!(clamp.height < wrap.height, "{kind}: clamp is shorter");
    }
    let layout = plain_layout(&mut visual, bidi::WRAPPED);
    assert!(!layout.line_layouts().is_empty(), "flat lines");
}

#[cfg(target_os = "macos")]
#[test]
fn bidi_page_wrapped_section_native() {
    // Without a Parley engine, typeset() renders as plain GPUI text.
    let (size, _) = wrapped_section(native(), None, |_| {});
    for id in ["wrap", "clamp"] {
        assert_eq!(
            size(&format!("rtl-{id}-plain")),
            size(&format!("rtl-{id}-typeset")),
            "{id}"
        );
    }
}
