//! Local checks for the conditions behind gpui-luma-dev/xgpui#332: plain GPUI
//! text stays on flat lines, and apps use `typeset()` or the paragraph API for
//! wrapped right-to-left text.
//!
//! A failure here means that decision needs another look, not necessarily that
//! something broke:
//! - `plain_text_wraps_rtl_in_painting_order_*`: plain GPUI text (or upstream)
//!   changed how right-to-left lines wrap.
//! - `typeset_covers_styled_text_api`: `StyledText`, `TextStyle` or
//!   `HighlightStyle` changed, so `typeset()` may have fallen behind.
//! - `bidi_page_wrapped_section_*`: the bidi lab's section 5 no longer shows the
//!   difference it documents.
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
    // (`TextLayout::wrapped_text` slices with these and panics on them.)
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

/// Draws the bidi lab and returns section 5's frame sizes.
fn wrapped_section(
    backend: Arc<dyn PlatformTextSystem>,
    parley: Option<Arc<ParleyTextSystem>>,
) -> impl Fn(&str) -> gpui::Size<gpui::Pixels> {
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend);
    let window = cx.open_window(size(px(900.), px(2400.)), |_, cx| {
        bidi::BidiLab::new("test".into(), parley, cx)
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
    move |selector| sizes.iter().find(|(name, _)| name == selector).unwrap().1
}

#[test]
fn bidi_page_wrapped_section_parley() {
    let parley = parley();
    let size = wrapped_section(parley.clone(), Some(parley));
    for kind in ["plain", "typeset"] {
        let wrap = size(&format!("rtl-wrap-{kind}"));
        let clamp = size(&format!("rtl-clamp-{kind}"));
        assert_eq!(wrap.width, bidi::WRAP_WIDTH, "{kind}");
        assert!(clamp.height < wrap.height, "{kind}: clamp is shorter");
    }
    // Both wrap the same words into the same number of lines here; only the
    // line order differs, which the wrap-order test checks.
    assert!(size("rtl-wrap-typeset").height > size("rtl-clamp-typeset").height);
}

#[cfg(target_os = "macos")]
#[test]
fn bidi_page_wrapped_section_native() {
    // Without a Parley engine, typeset() renders as plain GPUI text.
    let size = wrapped_section(native(), None);
    for id in ["wrap", "clamp"] {
        assert_eq!(
            size(&format!("rtl-{id}-plain")),
            size(&format!("rtl-{id}-typeset")),
            "{id}"
        );
    }
}
