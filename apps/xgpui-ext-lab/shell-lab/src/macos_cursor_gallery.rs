#[cfg(not(target_os = "macos"))]
pub(super) fn open(_: &mut gpui::App) -> anyhow::Result<()> {
    anyhow::bail!("The AppKit cursor gallery requires macOS");
}

#[cfg(target_os = "macos")]
pub(super) use appkit::open;

#[cfg(target_os = "macos")]
mod appkit {
    use gpui::{prelude::*, *};
    use gpui_macos::system_cursor::{
        MacSystemCursor as Cursor, NSCursorFrameResizeDirections as Directions,
        NSCursorFrameResizePosition as Position,
    };
    use std::{cell::Cell, rc::Rc};

    const STANDARD: &[(Cursor, &str)] = &[
        (Cursor::Arrow, "Arrow"),
        (Cursor::IBeam, "Text I-beam"),
        (Cursor::VerticalText, "Vertical text"),
        (Cursor::Crosshair, "Crosshair"),
        (Cursor::PointingHand, "Pointing hand"),
        (Cursor::OpenHand, "Open hand"),
        (Cursor::ClosedHand, "Closed hand"),
        (Cursor::OperationNotAllowed, "Operation not allowed"),
        (Cursor::DisappearingItem, "Disappearing item"),
        (Cursor::DragCopy, "Drag copy"),
        (Cursor::DragLink, "Drag link"),
        (Cursor::ContextualMenu, "Contextual menu"),
        (Cursor::ZoomIn, "Zoom in"),
        (Cursor::ZoomOut, "Zoom out"),
    ];

    const DIVIDERS: &[(Cursor, &str)] = &[
        (Cursor::ColumnResize, "Column · both directions"),
        (Cursor::ColumnResizeLeft, "Column · left"),
        (Cursor::ColumnResizeRight, "Column · right"),
        (Cursor::RowResize, "Row · both directions"),
        (Cursor::RowResizeUp, "Row · up"),
        (Cursor::RowResizeDown, "Row · down"),
    ];

    const LEGACY: &[(Cursor, &str)] = &[
        (Cursor::ResizeLeft, "Left"),
        (Cursor::ResizeRight, "Right"),
        (Cursor::ResizeLeftRight, "Left and right"),
        (Cursor::ResizeUp, "Up"),
        (Cursor::ResizeDown, "Down"),
        (Cursor::ResizeUpDown, "Up and down"),
    ];

    const FRAME_POSITIONS: &[(Position, &str)] = &[
        (Position::Top, "Top"),
        (Position::TopRight, "Top right"),
        (Position::Right, "Right"),
        (Position::BottomRight, "Bottom right"),
        (Position::Bottom, "Bottom"),
        (Position::BottomLeft, "Bottom left"),
        (Position::Left, "Left"),
        (Position::TopLeft, "Top left"),
    ];

    const FRAME_DIRECTIONS: &[(Directions, &str)] = &[
        (Directions::Inward, "inward"),
        (Directions::Outward, "outward"),
        (Directions::All, "both directions"),
    ];

    struct CursorTile {
        cursor: Cursor,
        label: SharedString,
        available: bool,
    }

    impl CursorTile {
        fn new(cursor: Cursor, label: impl Into<SharedString>) -> Self {
            Self {
                cursor,
                label: label.into(),
                available: cursor.load().is_some(),
            }
        }
    }

    pub(crate) fn open(cx: &mut App) -> anyhow::Result<()> {
        let bounds = Bounds::centered(None, size(px(860.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("macOS cursor gallery".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Gallery::new()),
        )?;
        Ok(())
    }

    struct Gallery {
        sections: Vec<(&'static str, Vec<CursorTile>)>,
        active: Rc<Cell<Option<Cursor>>>,
    }

    impl Gallery {
        fn new() -> Self {
            let tiles = |cursors: &[(Cursor, &str)]| {
                cursors
                    .iter()
                    .map(|&(cursor, label)| CursorTile::new(cursor, label.to_owned()))
                    .collect()
            };
            let frames = FRAME_POSITIONS
                .iter()
                .flat_map(|&(position, edge)| {
                    FRAME_DIRECTIONS
                        .iter()
                        .map(move |&(directions, direction)| {
                            CursorTile::new(
                                Cursor::FrameResize {
                                    position,
                                    directions,
                                },
                                format!("{edge} · {direction}"),
                            )
                        })
                })
                .collect();
            Self {
                sections: vec![
                    ("Standard AppKit cursors", tiles(STANDARD)),
                    ("Row and column dividers", tiles(DIVIDERS)),
                    ("Frame edges and corners", frames),
                    ("Legacy AppKit resize cursors", tiles(LEGACY)),
                ],
                active: Rc::default(),
            }
        }
    }

    impl Render for Gallery {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("macos-cursors")
                .size_full()
                .overflow_y_scroll()
                .bg(rgb(0xf6f8fa))
                .text_color(rgb(0x17212b))
                .p_5()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::BOLD)
                        .child("macOS cursor gallery"),
                )
                .child("Hover to see the cursor loaded directly from AppKit.")
                .children(self.sections.iter().map(|(title, tiles)| {
                    div()
                        .flex()
                        .flex_col()
                        .flex_shrink_0()
                        .gap_2()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(*title))
                        .child(div().flex().flex_wrap().gap_2().children(tiles.iter().map(
                            |tile| {
                                let cursor = tile.cursor;
                                let active = self.active.clone();
                                div()
                                    .id(tile.label.clone())
                                    .w(px(196.))
                                    .min_h(px(54.))
                                    .p_2()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(0xc4cfd9))
                                    .bg(rgb(0xffffff))
                                    .text_sm()
                                    .child(tile.label.clone())
                                    .when(!tile.available, |tile| {
                                        tile.child(
                                            div().text_xs().child("Unavailable on this macOS"),
                                        )
                                    })
                                    .when(tile.available, |tile| {
                                        tile.hover(|style| style.bg(rgb(0xe3efff))).on_hover(
                                            move |hovered, window, _| {
                                                let next = if *hovered {
                                                    Some(cursor)
                                                } else if active.get() == Some(cursor) {
                                                    None
                                                } else {
                                                    return;
                                                };
                                                match Cursor::set_for_window(window, next) {
                                                    Ok(()) => active.set(next),
                                                    Err(error) => log::warn!(
                                                        "Could not select {cursor:?}: {error:#}"
                                                    ),
                                                }
                                            },
                                        )
                                    })
                            },
                        )))
                }))
        }
    }
}
