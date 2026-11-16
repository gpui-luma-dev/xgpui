//! Right-to-left and mixed-direction single lines through GPUI's flat-line path (#325 B).
use super::*;
use crate::test_support::fonts;
use gpui::{
    Font, FontFallbacks, PlatformParagraphLayout as _, TextRun, TextSystem, TruncateFrom,
    WindowTextSystem, font,
};

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

fn label_font() -> Font {
    let mut font = font("IBM Plex Sans");
    font.fallbacks = Some(FontFallbacks::from_fonts(vec![
        "Noto Sans Hebrew".into(),
        "Noto Sans Arabic".into(),
    ]));
    font
}

fn runs(text: &str) -> [TextRun; 1] {
    [TextRun {
        len: text.len(),
        font: label_font(),
        ..Default::default()
    }]
}

/// `(logical index, x)` for every glyph in paint order.
fn glyphs(runs: &[gpui::ShapedRun]) -> Vec<(usize, f32)> {
    runs.iter()
        .flat_map(|run| run.glyphs.iter())
        .map(|glyph| (glyph.index, f32::from(glyph.position.x)))
        .collect()
}

const LABELS: [&str; 4] = ["עברית", "مرحبا بالعالم", "abc עברית 123", "שלום, world!"];

#[test]
fn rtl_and_mixed_labels_lay_out_in_visual_order_like_paragraphs() {
    let backend = backend();
    let window = WindowTextSystem::new(Arc::new(TextSystem::new(backend.clone())));
    for text in LABELS {
        let line = window.layout_line(text, px(16.0), &runs(text), None);
        let flat = glyphs(&line.runs);
        assert!(!flat.is_empty(), "{text}");
        // Real glyphs from the fallback fonts, painted left to right.
        assert!(
            line.runs
                .iter()
                .flat_map(|run| run.glyphs.iter())
                .all(|glyph| glyph.id.0 != 0),
            "{text}: missing glyph"
        );
        assert!(
            flat.windows(2).all(|pair| pair[0].1 <= pair[1].1),
            "{text}: not in visual order: {flat:?}"
        );
        // Parity with Parley's own paragraph engine on one unbroken line.
        let font_id = backend.font_id(&label_font()).unwrap();
        let shape = backend
            .prepare_paragraph(
                text,
                px(16.0),
                &[FontRun {
                    len: text.len(),
                    font_id,
                }],
            )
            .unwrap();
        let (_, max) = shape.content_widths();
        let paragraph = shape.layout(max + px(1.0)).unwrap();
        let expected = glyphs(paragraph.runs());
        assert_eq!(flat.len(), expected.len(), "{text}");
        // An RTL paragraph is start-aligned to the right of its slack, so compare
        // positions relative to the first glyph.
        let (x0, expected_x0) = (flat[0].1, expected[0].1);
        for ((index, x), (expected_index, expected_x)) in flat.iter().zip(&expected) {
            assert_eq!(index, expected_index, "{text}");
            let (x, expected_x) = (x - x0, expected_x - expected_x0);
            assert!((x - expected_x).abs() < 0.01, "{text}: {x} vs {expected_x}");
        }
        assert!((line.width - max).abs() < px(0.01), "{text}");
    }
}

#[test]
fn pure_rtl_labels_report_descending_logical_indices() {
    let backend = backend();
    let window = WindowTextSystem::new(Arc::new(TextSystem::new(backend)));
    let text = "עברית";
    let line = window.layout_line(text, px(16.0), &runs(text), None);
    let indices: Vec<_> = glyphs(&line.runs).into_iter().map(|(i, _)| i).collect();
    let mut logical: Vec<_> = text.char_indices().map(|(i, _)| i).collect();
    logical.reverse();
    assert_eq!(indices, logical);
    // index_for_x finds the cluster under a point regardless of glyph order:
    // the leftmost glyph is the last letter.
    assert_eq!(line.index_for_x(px(1.0)), Some(*logical.first().unwrap()));
}

#[test]
fn end_truncation_drops_the_logical_end_and_the_ellipsis_follows_direction() {
    let backend = backend();
    let text_system = Arc::new(TextSystem::new(backend));
    let window = WindowTextSystem::new(text_system.clone());
    for (text, rtl) in [
        ("עברית ארוכה מאוד מאוד", true),
        ("a rather long label here", false),
    ] {
        let mut wrapper = text_system.line_wrapper(label_font(), px(16.0));
        let text_runs = runs(text);
        let full = window.layout_line(text, px(16.0), &text_runs, None);
        let (truncated, truncated_runs) = wrapper.truncate_line(
            text.into(),
            full.width / 2.0,
            "…",
            &text_runs,
            TruncateFrom::End,
        );
        assert!(truncated.ends_with('…'), "{truncated}");
        let kept = truncated.trim_end_matches('…');
        assert!(
            text.starts_with(kept),
            "{truncated} keeps the logical start"
        );
        let line = window.layout_line(&truncated, px(16.0), &truncated_runs, None);
        assert!(line.width <= full.width / 2.0 + px(1.0), "{truncated}");
        let ellipsis = truncated.len() - '…'.len_utf8();
        let order = glyphs(&line.runs);
        let position = order
            .iter()
            .position(|(index, _)| *index == ellipsis)
            .unwrap();
        // RTL: the logical end (and its ellipsis) is the visual left edge.
        if rtl {
            assert_eq!(position, 0, "{truncated}: {order:?}");
        } else {
            assert_eq!(position, order.len() - 1, "{truncated}: {order:?}");
        }
    }
}
