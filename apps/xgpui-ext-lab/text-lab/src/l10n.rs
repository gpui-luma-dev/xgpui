//! Localization QA (gpui-luma-dev/xgpui#355): one settings screen in several
//! locales, with every text element checked after layout.
//!
//! Checks, per text element:
//! - bad break: a line break that isn't a line-break opportunity according to
//!   ICU's complex-script line breaker (UAX #14 plus dictionaries for Thai,
//!   Khmer, Lao and Burmese);
//! - split cluster: a line break inside a grapheme cluster;
//! - overflow: a line wider than its box, measured by shaping the line.
//!
//! With a Parley backend, the screen can be laid out by GPUI's flat lines or
//! by Parley (`xgpui_parley::install_text_layout`); the native window uses its
//! platform's flat lines. The same code runs in both windows.
#[path = "l10n/locales.rs"]
mod locales;

pub(crate) use locales::{LOCALES, Locale};

use gpui::{
    Bounds, Context, Pixels, Render, SharedString, StyledText, TextLayout, Window, canvas, div,
    prelude::*, px, rgb,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt,
    rc::Rc,
    sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;
use xgpui_parley::ParleyTextSystem;

/// A problem with one text element's layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Problem {
    BadBreak,
    SplitCluster,
    Overflow,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Problem::BadBreak => "bad break",
            Problem::SplitCluster => "split cluster",
            Problem::Overflow => "overflow",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub locale: &'static str,
    pub element: &'static str,
    pub problem: Problem,
    pub detail: String,
}

/// Line breaks in `wrapped` (lines joined by `\n`) that aren't allowed in
/// `text`, as (problem, byte offset in `text`). Returns nothing when the lines
/// don't rebuild `text`, as for truncated text.
pub(crate) fn check_breaks(text: &str, wrapped: &str) -> Vec<(Problem, usize)> {
    let lines: Vec<&str> = wrapped.split('\n').collect();
    if lines.len() < 2 || lines.concat() != text {
        return Vec::new();
    }
    let allowed: HashSet<usize> = icu_segmenter::LineSegmenter::new_dictionary(Default::default())
        .segment_str(text)
        .collect();
    let clusters: HashSet<usize> = text.grapheme_indices(true).map(|(ix, _)| ix).collect();
    let mut offset = 0;
    let mut problems = Vec::new();
    for line in &lines[..lines.len() - 1] {
        offset += line.len();
        if !clusters.contains(&offset) {
            problems.push((Problem::SplitCluster, offset));
        } else if !allowed.contains(&offset) {
            problems.push((Problem::BadBreak, offset));
        }
    }
    problems
}

/// A few characters on each side of `offset`, with a bar at the break.
pub(crate) fn excerpt(text: &str, offset: usize) -> String {
    let before: String = text[..offset]
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after: String = text[offset..].chars().take(6).collect();
    format!("…{before} | {after}…")
}

/// The panel's checked text elements, by name.
const ELEMENTS: [&str; 8] = [
    "title",
    "row 1",
    "row 2",
    "row 3",
    "description",
    "button 1",
    "button 2",
    "path",
];

/// One text element drawn this frame, kept for the audit.
struct Checked {
    locale: &'static str,
    element: &'static str,
    text: &'static str,
    layout: TextLayout,
    font_size: Pixels,
    /// Wrapping text gets break checks; one-line text only the overflow check.
    wraps: bool,
}

pub struct L10nLab {
    name: String,
    parley: Option<Arc<ParleyTextSystem>>,
    plain_on_parley: bool,
    applied: bool,
    checked: Vec<Checked>,
    /// Each panel's content box, recorded during prepaint.
    panels: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    findings: Vec<Finding>,
    audited: bool,
}

const PANEL_WIDTH: Pixels = px(280.);
const FONT: &str = "IBM Plex Sans";

