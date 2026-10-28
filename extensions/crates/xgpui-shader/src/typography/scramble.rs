//! A bounded, shaped replacement alphabet. Never invent platform glyph IDs.
use gpui::*;

pub(super) struct Replacement {
    pub glyph: GlyphId,
    pub position: Point<Pixels>,
    pub width: Pixels,
}
pub(super) struct Alphabet {
    pub font: FontId,
    pub size: Pixels,
    pub glyphs: Vec<Replacement>,
}
impl Alphabet {
    pub fn prepare(font: FontId, size: Pixels, window: &Window) -> Self {
        let mut glyphs = Vec::new();
        if let Some(resolved) = window.text_system().get_font_for_id(font) {
            for character in "0123456789ABCDEF".chars() {
                let text: SharedString = character.to_string().into();
                let run = TextRun {
                    len: text.len(),
                    font: resolved.clone(),
                    color: white(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window.text_system().shape_line(text, size, &[run], None);
                // Missing glyphs, fallback faces and color glyphs keep their original source.
                if line.runs.len() == 1
                    && line.runs[0].font_id == font
                    && line.runs[0].glyphs.len() == 1
                {
                    let glyph = &line.runs[0].glyphs[0];
                    if !glyph.is_emoji && glyph.id != GlyphId(0) && line.width > px(0.) {
                        glyphs.push(Replacement {
                            glyph: glyph.id,
                            position: glyph.position,
                            width: line.width,
                        });
                    }
                }
            }
        }
        Self { font, size, glyphs }
    }
}

pub(super) fn choice(seed: u64, tick: u64, ordinal: usize, len: usize) -> usize {
    let mut word = seed
        ^ tick.wrapping_mul(0x9e3779b97f4a7c15)
        ^ (ordinal as u64).wrapping_mul(0xd6e8feb86659fd93);
    word = (word ^ (word >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    word = (word ^ (word >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((word ^ (word >> 31)) % len as u64) as usize
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn seeded_choice_is_repeatable_bounded_and_frame_sensitive() {
        let choices: Vec<_> = (0..32).map(|i| choice(42, 9, i, 16)).collect();
        assert_eq!(
            choices,
            (0..32).map(|i| choice(42, 9, i, 16)).collect::<Vec<_>>()
        );
        assert!(choices.iter().all(|i| *i < 16));
        assert_ne!(
            choices,
            (0..32).map(|i| choice(43, 9, i, 16)).collect::<Vec<_>>()
        );
        assert_ne!(
            choices,
            (0..32).map(|i| choice(42, 10, i, 16)).collect::<Vec<_>>()
        );
    }
}
