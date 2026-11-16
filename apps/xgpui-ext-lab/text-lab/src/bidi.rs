//! Right-to-left text and paragraphs through GPUI's own APIs (#325). The native
//! and Parley windows run this same code; only the installed text backend differs.
use gpui::{
    Bounds, Context, FocusHandle, Focusable, Font, FontFallbacks, FontRun, HighlightStyle,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParagraphPosition,
    ParagraphShapeOptions, ParagraphSupport, Pixels, PlatformParagraphLayout, Point, Render,
    SharedString, StrikethroughStyle, StyledText, TextAffinity, TextDirection, UnderlineStyle,
    VisualDirection, Window, canvas, div, fill, hsla, paint_paragraph, prelude::*, px, rgb,
};
use std::sync::Arc;

const LABELS: [(&str, &str); 5] = [
    ("Latin", "Plain GPUI text label"),
    ("Hebrew", "עברית"),
    ("Arabic", "مرحبا بالعالم"),
    ("Mixed", "abc עברית 123"),
    ("Mixed, punctuation", "שלום, world!"),
];

/// Three Hebrew words, each styled differently (byte ranges are word + space).
const STYLED: &str = "שלום עולם יפה";

const LONG: [&str; 2] = [
    "כותרת ארוכה מאוד שצריכה להיחתך בסוף השורה",
    "A long Latin title that has to be truncated at the end",
];

const PARAGRAPHS: [(&str, &str); 3] = [
    (
        "Latin",
        "Paragraphs wrap, hit test and select through GPUI on every backend. Resize the window to reflow this text.",
    ),
    (
        "Hebrew",
        "פסקה בעברית שעוברת שבירת שורות, בדיקת פגיעה ובחירה דרך GPUI. שנו את גודל החלון כדי לראות את הזרימה מחדש.",
    ),
    (
        "Mixed",
        "Mixed text: שלום עולם sits inside a Latin sentence, then مرحبا بالعالم, then the numbers 2026 and 42.",
    ),
];

const DIRECTIONS: [(&str, TextDirection); 3] = [
    ("Auto", TextDirection::Auto),
    ("Left to right", TextDirection::LeftToRight),
    ("Right to left", TextDirection::RightToLeft),
];

#[derive(Clone, Copy, PartialEq)]
enum Width {
    Fit,
    MinContent,
    MaxContent,
}
const WIDTHS: [(&str, Width); 3] = [
    ("Fit window", Width::Fit),
    ("Min-content", Width::MinContent),
    ("Max-content", Width::MaxContent),
];

const FONT_SIZE: Pixels = px(22.0);

fn font() -> Font {
    let mut font = gpui::font("IBM Plex Sans");
    font.fallbacks = Some(FontFallbacks::from_fonts(vec![
        "Noto Sans Hebrew".into(),
        "Noto Sans Arabic".into(),
    ]));
    font
}

fn start() -> ParagraphPosition {
    ParagraphPosition {
        byte_index: 0,
        affinity: TextAffinity::Downstream,
    }
}

pub struct BidiLab {
    name: String,
    sample: usize,
    direction: usize,
    width: Width,
    focus: FocusHandle,
    anchor: ParagraphPosition,
    caret: ParagraphPosition,
    dragging: bool,
    geometry: Option<(Bounds<Pixels>, Arc<dyn PlatformParagraphLayout>)>,
}

/// One frame's paragraph: the layout plus what the readout shows.
struct Laid {
    layout: Arc<dyn PlatformParagraphLayout>,
    width: Pixels,
    content: (Pixels, Pixels),
}

impl BidiLab {
    pub fn new(name: String, cx: &mut Context<Self>) -> Self {
        Self {
            name,
            sample: 0,
            direction: 0,
            width: Width::Fit,
            focus: cx.focus_handle(),
            anchor: start(),
            caret: start(),
            dragging: false,
            geometry: None,
        }
    }