impl L10nLab {
    /// `parley` is the installed Parley backend, if any. With it, the screen
    /// starts on GPUI's flat lines, and a toggle switches plain text to Parley
    /// (`install_text_layout`).
    pub fn new(
        name: String,
        parley: Option<Arc<ParleyTextSystem>>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            name,
            parley,
            plain_on_parley: false,
            applied: false,
            checked: Vec::new(),
            panels: Rc::default(),
            findings: Vec::new(),
            audited: false,
        }
    }

    /// Switch plain text between Parley and GPUI's flat lines.
    #[cfg(test)]
    pub(crate) fn set_plain_on_parley(&mut self, on: bool) {
        self.plain_on_parley = on;
    }

    #[cfg(test)]
    pub(crate) fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Whether the findings describe the last drawn frame.
    #[cfg(test)]
    pub(crate) fn audited(&self) -> bool {
        self.audited
    }

    /// Check the text elements drawn last frame. Returns whether the
    /// findings changed.
    pub(crate) fn audit(&mut self, window: &mut Window) -> bool {
        let mut findings = Vec::new();
        for checked in &self.checked {
            let wrapped = checked.layout.wrapped_text();
            if checked.wraps {
                for (problem, offset) in check_breaks(checked.text, &wrapped) {
                    findings.push(Finding {
                        locale: checked.locale,
                        element: checked.element,
                        problem,
                        detail: excerpt(checked.text, offset),
                    });
                }
            }
            // Text that grows past its panel, such as a word wider than the
            // panel, overlaps whatever is beside it.
            let bounds = checked.layout.bounds();
            if let Some(panel) = self.panels.borrow().get(checked.locale) {
                let past = (bounds.right() - panel.right()).max(panel.left() - bounds.left());
                if past > px(1.) {
                    findings.push(Finding {
                        locale: checked.locale,
                        element: checked.element,
                        problem: Problem::Overflow,
                        detail: format!("extends {:.0}px past its panel", f32::from(past)),
                    });
                }
            }
            let width = bounds.size.width;
            let mut style = window.text_style();
            style.font_family = FONT.into();
            for (ix, line) in wrapped.split('\n').enumerate() {
                let line = line.trim_end();
                if line.is_empty() {
                    continue;
                }
                let shaped = window.text_system().shape_line(
                    SharedString::from(line.to_owned()),
                    checked.font_size,
                    &[style.to_run(line.len())],
                    None,
                );
                if shaped.width > width + px(1.) {
                    findings.push(Finding {
                        locale: checked.locale,
                        element: checked.element,
                        problem: Problem::Overflow,
                        detail: format!(
                            "line {} is {:.0}px in a {:.0}px box",
                            ix + 1,
                            f32::from(shaped.width),
                            f32::from(width)
                        ),
                    });
                }
            }
        }
        self.audited = true;
        let changed = findings != self.findings;
        self.findings = findings;
        changed
    }

    /// A checked plain text element.
    fn text(
        &mut self,
        locale: &'static str,
        element: &'static str,
        text: &'static str,
        font_size: Pixels,
        wraps: bool,
    ) -> StyledText {
        let styled = StyledText::new(text);
        self.checked.push(Checked {
            locale,
            element,
            text,
            layout: styled.layout().clone(),
            font_size,
            wraps,
        });
        styled
    }

    fn panel(&mut self, locale: &'static Locale) -> gpui::Div {
        let tag = locale.tag;
        let row = || {
            let row = div().flex().gap_2();
            if locale.rtl {
                row.flex_row_reverse()
            } else {
                row.flex_row()
            }
        };
        let problems: Vec<Finding> = self
            .findings
            .iter()
            .filter(|finding| finding.locale == tag)
            .cloned()
            .collect();
        let mut body =
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_size(px(18.)).child(self.text(
                    tag,
                    ELEMENTS[0],
                    locale.title,
                    px(18.),
                    true,
                )));
        for (ix, (label, value)) in locale.rows.iter().enumerate() {
            let label = self.text(tag, ELEMENTS[1 + ix], label, px(14.), true);
            body = body.child(
                row()
                    .items_start()
                    .justify_between()
                    .child(div().flex_1().min_w_0().child(label))
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .text_color(rgb(0x2563eb))
                            .child(*value),
                    ),
            );
        }
        body = body.child(
            div()
                .text_size(px(13.))
                .text_color(rgb(0x4b5563))
                .child(self.text(tag, ELEMENTS[4], locale.description, px(13.), true)),
        );
        let mut buttons = row();
        for (ix, label) in locale.buttons.iter().enumerate() {
            let label = self.text(tag, ELEMENTS[5 + ix], label, px(14.), false);
            buttons = buttons.child(
                div()
                    .w(px(110.))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(if ix == 1 {
                        rgb(0x2563eb)
                    } else {
                        rgb(0xe5e7eb)
                    })
                    .text_color(if ix == 1 {
                        rgb(0xffffff)
                    } else {
                        rgb(0x111827)
                    })
                    .truncate()
                    .child(label),
            );
        }
        body = body.child(buttons).child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0x6b7280))
                .truncate()
                .child(self.text(tag, ELEMENTS[7], locale.path, px(12.), false)),
        );
        let ok = problems.is_empty();
        let panels = self.panels.clone();
        let mut panel = div()
            .debug_selector(move || format!("l10n-panel-{tag}"))
            .relative()
            .child(
                canvas(
                    move |bounds, _, _| {
                        panels.borrow_mut().insert(tag, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .w(PANEL_WIDTH)
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_2()
            .border_color(if ok { rgb(0xd1d5db) } else { rgb(0xdc2626) })
            .bg(rgb(0xffffff))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(rgb(0x6b7280))
                    .child(format!("{tag} · {}", locale.name))
                    .child(if ok {
                        "no problems".to_owned()
                    } else {
                        format!("{} problems", problems.len())
                    }),
            )
            .child(body);
        if !ok {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(0xfecaca))
                    .text_size(px(11.))
                    .text_color(rgb(0xb91c1c))
                    .children(problems.into_iter().map(|finding| {
                        format!(
                            "{} · {}: {}",
                            finding.problem, finding.element, finding.detail
                        )
                    })),
            );
        }
        panel
    }

    fn summary(&self) -> String {
        let count = |problem| {
            self.findings
                .iter()
                .filter(|finding| finding.problem == problem)
                .count()
        };
        let mut locales: Vec<&str> = self.findings.iter().map(|finding| finding.locale).collect();
        locales.dedup();
        format!(
            "{} problems in {} of {} locales: {} bad breaks, {} split clusters, {} overflows",
            self.findings.len(),
            locales.len(),
            LOCALES.len(),
            count(Problem::BadBreak),
            count(Problem::SplitCluster),
            count(Problem::Overflow),
        )
    }

    fn engine_button(
        &self,
        ix: usize,
        label: &'static str,
        on: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(("l10n-engine", ix))
            .debug_selector(move || format!("l10n-engine-{ix}"))
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(12.))
            .bg(if on { rgb(0x2563eb) } else { rgb(0xe5e7eb) })
            .text_color(if on { rgb(0xffffff) } else { rgb(0x111827) })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.plain_on_parley = ix == 1;
                cx.notify();
            }))
    }
}

