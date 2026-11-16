use super::*;
use crate::{
    NoopTextSystem, ParagraphShapeOptions, ParagraphSupport, PlatformTextSystem, TextDirection,
    TextSystem, WindowTextSystem,
};

// Noop shaping at font size 10: every character advances 6px, ascent is 10.25px
// and a line is 13px tall.
const SIZE: Pixels = px(10.0);
const CHAR: f32 = 6.0;
const LINE: f32 = 13.0;

fn shape(text: &str) -> Arc<dyn PlatformParagraphShape> {
    let noop = NoopTextSystem::new();
    let options = ParagraphShapeOptions::default();
    noop.shape_paragraph(ParagraphRequest::new(text, SIZE, &[], &options))
        .unwrap()
}

fn layout(text: &str, width: f32) -> Arc<dyn PlatformParagraphLayout> {
    shape(text).layout(px(width)).unwrap()
}

fn down(byte_index: usize) -> ParagraphPosition {
    ParagraphPosition {
        byte_index,
        affinity: TextAffinity::Downstream,
    }
}

fn up(byte_index: usize) -> ParagraphPosition {
    ParagraphPosition {
        byte_index,
        affinity: TextAffinity::Upstream,
    }
}

fn line_starts(layout: &dyn PlatformParagraphLayout) -> Vec<usize> {
    let mut starts: Vec<usize> = layout
        .runs()
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .filter(|glyph| glyph.position.x == px(0.0))
        .map(|glyph| glyph.index)
        .collect();
    starts.dedup();
    starts
}

#[test]
fn wraps_at_spaces_with_trailing_space_hanging() {
    // "aaa bbb ccc" is 66px; at 45px "aaa bbb " fits (42px without its space).
    let layout = layout("aaa bbb ccc", 45.0);
    assert_eq!(layout.size(), size(px(45.0), px(2.0 * LINE)));
    assert_eq!(line_starts(&*layout), vec![0, 8]);
    let c = layout
        .runs()
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .find(|glyph| glyph.index == 9)
        .unwrap();
    assert_eq!(c.position, point(px(CHAR), px(LINE + 10.25)));
}

#[test]
fn soft_wrap_boundary_is_shared_and_affinity_picks_the_line() {
    let layout = layout("aaa bbb ccc", 45.0);
    let end_of_first = layout.caret_bounds(up(8)).unwrap();
    assert_eq!(end_of_first.origin, point(px(8.0 * CHAR), px(0.0)));
    assert_eq!(end_of_first.size, size(px(1.0), px(LINE)));
    let start_of_second = layout.caret_bounds(down(8)).unwrap();
    assert_eq!(start_of_second.origin, point(px(0.0), px(LINE)));
    // Clicking past the end of the first line lands there, upstream.
    assert_eq!(layout.hit_test(point(px(100.0), px(5.0))).unwrap(), up(8));
    assert_eq!(layout.hit_test(point(px(0.0), px(20.0))).unwrap(), down(8));
    assert_eq!(layout.hit_test(point(px(13.0), px(-4.0))).unwrap(), down(2));
    assert_eq!(
        layout.hit_test(point(px(1000.0), px(1000.0))).unwrap(),
        down(11)
    );
}

#[test]
fn visual_movement_follows_carets_and_clamps_at_the_ends() {
    let layout = layout("aaa bbb ccc", 45.0);
    let right = |p| layout.move_visual(p, VisualDirection::Right).unwrap();
    let left = |p| layout.move_visual(p, VisualDirection::Left).unwrap();
    // Right onto a soft wrap stops at the first line's end; the next Right
    // continues past the shared byte on the following line.
    assert_eq!(right(down(7)), up(8));
    assert_eq!(right(up(8)), down(9));
    assert_eq!(left(down(8)), down(7));
    assert_eq!(left(down(0)), down(0));
    assert_eq!(right(down(11)), down(11));
}

