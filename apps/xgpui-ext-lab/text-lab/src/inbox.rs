//! A multilingual inbox built only from plain GPUI text (gpui-luma-dev/xgpui#355).
//!
//! Every label, preview and message here is `div().child(…)`, `StyledText` or
//! `InteractiveText`; nothing uses `typeset()`. On Parley with
//! `xgpui_parley::install_text_layout`, right-to-left and mixed text wraps,
//! truncates, clamps and hit-tests correctly. On GPUI's flat lines (native
//! text systems, or the "GPUI lines" toggle) the same code shows the gaps:
//! wrapped right-to-left lines come out in painting order. The native and
//! Parley windows, and the Wasm gallery (`?demo=inbox`), run this same code.
use gpui::{
    Context, FontWeight, HighlightStyle, InteractiveText, Render, SharedString, StyledText,
    TextLayout, UnderlineStyle, Window, div, prelude::*, px, rgb,
};
use std::{ops::Range, sync::Arc};
use xgpui_parley::ParleyTextSystem;

/// One conversation: its sender, preview line, message and replies.
pub(crate) struct Message {
    pub from: &'static str,
    pub time: &'static str,
    pub subject: &'static str,
    /// Paragraphs, separated by blank lines.
    pub body: &'static str,
    /// A quoted earlier message, if any.
    pub quote: Option<&'static str>,
    /// Short chat replies, shown clamped to two lines.
    pub replies: &'static [&'static str],
    pub unread: bool,
}

pub(crate) const MESSAGES: &[Message] = &[
    Message {
        from: "נועה לוי",
        time: "09:41",
        subject: "עדכון על הפרויקט",
        body: "היי, רציתי לעדכן שהגרסה החדשה מוכנה לבדיקה. הוספנו תמיכה מלאה בטקסט מימין לשמאל, כולל שבירת שורות, קיצור עם שלוש נקודות והגבלת מספר השורות.\n\nאשמח אם תעבור על המסמך עד יום חמישי ותכתוב לי הערות. תודה רבה!",
        quote: None,
        replies: &[
            "מעולה, אעבור על זה היום אחרי הצהריים ואחזור אליך עם הערות מפורטות על כל הסעיפים.",
            "תודה!",
        ],
        unread: true,
    },
    Message {
        from: "سارة حداد",
        time: "08:15",
        subject: "جدول الاجتماع الأسبوعي",
        body: "صباح الخير، أرسلت لكم جدول الاجتماع الأسبوعي. سنناقش خطة الإصدار القادم وتوزيع المهام على الفريق.\n\nيرجى تأكيد الحضور قبل نهاية اليوم.",
        quote: None,
        replies: &["سأحضر الاجتماع، لكن قد أتأخر عشر دقائق بسبب موعد آخر في الصباح الباكر."],
        unread: true,
    },
    Message {
        from: "Daniel Cohen",
        time: "Yesterday",
        subject: "Meeting moved — הפגישה נדחתה",
        body: "Meeting moved to 14:30 — הפגישה נדחתה לשעה 14:30, נא לאשר הגעה.\n\n@Noa please update the calendar invite.",
        quote: Some("Can we meet tomorrow at 10:00 to review the release checklist?"),
        replies: &["Confirmed, see you at 14:30. אני אביא את המצגת."],
        unread: false,
    },
    Message {
        from: "عمر خليل",
        time: "Mon",
        subject: "The report is ready — التقرير جاهز",
        body: "The quarterly report is ready for review: التقرير الربعي جاهز للمراجعة، وأضفت ملخصاً في الصفحة الأولى.\n\n@Sara can you check the numbers in section 3?",
        quote: None,
        replies: &[],
        unread: false,
    },
    Message {
        from: "Release bot",
        time: "Sun",
        subject: "XGPUI docs updated",
        body: "The right-to-left guide is live at https://xgpui.dev/right-to-left-text.html with a runnable example.\n\nOpen the right-to-left demo to compare GPUI lines and Parley side by side.",
        quote: None,
        replies: &[],
        unread: false,
    },
];

