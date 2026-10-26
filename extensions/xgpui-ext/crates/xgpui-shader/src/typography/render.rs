//! Shared visual glyph plan for foreground and shadow. No selection chrome here.
use super::{KineticEffect, scramble};
use gpui::*;
use std::rc::Rc;

pub(super) struct GlyphCommand {
    origin: Point<Pixels>,
    font: FontId,
    glyph: GlyphId,
    size: Pixels,
    emoji: bool,
    mask: Option<Bounds<Pixels>>,
}
pub(super) struct GlyphFrame<'a> {
    pub line: &'a ShapedLine,
    pub effect: KineticEffect,
    pub boundaries: &'a [usize],
    pub groups: &'a [(usize, usize, usize)],
    pub source: &'a str,
    pub alphabets: &'a [Rc<scramble::Alphabet>],
    pub carets: &'a [f32],
    pub bounds: Bounds<Pixels>,
    pub origin: Point<Pixels>,
    pub scroll_x: Pixels,
    pub line_height: Pixels,
}
impl GlyphFrame<'_> {
    pub fn commands(&self) -> Vec<GlyphCommand> {
        let Self {
            line,
            effect,
            groups,
            boundaries,
            source,
            alphabets,
            carets,
            bounds,
            origin,
            scroll_x,
            line_height,
        } = self;
        let baseline = (*line_height - line.ascent - line.descent) / 2. + line.ascent;
        let mut commands = Vec::new();
        let mut substituted = vec![false; groups.len()];
        if let KineticEffect::Scramble { seed, tick, .. } = *effect {
            let resolved = effect.resolved_byte(boundaries);
            let mut fonts = vec![None; groups.len()];
            let mut eligible = vec![true; groups.len()];
            for run in &line.runs {
                for glyph in &run.glyphs {
                    let index = group_for(groups, glyph.index);
                    if glyph.is_emoji || fonts[index].is_some_and(|font| font != run.font_id) {
                        eligible[index] = false;
                    }
                    fonts[index].get_or_insert(run.font_id);
                }
            }
            for (index, &(start, end, ordinal)) in groups.iter().enumerate() {
                if end <= resolved
                    || source[start..end].chars().all(char::is_whitespace)
                    || !eligible[index]
                {
                    continue;
                }
                let Some(font) = fonts[index] else {
                    continue;
                };
                let Some(alphabet) = alphabets
                    .iter()
                    .find(|alphabet| alphabet.font == font && !alphabet.glyphs.is_empty())
                else {
                    continue;
                };
                let replacement =
                    &alphabet.glyphs[scramble::choice(seed, tick, ordinal, alphabet.glyphs.len())];
                let left = px(carets[boundaries.binary_search(&start).unwrap()]);
                let right = px(carets[boundaries.binary_search(&end).unwrap()]);
                let width = right - left;
                if width <= px(0.) {
                    continue;
                }
                let scale = (f32::from(width) / f32::from(replacement.width)).min(1.);
                commands.push(GlyphCommand {
                    origin: *origin
                        + point(
                            left + (width - replacement.width * scale) / 2.,
                            baseline - line.ascent * (1. - scale) / 2.,
                        )
                        + replacement.position * scale,
                    font,
                    glyph: replacement.glyph,
                    size: line.font_size * scale,
                    emoji: false,
                    mask: Some(Bounds::new(
                        point(bounds.left() + left - *scroll_x, bounds.top()),
                        size(width, bounds.size.height),
                    )),
                });
                substituted[index] = true;
            }
        }
        let cutoff = effect.reveal_byte(boundaries);
        for run in &line.runs {
            for glyph in &run.glyphs {
                let ordinal = if *effect == KineticEffect::Static {
                    0
                } else {
                    let index = group_for(groups, glyph.index);
                    let (_, end, ordinal) = groups[index];
                    if end > cutoff || substituted[index] {
                        continue;
                    }
                    ordinal
                };
                commands.push(GlyphCommand {
                    origin: *origin
                        + glyph.position
                        + point(px(0.), baseline + effect.offset(ordinal)),
                    font: run.font_id,
                    glyph: glyph.id,
                    size: line.font_size,
                    emoji: glyph.is_emoji,
                    mask: None,
                });
            }
        }
        commands
    }
}
fn group_for(groups: &[(usize, usize, usize)], byte: usize) -> usize {
    groups
        .partition_point(|(start, _, _)| *start <= byte)
        .saturating_sub(1)
}

pub(super) fn paint(
    commands: &[GlyphCommand],
    window: &mut Window,
    color: Hsla,
    offset: Point<Pixels>,
    monochrome_only: bool,
) -> anyhow::Result<()> {
    for command in commands {
        if command.emoji && monochrome_only {
            continue;
        }
        let draw = |window: &mut Window| {
            if command.emoji {
                window.paint_emoji(
                    command.origin + offset,
                    command.font,
                    command.glyph,
                    command.size,
                )
            } else {
                window.paint_glyph(
                    command.origin + offset,
                    command.font,
                    command.glyph,
                    command.size,
                    color,
                )
            }
        };
        if let Some(mut mask) = command.mask {
            mask.origin += offset;
            window.with_content_mask(Some(ContentMask { bounds: mask }), draw)?;
        } else {
            draw(window)?;
        }
    }
    Ok(())
}