    fn text(&self) -> &'static str {
        PARAGRAPHS[self.sample].1
    }

    fn options(&self) -> ParagraphShapeOptions {
        ParagraphShapeOptions::default().with_direction(DIRECTIONS[self.direction].1)
    }

    fn lay_out(&self, window: &Window) -> anyhow::Result<Laid> {
        let system = window.text_system();
        let text = self.text();
        let runs = [FontRun {
            len: text.len(),
            font_id: system.resolve_font(&font()),
        }];
        let options = self.options();
        let content = system.paragraph_content_widths_with_options(
            text.into(),
            FONT_SIZE,
            &runs,
            &options,
        )?;
        let fit = (window.viewport_size().width - px(64.0)).max(px(80.0));
        let width = match self.width {
            Width::Fit => fit,
            Width::MinContent => content.0,
            Width::MaxContent => content.1,
        };
        let layout =
            system.layout_paragraph_with_options(text.into(), FONT_SIZE, &runs, &options, width)?;
        Ok(Laid {
            layout,
            width,
            content,
        })
    }

    fn reset_caret(&mut self) {
        self.anchor = start();
        self.caret = start();
    }

    fn select_point(&mut self, position: Point<Pixels>) {
        if let Some((bounds, layout)) = &self.geometry
            && let Ok(hit) = layout.hit_test(position - bounds.origin)
        {
            self.caret = hit;
        }
    }

    fn key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        let direction = match event.keystroke.key.as_str() {
            "left" => VisualDirection::Left,
            "right" => VisualDirection::Right,
            _ => return,
        };
        if let Some((_, layout)) = &self.geometry
            && let Ok(next) = layout.move_visual(self.caret, direction)
        {
            self.caret = next;
            if !modifiers.shift {
                self.anchor = next;
            }
            cx.notify();
        }
    }

    fn buttons<T: Copy + 'static>(
        &self,
        id: &'static str,
        items: &[(&'static str, T)],
        selected: impl Fn(&Self, T) -> bool,
        select: impl Fn(&mut Self, T) + Copy + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(items.iter().enumerate().map(|(ix, &(label, value))| {
                let on = selected(self, value);
                div()
                    .id((id, ix))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(if on { rgb(0x2563eb) } else { rgb(0x374151) })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        select(this, value);
                        this.reset_caret();
                        cx.notify();
                    }))
            }))
    }
}

impl Focusable for BidiLab {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

fn section(title: &str, expect: &str) -> gpui::Div {
    div()
        .p_3()
        .rounded_md()
        .bg(rgb(0x1f2937))
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_size(px(16.0)).child(title.to_owned()))
        .child(
            div()
                .text_color(rgb(0x9ca3af))
                .child(format!("Expect: {expect}")),
        )
}