#[test]
fn selection_spans_wrapped_lines() {
    let layout = layout("aaa bbb ccc", 45.0);
    let rects = layout.selection_bounds(down(2), down(10)).unwrap();
    assert_eq!(
        rects,
        vec![
            Bounds::new(
                point(px(2.0 * CHAR), px(0.0)),
                size(px(6.0 * CHAR), px(LINE))
            ),
            Bounds::new(point(px(0.0), px(LINE)), size(px(2.0 * CHAR), px(LINE))),
        ]
    );
    // Order does not matter, and an empty selection has no rectangles.
    assert_eq!(layout.selection_bounds(down(10), down(2)).unwrap(), rects);
    assert!(
        layout
            .selection_bounds(down(4), down(4))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn long_words_break_at_clusters_and_zero_width_still_progresses() {
    let narrow = layout("abcdefghij", 25.0);
    assert_eq!(line_starts(&*narrow), vec![0, 4, 8]);
    assert_eq!(narrow.size().height, px(3.0 * LINE));
    let zero = layout("abcdefghij", 0.0);
    assert_eq!(zero.size().height, px(10.0 * LINE));
}

#[test]
fn content_widths_are_longest_word_and_whole_line() {
    assert_eq!(
        shape("aaa bbbbb cc").content_widths().unwrap(),
        (px(5.0 * CHAR), px(12.0 * CHAR))
    );
    let wide = layout("aaa bbbbb cc", 12.0 * CHAR);
    assert_eq!(wide.size().height, px(LINE));
}

#[test]
fn hard_breaks_split_lines_and_empty_lines_have_height() {
    // "ab" 0..2, break 2..5, "" 5..5, break 5..8, "cd" 8..10.
    let text = "ab\u{2028}\u{2028}cd";
    let layout = layout(text, 100.0);
    assert_eq!(layout.size().height, px(3.0 * LINE));
    assert_eq!(
        layout.caret_bounds(down(5)).unwrap().origin,
        point(px(0.0), px(LINE))
    );
    assert_eq!(
        layout.caret_bounds(down(8)).unwrap().origin,
        point(px(0.0), px(2.0 * LINE))
    );
    assert!(layout.caret_bounds(down(3)).is_err());
    let right = |p| layout.move_visual(p, VisualDirection::Right).unwrap();
    assert_eq!(right(down(2)), down(5));
    assert_eq!(right(down(5)), down(8));
    assert_eq!(
        layout.hit_test(point(px(50.0), px(LINE + 1.0))).unwrap(),
        down(5)
    );

    let empty = self::layout("", 100.0);
    assert_eq!(empty.size(), size(px(100.0), px(LINE)));
    assert_eq!(
        empty.caret_bounds(down(0)).unwrap().size,
        size(px(1.0), px(LINE))
    );
}

#[test]
fn invalid_input_and_positions_are_errors() {
    let noop = NoopTextSystem::new();
    let options = ParagraphShapeOptions::default();
    for text in ["a\nb", "a\rb", "a\u{2029}b"] {
        assert!(
            noop.shape_paragraph(ParagraphRequest::new(text, SIZE, &[], &options))
                .is_err()
        );
    }
    assert!(shape("ab").layout(px(f32::NAN)).is_err());
    let layout = layout("é", 100.0);
    assert!(layout.caret_bounds(down(1)).is_err());
    assert!(layout.caret_bounds(down(3)).is_err());
    assert!(layout.caret_bounds(down(2)).is_ok());
}

#[test]
fn native_backends_report_and_serve_left_to_right_paragraphs() {
    let text_system = Arc::new(TextSystem::new(Arc::new(NoopTextSystem::new())));
    assert_eq!(
        text_system.paragraph_support(),
        ParagraphSupport::LeftToRight
    );
    let window = WindowTextSystem::new(text_system);
    let auto = window
        .layout_paragraph("aaa bbb ccc".into(), SIZE, &[], px(45.0))
        .unwrap();
    // Forced right to left keeps the wrapping and sets each line flush right.
    let rtl = window
        .layout_paragraph_with_options(
            "aaa bbb ccc".into(),
            SIZE,
            &[],
            &ParagraphShapeOptions::default().with_direction(TextDirection::RightToLeft),
            px(45.0),
        )
        .unwrap();
    assert_eq!(auto.size(), rtl.size());
    assert_eq!(auto.caret_bounds(down(0)).unwrap().origin.x, px(0.0));
    // "aaa bbb" is 42px of the 45px line; "ccc" is 18px.
    assert_eq!(rtl.caret_bounds(down(0)).unwrap().origin.x, px(3.0));
    assert_eq!(
        rtl.caret_bounds(down(8)).unwrap().origin,
        point(px(27.0), px(LINE))
    );
    assert_eq!(
        window
            .paragraph_content_widths("aaa bbb ccc".into(), SIZE, &[])
            .unwrap(),
        (px(3.0 * CHAR), px(11.0 * CHAR))
    );
}

/// Noop shaping with each line's glyphs mirrored, as a native shaper reports a
/// right-to-left run: byte 0 is the rightmost glyph.
fn shape_reversed(text: &str) -> Arc<dyn PlatformParagraphShape> {
    let options = ParagraphShapeOptions::default();
    shape_paragraph(
        ParagraphRequest::new(text, SIZE, &[], &options),
        |text, size, runs| {
            let mut line = NoopTextSystem::new().layout_line(text, size, runs);
            let width = line.width;
            for run in &mut line.runs {
                for glyph in &mut run.glyphs {
                    glyph.position.x = width - glyph.position.x - px(CHAR);
                }
                run.glyphs.reverse();
            }
            line
        },
    )
    .unwrap()
}

#[test]
fn reordered_runs_stay_inside_their_lines() {
    // Ten 6px clusters, no break opportunities: at 25px, lines of 4, 4 and 2.
    let shape = shape_reversed("abcdefghij");
    assert_eq!(shape.content_widths().unwrap(), (px(60.0), px(60.0)));
    let layout = shape.layout(px(25.0)).unwrap();
    assert_eq!(layout.size().height, px(3.0 * LINE));
    let glyphs: Vec<_> = layout
        .runs()
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .collect();
    for glyph in &glyphs {
        assert!(
            glyph.position.x >= px(0.0) && glyph.position.x < px(25.0),
            "{} at {:?}",
            glyph.index,
            glyph.position.x
        );
    }
    // Within the first line, byte order runs right to left.
    let first: Vec<_> = glyphs
        .iter()
        .filter(|glyph| glyph.index < 4)
        .map(|glyph| (glyph.index, glyph.position.x))
        .collect();
    assert!(
        first
            .iter()
            .all(|(index, x)| *x == px((3 - *index) as f32 * CHAR)),
        "{first:?}"
    );

    // The logical start is the cluster's right edge, the line's end its left edge.
    assert_eq!(
        layout.caret_bounds(down(0)).unwrap().origin.x,
        px(4.0 * CHAR)
    );
    assert_eq!(layout.caret_bounds(up(4)).unwrap().origin.x, px(0.0));
    for byte_index in 0..=10 {
        let caret = layout.caret_bounds(down(byte_index)).unwrap();
        assert_eq!(
            layout.hit_test(caret.center()).unwrap().byte_index,
            byte_index,
            "caret {byte_index}"
        );
    }
    // Moving left reads forward through the run.
    assert_eq!(
        layout.move_visual(down(0), VisualDirection::Left).unwrap(),
        down(1)
    );
    assert_eq!(
        layout.move_visual(down(1), VisualDirection::Right).unwrap(),
        down(0)
    );
    // A selection inside the run is one rectangle at its visual place.
    assert_eq!(
        layout.selection_bounds(down(1), down(3)).unwrap(),
        vec![Bounds::new(
            point(px(CHAR), px(0.0)),
            size(px(2.0 * CHAR), px(LINE))
        )]
    );
}

fn shape_with(text: &str, direction: TextDirection) -> Arc<dyn PlatformParagraphShape> {
    let options = ParagraphShapeOptions::default().with_direction(direction);
    NoopTextSystem::new()
        .shape_paragraph(ParagraphRequest::new(text, SIZE, &[], &options))
        .unwrap()
}

/// Where each line's visible text starts and ends, top to bottom.
fn line_extents(layout: &dyn PlatformParagraphLayout, text: &str) -> Vec<(Pixels, Pixels)> {
    let mut lines: Vec<(Pixels, Pixels, Pixels)> = Vec::new();
    for glyph in layout.runs().iter().flat_map(|run| run.glyphs.iter()) {
        if text.as_bytes()[glyph.index] == b' ' {
            continue;
        }
        let (top, left, right) = (
            glyph.position.y,
            glyph.position.x,
            glyph.position.x + px(CHAR),
        );
        match lines.iter_mut().find(|line| line.0 == top) {
            Some(line) => {
                line.1 = line.1.min(left);
                line.2 = line.2.max(right);
            }
            None => lines.push((top, left, right)),
        }
    }
    lines
        .into_iter()
        .map(|(_, left, right)| (left, right))
        .collect()
}

#[test]
fn the_base_direction_sets_each_lines_start_edge() {
    // Auto: a first strong Hebrew letter makes the base right to left. Noop
    // shaping keeps byte order, which is enough to check the alignment.
    let hebrew = "אבג דהו זחט";
    let layout = shape_with(hebrew, TextDirection::Auto)
        .layout(px(45.0))
        .unwrap();
    for (left, right) in line_extents(&*layout, hebrew) {
        assert_eq!(right, px(45.0), "flush right ({left:?}..{right:?})");
    }
    // Digits and Latin are left to right under Auto; an explicit direction wins.
    for (text, direction, right_aligned) in [
        ("123 456 789", TextDirection::Auto, false),
        ("aaa bbb ccc", TextDirection::Auto, false),
        ("123 456 789", TextDirection::RightToLeft, true),
        (hebrew, TextDirection::LeftToRight, false),
    ] {
        let layout = shape_with(text, direction).layout(px(45.0)).unwrap();
        for (left, right) in line_extents(&*layout, text) {
            if right_aligned {
                assert_eq!(right, px(45.0), "{text:?} {direction:?}: flush right");
            } else {
                assert_eq!(left, px(0.0), "{text:?} {direction:?}: flush left");
            }
        }
    }
    // An empty right-to-left paragraph puts its caret at the right edge.
    let empty = shape_with("", TextDirection::RightToLeft)
        .layout(px(45.0))
        .unwrap();
    assert_eq!(empty.caret_bounds(down(0)).unwrap().origin.x, px(44.0));
    let empty = shape_with("", TextDirection::Auto)
        .layout(px(45.0))
        .unwrap();
    assert_eq!(empty.caret_bounds(down(0)).unwrap().origin.x, px(0.0));
}
