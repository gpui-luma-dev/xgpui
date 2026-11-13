//! Replaceable artwork for shell-drawn window controls.
//!
//! The shell keeps control order, availability, capability filtering, window
//! state, accessible names, keyboard/pointer activation, drag isolation and the
//! keyboard focus ring. A [`ShellControlPresenter`] supplies everything drawn
//! inside the control: background, icon, size of the icon and state treatment.
use crate::ShellColors;
use gpui::{AnyElement, App, Pixels, Window, WindowAppearance};
use std::{fmt, rc::Rc};

/// A window control drawn by the shell.
///
/// `Restore` replaces `Maximize` while the window is maximized; both dispatch
/// the same maximize/restore operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ShellControl {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl ShellControl {
    /// The accessible name the shell gives this control.
    pub fn accessible_label(self) -> &'static str {
        match self {
            Self::Minimize => "Minimize window",
            Self::Maximize => "Maximize window",
            Self::Restore => "Restore window",
            Self::Close => "Close window",
        }
    }
}

/// Everything a presenter may use to draw one control.
///
/// New fields may be added in minor releases; read the fields you need.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ShellControlState {
    /// Which control to draw.
    pub control: ShellControl,
    /// The pointer is over the control (or hover is simulated in a preview).
    pub hovered: bool,
    /// The primary button is held down on the control.
    pub pressed: bool,
    /// Keyboard focus is visible on the control (or simulated in a preview).
    /// The shell already paints a focus ring; use this only for extra treatment.
    pub focused: bool,
    /// The window is active.
    pub window_active: bool,
    /// Light or dark appearance used for the frame.
    pub appearance: WindowAppearance,
    /// High contrast is in effect. Presenters should switch to high-contrast
    /// artwork; `colors` already holds the shell's high-contrast tokens.
    pub high_contrast: bool,
    /// The frame's resolved colors for this appearance and activation.
    pub colors: ShellColors,
    /// The focus-ring color the shell uses (0xRRGGBB).
    pub focus_color: u32,
    /// Logical text/target scale (0.5–4), independent of device DPI.
    pub scale: f32,
    /// Side of the square clickable target. Presenter content is centered in it
    /// and clipped to it; draw the icon smaller to keep a comfortable target.
    pub target_size: Pixels,
    /// Drawn in an inert [`crate::ClientFramePreview`], not a real window.
    pub preview: bool,
}

type Render = dyn Fn(&ShellControlState, &mut Window, &mut App) -> AnyElement;

/// App-supplied artwork for shell-drawn window controls.
///
/// Called once per visible control per frame. Return ordinary GPUI elements,
/// including `svg()` assets; don't attach click handlers, since activation,
/// keyboard handling and accessibility stay with the shell.
#[derive(Clone)]
pub struct ShellControlPresenter(Rc<Render>);

impl ShellControlPresenter {
    pub fn new(
        render: impl Fn(&ShellControlState, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self(Rc::new(render))
    }

    pub(crate) fn render(
        &self,
        state: &ShellControlState,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        (self.0)(state, window, cx)
    }
}

impl fmt::Debug for ShellControlPresenter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ShellControlPresenter")
    }
}
