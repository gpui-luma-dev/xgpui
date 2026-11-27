//! Native cursor selection and availability for the active Linux connection.

use std::rc::{Rc, Weak};

use anyhow::Context as _;
use gpui::{Platform, Window, WindowingModes};
#[cfg(feature = "wayland")]
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

use super::{DisplayConnection, LinuxPlatform};

macro_rules! cursor_catalog {
    ($($cursor:ident => $label:literal, [$($name:literal),+]),+ $(,)?) => {
        /// A cursor intent from the version 1 Wayland cursor-shape catalog.
        ///
        /// On X11 or Wayland without the cursor-shape protocol, the backend
        /// tries the named Xcursor theme aliases for the same intent.
        #[non_exhaustive]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum LinuxSystemCursor {
            $(#[doc = $label] $cursor,)+
        }

        impl LinuxSystemCursor {
            /// Cursor intents supported by this API.
            pub const ALL: &[Self] = &[$(Self::$cursor,)+];

            /// Display name for this intent.
            pub const fn name(self) -> &'static str {
                match self { $(Self::$cursor => $label,)+ }
            }

            #[cfg(any(feature = "wayland", feature = "x11"))]
            pub(crate) fn theme_names(self) -> &'static [&'static str] {
                match self { $(Self::$cursor => &[$($name),+],)+ }
            }

            #[cfg(feature = "wayland")]
            pub(crate) fn wayland_shape(self) -> Shape {
                match self { $(Self::$cursor => Shape::$cursor,)+ }
            }
        }
    }
}

cursor_catalog! {
    Default => "Default pointer", ["default", "left_ptr"],
    ContextMenu => "Context menu", ["context-menu"],
    Help => "Help", ["help", "question_arrow"],
    Pointer => "Pointing hand", ["pointer", "hand2"],
    Progress => "Working in background", ["progress", "left_ptr_watch"],
    Wait => "Wait", ["wait", "watch"],
    Cell => "Cell selection", ["cell", "plus"],
    Crosshair => "Crosshair", ["crosshair", "cross"],
    Text => "Text I-beam", ["text", "xterm"],
    VerticalText => "Vertical text", ["vertical-text"],
    Alias => "Link", ["alias", "dnd-link"],
    Copy => "Copy", ["copy", "dnd-copy"],
    Move => "Move", ["move", "fleur"],
    NoDrop => "No drop", ["no-drop", "dnd-none"],
    NotAllowed => "Not allowed", ["not-allowed", "crossed_circle"],
    Grab => "Grab", ["grab", "openhand"],
    Grabbing => "Grabbing", ["grabbing", "closedhand"],
    EResize => "Resize right", ["e-resize", "right_side"],
    NResize => "Resize up", ["n-resize", "top_side"],
    NeResize => "Resize up and right", ["ne-resize", "top_right_corner"],
    NwResize => "Resize up and left", ["nw-resize", "top_left_corner"],
    SResize => "Resize down", ["s-resize", "bottom_side"],
    SeResize => "Resize down and right", ["se-resize", "bottom_right_corner"],
    SwResize => "Resize down and left", ["sw-resize", "bottom_left_corner"],
    WResize => "Resize left", ["w-resize", "left_side"],
    EwResize => "Resize left–right", ["ew-resize", "sb_h_double_arrow"],
    NsResize => "Resize up–down", ["ns-resize", "sb_v_double_arrow"],
    NeswResize => "Resize northeast–southwest", ["nesw-resize", "size_bdiag", "fd_double_arrow"],
    NwseResize => "Resize northwest–southeast", ["nwse-resize", "size_fdiag", "bd_double_arrow"],
    ColResize => "Column divider", ["col-resize"],
    RowResize => "Row divider", ["row-resize"],
    AllScroll => "Scroll in all directions", ["all-scroll"],
    ZoomIn => "Zoom in", ["zoom-in"],
    ZoomOut => "Zoom out", ["zoom-out"],
}

/// The cursor path active for the application's display connection.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxCursorBackend {
    Headless,
    WaylandShape,
    WaylandTheme,
    X11,
}

/// How a native cursor request resolves in the current session.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxCursorAvailability {
    /// The compositor accepts the standard Wayland shape. Its artwork is
    /// selected by the compositor.
    WaylandShape,
    /// Name resolved by the cursor loader. X11 lookup can use the theme or
    /// the legacy X11 cursor font.
    XcursorName(&'static str),
    /// None of the requested aliases is available, or the app is headless.
    Unavailable,
}

/// Access to native cursors on a GPUI app's Linux platform.
#[derive(Clone)]
pub struct LinuxCursorAccess {
    platform: Weak<LinuxPlatform>,
}

impl LinuxCursorAccess {
    /// Create a Linux platform and an access handle for it. Pass the platform
    /// to `Application::with_platform`. Query cursors after the app starts.
    pub fn create_platform(modes: WindowingModes) -> (Rc<dyn Platform>, Self) {
        let platform = Rc::new(LinuxPlatform::new(modes));
        let access = Self {
            platform: Rc::downgrade(&platform),
        };
        (platform, access)
    }

    /// Report the active cursor path for this platform.
    pub fn backend(&self) -> LinuxCursorBackend {
        let Some(platform) = self.platform.upgrade() else {
            return LinuxCursorBackend::Headless;
        };
        match &*platform.connection.borrow() {
            DisplayConnection::Headless(_) => LinuxCursorBackend::Headless,
            #[cfg(feature = "wayland")]
            DisplayConnection::Wayland(connection) => connection.cursor_backend(),
            #[cfg(feature = "x11")]
            DisplayConnection::X11(_) => LinuxCursorBackend::X11,
        }
    }

    /// Report availability for the window's active connection.
    /// X11 uses the cursor configuration loaded when it connected.
    #[allow(
        unused_variables,
        reason = "no cursor backend in a headless-only build"
    )]
    pub fn availability(
        &self,
        window: &Window,
        cursor: LinuxSystemCursor,
    ) -> LinuxCursorAvailability {
        let Some(platform) = self.platform.upgrade() else {
            return LinuxCursorAvailability::Unavailable;
        };
        match &*platform.connection.borrow() {
            DisplayConnection::Headless(_) => LinuxCursorAvailability::Unavailable,
            #[cfg(feature = "wayland")]
            DisplayConnection::Wayland(connection) => {
                connection.system_cursor_availability(window.window_handle(), cursor)
            }
            #[cfg(feature = "x11")]
            DisplayConnection::X11(connection) => {
                connection.system_cursor_availability(window.window_handle(), cursor)
            }
        }
    }

    /// Select a native cursor for a window owned by this platform. Pass
    /// `None` to restore GPUI hover selection. An unavailable cursor returns
    /// an error and preserves the current selection. The backend can clear
    /// the override when a modal window blocks the pointer input.
    #[allow(
        unused_variables,
        reason = "no cursor backend in a headless-only build"
    )]
    pub fn set_for_window(
        &self,
        window: &Window,
        cursor: Option<LinuxSystemCursor>,
    ) -> anyhow::Result<()> {
        let platform = self
            .platform
            .upgrade()
            .context("Linux platform has closed")?;
        match &*platform.connection.borrow() {
            DisplayConnection::Headless(_) => {
                anyhow::bail!("native cursors require a Linux display connection")
            }
            #[cfg(feature = "wayland")]
            DisplayConnection::Wayland(connection) => {
                connection.set_system_cursor(window.window_handle(), cursor)
            }
            #[cfg(feature = "x11")]
            DisplayConnection::X11(connection) => {
                connection.set_system_cursor(window.window_handle(), cursor)
            }
        }
    }
}
