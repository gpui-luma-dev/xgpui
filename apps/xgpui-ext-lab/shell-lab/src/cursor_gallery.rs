use gpui::{prelude::*, *};
#[cfg(target_os = "windows")]
use gpui_windows::WindowsSystemCursor;
#[cfg(target_os = "windows")]
use std::{cell::Cell, rc::Rc};

type CursorSamples = &'static [(CursorStyle, &'static str)];

const CURSOR_GROUPS: &[(&str, CursorSamples)] = &[
    (
        "Hands and dragging",
        &[
            (CursorStyle::Arrow, "Arrow pointer"),
            (CursorStyle::PointingHand, "Pointing index finger"),
            (CursorStyle::OpenHand, "Arrow fallback (IDC_ARROW)"),
            (CursorStyle::ClosedHand, "Arrow fallback (IDC_ARROW)"),
            (CursorStyle::DragCopy, "Arrow fallback (IDC_ARROW)"),
            (CursorStyle::DragLink, "Pointing-hand fallback (IDC_HAND)"),
            (CursorStyle::OperationNotAllowed, "Circle with a slash"),
        ],
    ),
    (
        "Resize",
        &[
            (CursorStyle::ResizeLeft, "Two-way Windows fallback ↔"),
            (CursorStyle::ResizeRight, "Two-way Windows fallback ↔"),
            (CursorStyle::ResizeLeftRight, "Left–right resize arrows"),
            (CursorStyle::ResizeColumn, "Two-way Windows fallback ↔"),
            (CursorStyle::ResizeUp, "Two-way Windows fallback ↕"),
            (CursorStyle::ResizeDown, "Two-way Windows fallback ↕"),
            (CursorStyle::ResizeUpDown, "Up–down resize arrows"),
            (CursorStyle::ResizeRow, "Two-way Windows fallback ↕"),
            (CursorStyle::ResizeUpLeftDownRight, "Diagonal resize ↖ ↘"),
            (CursorStyle::ResizeUpRightDownLeft, "Diagonal resize ↗ ↙"),
        ],
    ),
    (
        "Text and selection",
        &[
            (CursorStyle::IBeam, "Vertical I-beam for text"),
            (
                CursorStyle::IBeamCursorForVerticalLayout,
                "Vertical I-beam fallback (IDC_IBEAM)",
            ),
            (CursorStyle::Crosshair, "Crosshair for precise selection"),
            (CursorStyle::ContextualMenu, "Arrow fallback (IDC_ARROW)"),
        ],
    ),
];

pub(super) fn open(cx: &mut App) -> anyhow::Result<()> {
    anyhow::ensure!(
        cx.platform_info().os == OperatingSystem::Windows,
        "The cursor gallery requires Windows"
    );
    let bounds = Bounds::centered(None, size(px(860.), px(620.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Windows cursor gallery".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |_, cx| cx.new(|_| CursorGallery::default()),
    )?;
    Ok(())
}

#[derive(Default)]
struct CursorGallery {
    #[cfg(target_os = "windows")]
    active_system_cursor: Rc<Cell<Option<WindowsSystemCursor>>>,
}

#[cfg(target_os = "windows")]
impl CursorGallery {
    fn windows_cursors(&self, title: &'static str, cursors: &[WindowsSystemCursor]) -> AnyElement {
        let active = &self.active_system_cursor;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(cursors.iter().map(|&cursor| {
                        let available = cursor.load().is_ok();
                        let active = active.clone();
                        div()
                            .id(("system-cursor", cursor as u16 as usize))
                            .w(px(196.))
                            .min_h(px(58.))
                            .p_2()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0xc4cfd9))
                            .bg(rgb(0xffffff))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(cursor.name()),
                            )
                            .child(div().text_xs().child(if available {
                                format!("Resource {}", cursor as u16)
                            } else {
                                format!("Resource {} · Unavailable", cursor as u16)
                            }))
                            .when(available, |tile| {
                                tile.hover(|style| style.bg(rgb(0xe3efff))).on_hover(
                                    move |hovered, window, _| {
                                        let next = if *hovered {
                                            Some(cursor)
                                        } else if active.get() == Some(cursor) {
                                            None
                                        } else {
                                            return;
                                        };
                                        match WindowsSystemCursor::set_for_window(window, next) {
                                            Ok(()) => active.set(next),
                                            Err(error) => {
                                                log::warn!("Could not select {cursor:?}: {error:#}")
                                            }
                                        }
                                    },
                                )
                            })
                    })),
            )
            .into_any_element()
    }
}

impl Render for CursorGallery {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let gallery = div()
            .id("cursor-gallery")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(0xf6f8fa))
            .text_color(rgb(0x17212b))
            .p_5()
            .flex()
            .flex_col()
            .gap_4();
        #[cfg(target_os = "windows")]
        let gallery = gallery
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Windows cursor gallery"),
            )
            .child(div().child("Hover to see the cursor loaded directly from Windows."))
            .child(self.windows_cursors("Standard Win32 cursors", WindowsSystemCursor::STANDARD))
            .child(self.windows_cursors(
                "Additional Win32 resources",
                WindowsSystemCursor::ADDITIONAL,
            ));
        gallery
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child("GPUI cursor styles on Windows"),
            )
            .child(div().child("Each tile names a GPUI style and the cursor Windows uses for it."))
            .children(CURSOR_GROUPS.iter().map(|&(label, styles)| {
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
                    .child(div().flex().flex_wrap().gap_2().children(styles.iter().map(
                        |&(style, windows_cursor)| {
                            div()
                                .id(SharedString::from(format!("cursor-{style:?}")))
                                .w(px(196.))
                                .min_h(px(64.))
                                .flex()
                                .flex_col()
                                .justify_center()
                                .gap_1()
                                .px_2()
                                .py_2()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(0xc4cfd9))
                                .bg(rgb(0xffffff))
                                .hover(|tile| tile.bg(rgb(0xe3efff)))
                                .cursor(style)
                                .child(div().text_xs().child(format!("{style:?}")))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(windows_cursor),
                                )
                        },
                    )))
            }))
    }
}