impl Render for BidiLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let support = window.text_system().paragraph_support();
        let support_note = match support {
            ParagraphSupport::Full => {
                "Full: bidirectional paragraphs; direction and language are honored."
            }
            ParagraphSupport::LeftToRight => {
                "LeftToRight: built-in fallback paragraphs; lines align to the base direction's start edge, but mixed text keeps the native line order."
            }
            _ => "Unsupported: paragraph layout returns an error.",
        };

        let labels = section(
            "1. Plain GPUI text, single line (#325 B)",
            "Hebrew and Arabic read right to left. In \"abc עברית 123\" the number follows the Hebrew run, so it sits to the Hebrew word's left (Unicode bidi). \"שלום, world!\" starts with Hebrew, so the line reads right to left and the \"!\" ends up at the far left. Both windows should match.",
        )
        .children(LABELS.iter().map(|(name, text)| {
            div()
                .flex()
                .gap_3()
                .items_baseline()
                .child(div().w(px(150.0)).text_color(rgb(0x9ca3af)).child(*name))
                .child(div().text_size(FONT_SIZE).whitespace_nowrap().child(*text))
        }));

        let red = hsla(0.0, 0.85, 0.65, 1.0);
        let green = hsla(0.33, 0.7, 0.55, 1.0);
        let blue = hsla(0.6, 0.9, 0.7, 1.0);
        let words: Vec<_> = STYLED.split_inclusive(' ').collect();
        let mut offset = 0;
        let mut highlights = Vec::new();
        for (word, style) in words.iter().zip([
            HighlightStyle {
                color: Some(red),
                underline: Some(UnderlineStyle {
                    color: Some(red),
                    thickness: px(2.0),
                    wavy: false,
                }),
                ..Default::default()
            },
            HighlightStyle {
                color: Some(rgb(0x111827).into()),
                background_color: Some(green),
                ..Default::default()
            },
            HighlightStyle {
                color: Some(blue),
                strikethrough: Some(StrikethroughStyle {
                    color: Some(blue),
                    thickness: px(2.0),
                }),
                ..Default::default()
            },
        ]) {
            let len = word.trim_end().len();
            highlights.push((offset..offset + len, style));
            offset += word.len();
        }
        let styled = section(
            "2. Styled runs in right-to-left text (#325 C)",
            "the rightmost word שלום is red and underlined, the middle word עולם sits on a green background, and the leftmost word יפה is blue and struck through.",
        )
        .child(
            div().text_size(FONT_SIZE).whitespace_nowrap().child(
                StyledText::new(SharedString::from(STYLED)).with_highlights(highlights),
            ),
        );

        let truncation = section(
            "3. End truncation",
            "the Hebrew title keeps its first words with the ellipsis on the left; the Latin title keeps its start with the ellipsis on the right.",
        )
        .children(LONG.iter().map(|text| {
            div()
                .w(px(260.0))
                .p_1()
                .bg(rgb(0x111827))
                .text_size(FONT_SIZE)
                .truncate()
                .child(*text)
        }));

        let mut paragraph = section(
            "4. GPUI paragraph: window.text_system().layout_paragraph_with_options (#325 A)",
            "click places a caret, drag selects, Left/Right move visually and Shift extends. Hebrew (Auto) and Right to left start at the right edge in both windows. Min-content and max-content resize the paragraph to its reported content widths.",
        )
        .child(self.buttons(
            "bidi-sample",
            &[("Latin", 0), ("Hebrew", 1), ("Mixed", 2)],
            |this, ix| this.sample == ix,
            |this, ix| this.sample = ix,
            cx,
        ))
        .child(self.buttons(
            "bidi-direction",
            &[
                (DIRECTIONS[0].0, 0),
                (DIRECTIONS[1].0, 1),
                (DIRECTIONS[2].0, 2),
            ],
            |this, ix| this.direction == ix,
            |this, ix| this.direction = ix,
            cx,
        ))
        .child(self.buttons(
            "bidi-width",
            &WIDTHS,
            |this, width| this.width == width,
            |this, width| this.width = width,
            cx,
        ));

        match self.lay_out(window) {
            Err(error) => {
                paragraph = paragraph.child(
                    div()
                        .text_color(rgb(0xf87171))
                        .child(format!("Paragraph layout failed: {error:#}")),
                );
            }
            Ok(laid) => {
                let size = laid.layout.size();
                paragraph = paragraph.child(format!(
                    "Content widths: min {:.1}px, max {:.1}px | laid out at {:.1}px -> {:.1} x {:.1}px | caret byte {} ({:?}) | anchor byte {}",
                    f32::from(laid.content.0),
                    f32::from(laid.content.1),
                    f32::from(laid.width),
                    f32::from(size.width),
                    f32::from(size.height),
                    self.caret.byte_index,
                    self.caret.affinity,
                    self.anchor.byte_index,
                ));
                let entity = cx.entity();
                let layout = laid.layout.clone();
                let (anchor, caret) = (self.anchor, self.caret);
                let focused = self.focus.is_focused(window);
                paragraph = paragraph.child(
                    div()
                        .id("bidi-paragraph")
                        .track_focus(&self.focus)
                        .on_key_down(cx.listener(Self::key))
                        .p_3()
                        .bg(rgb(0xffffff))
                        .flex_none()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                window.focus(&this.focus, cx);
                                this.select_point(event.position);
                                if !event.modifiers.shift {
                                    this.anchor = this.caret;
                                }
                                this.dragging = true;
                                cx.notify();
                            }),
                        )
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                            if this.dragging {
                                this.select_point(event.position);
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| this.dragging = false),
                        )
                        .on_mouse_up_out(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| this.dragging = false),
                        )
                        .child(
                            canvas(
                                move |bounds, _, cx| {
                                    entity.update(cx, |this, _| {
                                        this.geometry = Some((bounds, layout.clone()));
                                    });
                                    layout
                                },
                                move |bounds, layout, window, _| {
                                    if let Ok(rects) = layout.selection_bounds(anchor, caret) {
                                        for rect in rects {
                                            window.paint_quad(fill(
                                                Bounds::new(bounds.origin + rect.origin, rect.size),
                                                rgb(0xbdd7ff),
                                            ));
                                        }
                                    }
                                    if let Err(error) = paint_paragraph(
                                        layout.as_ref(),
                                        bounds.origin,
                                        |_| rgb(0x111827).into(),
                                        window,
                                    ) {
                                        eprintln!("Paragraph paint: {error:#}");
                                    }
                                    if focused && let Ok(rect) = layout.caret_bounds(caret) {
                                        window.paint_quad(fill(
                                            Bounds::new(bounds.origin + rect.origin, rect.size),
                                            rgb(0xdc2626),
                                        ));
                                    }
                                },
                            )
                            .w(size.width)
                            .h(size.height),
                        ),
                );
            }
        }

        div()
            .id("bidi-lab")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(0x111827))
            .text_color(rgb(0xeff4fc))
            .font_family("IBM Plex Sans")
            .text_size(px(13.0))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(24.0))
                    .child(format!("{} text: right to left and paragraphs", self.name)),
            )
            .child(format!("Paragraph support: {support_note}"))
            .child(labels)
            .child(styled)
            .child(truncation)
            .child(paragraph)
    }
}