/// The chrome's language. Right-to-left languages mirror the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UiLanguage {
    English,
    Hebrew,
    Arabic,
}

const LANGUAGES: [(UiLanguage, &str); 3] = [
    (UiLanguage::English, "English"),
    (UiLanguage::Hebrew, "עברית"),
    (UiLanguage::Arabic, "العربية"),
];

struct Strings {
    inbox: &'static str,
    unread: &'static str,
    from: &'static str,
    replies: &'static str,
    clicked: &'static str,
    nothing_clicked: &'static str,
    engine: &'static str,
    parley: &'static str,
    flat: &'static str,
}

impl UiLanguage {
    pub(crate) fn is_rtl(self) -> bool {
        self != UiLanguage::English
    }

    fn strings(self) -> Strings {
        match self {
            UiLanguage::English => Strings {
                inbox: "Inbox",
                unread: "unread",
                from: "From",
                replies: "Replies",
                clicked: "Clicked",
                nothing_clicked: "Click a mention or a link in a message.",
                engine: "Plain text",
                parley: "Parley",
                flat: "GPUI lines",
            },
            UiLanguage::Hebrew => Strings {
                inbox: "דואר נכנס",
                unread: "שלא נקראו",
                from: "מאת",
                replies: "תשובות",
                clicked: "נלחץ",
                nothing_clicked: "לחצו על אזכור או על קישור בהודעה.",
                engine: "טקסט רגיל",
                parley: "Parley",
                flat: "שורות GPUI",
            },
            UiLanguage::Arabic => Strings {
                inbox: "البريد الوارد",
                unread: "غير مقروءة",
                from: "من",
                replies: "الردود",
                clicked: "تم النقر",
                nothing_clicked: "انقر على إشارة أو رابط في الرسالة.",
                engine: "النص العادي",
                parley: "Parley",
                flat: "أسطر GPUI",
            },
        }
    }
}

/// Mentions (`@name`) and links (`https://…`) in `text`, as byte ranges.
pub(crate) fn clickable_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, ch) in text.char_indices().chain([(text.len(), ' ')]) {
        match start {
            None if ch == '@' || text[index..].starts_with("https://") => start = Some(index),
            Some(begin) if ch.is_whitespace() => {
                let end = text[begin..index]
                    .trim_end_matches(['.', ',', '?', '!'])
                    .len()
                    + begin;
                ranges.push(begin..end);
                start = None;
            }
            _ => {}
        }
    }
    ranges
}

pub struct InboxLab {
    name: String,
    parley: Option<Arc<ParleyTextSystem>>,
    plain_on_parley: bool,
    applied: bool,
    ui: UiLanguage,
    selected: usize,
    clicked: Option<SharedString>,
    /// The open message's first paragraph, for tests.
    pub(crate) body_layout: Option<TextLayout>,
}

impl InboxLab {
    /// `parley` is the installed Parley backend, if any. With it, plain GPUI
    /// text is laid out by the Parley engine (`install_text_layout`), and a
    /// toggle switches back to GPUI's flat lines for comparison.
    pub fn new(
        name: String,
        parley: Option<Arc<ParleyTextSystem>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let plain_on_parley = parley.is_some();
        if let Some(parley) = &parley {
            xgpui_parley::install_text_layout(parley.clone(), cx);
        }
        Self {
            name,
            parley,
            plain_on_parley,
            applied: plain_on_parley,
            ui: UiLanguage::English,
            selected: 0,
            clicked: None,
            body_layout: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn ui(&self) -> UiLanguage {
        self.ui
    }

    #[cfg(test)]
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    #[cfg(test)]
    pub(crate) fn clicked(&self) -> Option<&SharedString> {
        self.clicked.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn set_plain_on_parley(&mut self, on: bool) {
        self.plain_on_parley = on;
    }

    /// A row whose children reverse order in a right-to-left UI.
    fn row(&self) -> gpui::Div {
        let row = div().flex();
        if self.ui.is_rtl() {
            row.flex_row_reverse()
        } else {
            row.flex_row()
        }
    }

    fn chip(
        &self,
        id: (&'static str, usize),
        label: &'static str,
        on: bool,
        cx: &mut Context<Self>,
        select: impl Fn(&mut Self) + 'static,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .debug_selector(move || format!("{}-{}", id.0, id.1))
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(12.))
            .bg(if on { rgb(0x2563eb) } else { rgb(0xe5e7eb) })
            .text_color(if on { rgb(0xffffff) } else { rgb(0x111827) })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                select(this);
                cx.notify();
            }))
    }