impl Render for L10nLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(parley) = &self.parley
            && self.applied != self.plain_on_parley
        {
            if self.plain_on_parley {
                xgpui_parley::install_text_layout(parley.clone(), cx);
            } else {
                cx.clear_text_layout_provider();
            }
            self.applied = self.plain_on_parley;
            self.audited = false;
        }
        self.checked.clear();
        let panels: Vec<_> = LOCALES.iter().map(|locale| self.panel(locale)).collect();
        // Check what this frame lays out once it has been drawn.
        cx.on_next_frame(window, |this, window, cx| {
            if this.audit(window) {
                cx.notify();
            }
        });
        let engine = if self.parley.is_some() {
            div()
                .flex()
                .gap_1()
                .items_center()
                .child(div().text_size(px(12.)).child("Plain text:"))
                .child(self.engine_button(0, "GPUI lines", !self.plain_on_parley, cx))
                .child(self.engine_button(1, "Parley", self.plain_on_parley, cx))
        } else {
            div()
                .text_size(px(12.))
                .child("Plain text: the platform's GPUI lines")
        };
        div()
            .id("l10n-lab")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(0xf3f4f6))
            .text_color(rgb(0x111827))
            .font_family(FONT)
            .text_size(px(14.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(div().text_size(px(20.)).child(format!("{} text: localization QA", self.name)))
                    .child(engine),
            )
            .child(
                div()
                    .debug_selector(|| "l10n-summary".into())
                    .text_color(if self.findings.is_empty() {
                        rgb(0x15803d)
                    } else {
                        rgb(0xb91c1c)
                    })
                    .child(self.summary()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(0x6b7280))
                    .child("Bad break: a line break where ICU's line breaker allows none. Split cluster: a break inside a character cluster. Overflow: a line wider than its box. Sample translations, not reviewed by native speakers."),
            )
            .child(div().flex().flex_wrap().gap_3().items_start().children(panels))
    }
}
