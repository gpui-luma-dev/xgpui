use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Logical selection endpoints are always extended-grapheme boundaries in UTF-8.
#[derive(Clone, Debug)]
pub(super) struct Selection {
    pub boundaries: Vec<usize>,
    pub anchor: usize,
    pub focus: usize,
}
impl Selection {
    pub fn new(text: &str) -> Self {
        let mut boundaries: Vec<_> = text.grapheme_indices(true).map(|(i, _)| i).collect();
        boundaries.push(text.len());
        Self {
            boundaries,
            anchor: 0,
            focus: 0,
        }
    }
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }
    pub fn snap(&self, offset: usize) -> usize {
        let i = self
            .boundaries
            .partition_point(|b| *b <= offset)
            .saturating_sub(1);
        self.boundaries[i]
    }
    pub fn move_to(&mut self, offset: usize, extend: bool) {
        self.focus = self.snap(offset);
        if !extend {
            self.anchor = self.focus;
        }
    }
    pub fn step(&mut self, forward: bool, extend: bool) {
        let range = self.range();
        if !extend && !range.is_empty() {
            self.move_to(if forward { range.end } else { range.start }, false);
            return;
        }
        let i = self.boundaries.binary_search(&self.focus).unwrap();
        let next = if forward {
            (i + 1).min(self.boundaries.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        self.move_to(self.boundaries[next], extend);
    }
    pub fn word(&mut self, text: &str, forward: bool, extend: bool) {
        let offset = if forward {
            text.unicode_word_indices()
                .map(|(i, w)| i + w.len())
                .find(|i| *i > self.focus)
                .unwrap_or(text.len())
        } else {
            text.unicode_word_indices()
                .map(|(i, _)| i)
                .take_while(|i| *i < self.focus)
                .last()
                .unwrap_or(0)
        };
        self.move_to(offset, extend);
    }
    pub fn select_word(&mut self, text: &str, offset: usize) {
        let span = text
            .split_word_bound_indices()
            .find(|(i, word)| *i <= offset && offset < i + word.len());
        if let Some((i, word)) = span {
            self.anchor = self.snap(i);
            self.focus = self.snap(i + word.len());
        } else {
            self.move_to(offset, false);
        }
    }
}

/// Interpolate grapheme carets inside a shaped ligature without reshaping it.
/// Anchors must be logical-byte ordered and visually monotonic (the LTR contract).
pub(super) fn caret_positions(boundaries: &[usize], anchors: &[(usize, f32)]) -> Vec<f32> {
    boundaries
        .iter()
        .map(|byte| {
            let index = anchors
                .partition_point(|(i, _)| i <= byte)
                .saturating_sub(1);
            let (start, x) = anchors[index];
            let Some(&(end, next_x)) = anchors.get(index + 1) else {
                return x;
            };
            let start_i = boundaries.partition_point(|b| *b < start);
            let end_i = boundaries.partition_point(|b| *b < end);
            let i = boundaries.partition_point(|b| *b < *byte);
            if end_i == start_i {
                x
            } else {
                x + (next_x - x) * (i - start_i) as f32 / (end_i - start_i) as f32
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_never_splits_combining_marks_or_emoji() {
        let text = "Ae\u{301}👩🏽‍💻🇺🇸Z";
        let mut s = Selection::new(text);
        for boundary in s.boundaries.clone().into_iter().skip(1) {
            s.step(true, true);
            assert_eq!(s.focus, boundary);
            assert!(text.get(s.range()).is_some());
        }
        assert_eq!(&text[s.range()], text);
        s.step(false, false);
        assert_eq!(s.range(), 0..0);
        s.move_to(text.len(), false);
        s.step(false, true);
        assert_eq!(&text[s.range()], "Z");
        s.move_to(2, false);
        assert_eq!(s.focus, 1);
    }
    #[test]
    fn reversal_word_selection_and_empty_text() {
        let mut s = Selection::new("hello café");
        s.move_to(5, false);
        s.move_to(0, true);
        assert_eq!(s.range(), 0..5);
        s.move_to(11, true);
        assert_eq!(s.range(), 5..11);
        s.select_word("hello café", 8);
        assert_eq!(s.range(), 6..11);
        s.word("hello café", false, false);
        assert_eq!(s.focus, 6);
        s.word("hello café", true, true);
        assert_eq!(s.range(), 6..11);
        let mut empty = Selection::new("");
        empty.step(true, true);
        empty.step(false, false);
        assert_eq!(empty.range(), 0..0);
    }
    #[test]
    fn ligature_carets_use_graphemes_not_utf8_byte_lengths() {
        let s = Selection::new("fié");
        assert_eq!(
            caret_positions(&s.boundaries, &[(0, 0.), (2, 12.), (4, 20.)]),
            vec![0., 6., 12., 20.]
        );
    }
}