    fn header(&self, cx: &mut Context<Self>) -> gpui::Div {
        let strings = self.ui.strings();
        let unread = MESSAGES.iter().filter(|message| message.unread).count();
        let mut controls = self.row().gap_1().items_center();
        for (ix, (language, label)) in LANGUAGES.into_iter().enumerate() {
            controls = controls.child(self.chip(
                ("inbox-ui", ix),
                label,
                self.ui == language,
                cx,
                move |this| this.ui = language,
            ));
        }
        if self.parley.is_some() {
            controls = controls
                .child(div().w(px(12.)))
                .child(div().text_size(px(12.)).child(strings.engine))
                .child(self.chip(
                    ("inbox-engine", 0),
                    strings.parley,
                    self.plain_on_parley,
                    cx,
                    |this| this.plain_on_parley = true,
                ))
                .child(self.chip(
                    ("inbox-engine", 1),
                    strings.flat,
                    !self.plain_on_parley,
                    cx,
                    |this| this.plain_on_parley = false,
                ));
        }
        self.row()
            .justify_between()
            .items_center()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(0xe5e7eb))
            .child(
                self.row()
                    .gap_2()
                    .items_baseline()
                    .child(div().text_size(px(18.)).child(strings.inbox))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(0x6b7280))
                            .child(format!("{unread} {}", strings.unread)),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(0x9ca3af))
                            .child(self.name.clone()),
                    ),
            )
            .child(controls)
    }

    fn list(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut list = div()
            .debug_selector(|| "inbox-list".into())
            .w(px(300.))
            .flex_none()
            .flex()
            .flex_col()
            .border_color(rgb(0xe5e7eb));
        list = if self.ui.is_rtl() {
            list.border_l_1()
        } else {
            list.border_r_1()
        };
        for (ix, message) in MESSAGES.iter().enumerate() {
            let selected = ix == self.selected;
            let initial: String = message.from.chars().next().into_iter().collect();
            let text = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    self.row()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(if message.unread {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .child(message.from),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(rgb(0x6b7280))
                                .child(message.time),
                        ),
                )
                .child(div().truncate().text_size(px(13.)).child(message.subject))
                .child(
                    div()
                        .debug_selector(move || format!("inbox-preview-{ix}"))
                        .truncate()
                        .text_size(px(12.))
                        .text_color(rgb(0x6b7280))
                        // One line: the body with its paragraph breaks flattened.
                        .child(message.body.replace("\n\n", " ")),
                );
            list = list.child(
                self.row()
                    .id(("inbox-row", ix))
                    .debug_selector(move || format!("inbox-row-{ix}"))
                    .gap_2()
                    .p_2()
                    .cursor_pointer()
                    .bg(if selected {
                        rgb(0xdbeafe)
                    } else {
                        rgb(0xffffff)
                    })
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded_full()
                            .bg(if message.unread {
                                rgb(0x2563eb)
                            } else {
                                rgb(0x9ca3af)
                            })
                            .text_color(rgb(0xffffff))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(initial),
                    )
                    .child(text)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = ix;
                        this.clicked = None;
                        cx.notify();
                    })),
            );
        }
        list
    }

    fn message(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let strings = self.ui.strings();
        let message = &MESSAGES[self.selected];
        let link = HighlightStyle {
            color: Some(rgb(0x1d4ed8).into()),
            underline: Some(UnderlineStyle {
                color: Some(rgb(0x1d4ed8).into()),
                thickness: px(1.),
                wavy: false,
            }),
            ..Default::default()
        };
        let mut body = div()
            .debug_selector(|| "inbox-body".into())
            .flex()
            .flex_col()
            .gap_3()
            .max_w(px(560.))
            .text_size(px(15.));
        self.body_layout = None;
        for (ix, paragraph) in message.body.split("\n\n").enumerate() {
            let ranges = clickable_ranges(paragraph);
            let text = StyledText::new(paragraph)
                .with_highlights(ranges.iter().map(|range| (range.clone(), link)));
            if ix == 0 {
                self.body_layout = Some(text.layout().clone());
            }
            let entity = cx.entity().downgrade();
            let targets: Vec<SharedString> = ranges
                .iter()
                .map(|range| paragraph[range.clone()].to_owned().into())
                .collect();
            body = body.child(
                InteractiveText::new(("inbox-paragraph", ix), text).on_click(
                    ranges,
                    move |range_ix, _, cx| {
                        let target = targets[range_ix].clone();
                        entity
                            .update(cx, |this, cx| {
                                this.clicked = Some(target);
                                cx.notify();
                            })
                            .ok();
                    },
                ),
            );
        }
        if let Some(quote) = message.quote {
            let block = div()
                .px_3()
                .text_color(rgb(0x4b5563))
                .border_color(rgb(0xd1d5db));
            body = body.child(
                if self.ui.is_rtl() {
                    block.border_r_2()
                } else {
                    block.border_l_2()
                }
                .child(quote),
            );
        }
        let mut replies = div().flex().flex_col().gap_2();
        if !message.replies.is_empty() {
            replies = replies.child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(0x6b7280))
                    .child(strings.replies),
            );
        }
        for (ix, reply) in message.replies.iter().enumerate() {
            replies = replies.child(
                self.row().child(
                    div()
                        .debug_selector(move || format!("inbox-reply-{ix}"))
                        .w(px(260.))
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(rgb(0xf3f4f6))
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(*reply),
                ),
            );
        }
        let column = div()
            .debug_selector(|| "inbox-message".into())
            .flex_1()
            .min_w_0()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();
        // A right-to-left UI puts the message's blocks at the right edge.
        if self.ui.is_rtl() {
            column.items_end()
        } else {
            column
        }
        .child(div().text_size(px(20.)).child(message.subject))
        .child(
            self.row()
                .gap_1()
                .text_size(px(13.))
                .text_color(rgb(0x6b7280))
                .child(strings.from)
                .child(div().text_color(rgb(0x111827)).child(message.from))
                .child("·")
                .child(message.time),
        )
        .child(body)
        .child(replies)
    }

    fn status(&self) -> gpui::Div {
        let strings = self.ui.strings();
        self.row()
            .debug_selector(|| "inbox-status".into())
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(rgb(0xe5e7eb))
            .text_size(px(12.))
            .text_color(rgb(0x6b7280))
            .child(match &self.clicked {
                Some(target) => format!("{}: {target}", strings.clicked),
                None => strings.nothing_clicked.to_owned(),
            })
    }
}

impl Render for InboxLab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(parley) = &self.parley
            && self.applied != self.plain_on_parley
        {
            if self.plain_on_parley {
                xgpui_parley::install_text_layout(parley.clone(), cx);
            } else {
                cx.clear_text_layout_provider();
            }
            self.applied = self.plain_on_parley;
        }
        let header = self.header(cx);
        let list = self.list(cx);
        let message = self.message(cx);
        div()
            .id("inbox-lab")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x111827))
            .font_family("IBM Plex Sans")
            .text_size(px(14.))
            .child(header)
            .child(
                self.row().flex_1().min_h_0().child(list).child(
                    div()
                        .id("inbox-scroll")
                        .flex_1()
                        .min_w_0()
                        .overflow_y_scroll()
                        .child(message),
                ),
            )
            .child(self.status())
    }
}
