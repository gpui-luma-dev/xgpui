//! Public AppKit cursor selection for macOS applications.

use anyhow::Context as _;
use gpui::Window;
use objc2::{ClassType, MainThreadMarker, msg_send, rc::Retained, runtime::Bool, sel};
use objc2_app_kit::{NSCursor, NSHorizontalDirections, NSVerticalDirections};

pub use objc2_app_kit::{NSCursorFrameResizeDirections, NSCursorFrameResizePosition};

/// A built-in cursor exposed by a public AppKit `NSCursor` selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MacSystemCursor {
    Arrow,
    IBeam,
    VerticalText,
    Crosshair,
    PointingHand,
    OpenHand,
    ClosedHand,
    OperationNotAllowed,
    DisappearingItem,
    DragCopy,
    DragLink,
    ContextualMenu,
    ZoomIn,
    ZoomOut,
    ColumnResize,
    ColumnResizeLeft,
    ColumnResizeRight,
    RowResize,
    RowResizeUp,
    RowResizeDown,
    ResizeLeft,
    ResizeRight,
    ResizeLeftRight,
    ResizeUp,
    ResizeDown,
    ResizeUpDown,
    /// Resize a rectangular frame at an edge or corner.
    FrameResize {
        position: NSCursorFrameResizePosition,
        /// Use `Inward`, `Outward`, or `All`.
        directions: NSCursorFrameResizeDirections,
    },
}

impl MacSystemCursor {
    /// Load a retained AppKit cursor on the main thread. Returns `None` when
    /// the selector is unavailable on this macOS version, the frame directions
    /// are invalid, or the call is made off the main thread.
    pub fn load(self) -> Option<Retained<NSCursor>> {
        let _main_thread = MainThreadMarker::new()?;

        macro_rules! cursor {
            ($method:ident) => {
                cursor!(($method), NSCursor::$method())
            };
            (($($selector:tt)+), $load:expr) => {{
                let available: Bool = unsafe {
                    msg_send![NSCursor::class(), respondsToSelector: sel!($($selector)+)]
                };
                available.as_bool().then(|| $load)
            }};
        }

        // The legacy AppKit resize cursors remain useful for comparison and
        // for systems that predate the newer directional selectors.
        #[allow(deprecated)]
        match self {
            Self::Arrow => cursor!(arrowCursor),
            Self::IBeam => cursor!(IBeamCursor),
            Self::VerticalText => cursor!(IBeamCursorForVerticalLayout),
            Self::Crosshair => cursor!(crosshairCursor),
            Self::PointingHand => cursor!(pointingHandCursor),
            Self::OpenHand => cursor!(openHandCursor),
            Self::ClosedHand => cursor!(closedHandCursor),
            Self::OperationNotAllowed => cursor!(operationNotAllowedCursor),
            Self::DisappearingItem => cursor!(disappearingItemCursor),
            Self::DragCopy => cursor!(dragCopyCursor),
            Self::DragLink => cursor!(dragLinkCursor),
            Self::ContextualMenu => cursor!(contextualMenuCursor),
            Self::ZoomIn => cursor!(zoomInCursor),
            Self::ZoomOut => cursor!(zoomOutCursor),
            Self::ColumnResize => cursor!(columnResizeCursor),
            Self::ColumnResizeLeft => cursor!(
                (columnResizeCursorInDirections:),
                NSCursor::columnResizeCursorInDirections(NSHorizontalDirections::Left)
            ),
            Self::ColumnResizeRight => cursor!(
                (columnResizeCursorInDirections:),
                NSCursor::columnResizeCursorInDirections(NSHorizontalDirections::Right)
            ),
            Self::RowResize => cursor!(rowResizeCursor),
            Self::RowResizeUp => cursor!(
                (rowResizeCursorInDirections:),
                NSCursor::rowResizeCursorInDirections(NSVerticalDirections::Up)
            ),
            Self::RowResizeDown => cursor!(
                (rowResizeCursorInDirections:),
                NSCursor::rowResizeCursorInDirections(NSVerticalDirections::Down)
            ),
            Self::ResizeLeft => cursor!(resizeLeftCursor),
            Self::ResizeRight => cursor!(resizeRightCursor),
            Self::ResizeLeftRight => cursor!(resizeLeftRightCursor),
            Self::ResizeUp => cursor!(resizeUpCursor),
            Self::ResizeDown => cursor!(resizeDownCursor),
            Self::ResizeUpDown => cursor!(resizeUpDownCursor),
            Self::FrameResize {
                position,
                directions,
            } => {
                if directions.is_empty() || !NSCursorFrameResizeDirections::All.contains(directions)
                {
                    return None;
                }
                cursor!(
                    (frameResizeCursorFromPosition:inDirections:),
                    NSCursor::frameResizeCursorFromPosition_inDirections(position, directions)
                )
            }
        }
    }

    /// Use an AppKit cursor over this window's content view. Pass `None` to
    /// resume GPUI's normal hover selection. Call this on the AppKit main
    /// thread. An unavailable cursor returns an error and keeps the current
    /// selection.
    pub fn set_for_window(window: &Window, cursor: Option<Self>) -> anyhow::Result<()> {
        let _main_thread =
            MainThreadMarker::new().context("AppKit cursors require the main thread")?;
        let cursor = cursor
            .map(|cursor| {
                cursor
                    .load()
                    .with_context(|| format!("AppKit cursor {cursor:?} is unavailable"))
            })
            .transpose()?;
        crate::window::apply_system_cursor(window, cursor)
    }
}
