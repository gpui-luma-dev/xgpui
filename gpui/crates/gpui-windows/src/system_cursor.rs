use gpui::Window;
use windows::{
    Win32::UI::WindowsAndMessaging::{HCURSOR, LoadCursorW},
    core::PCWSTR,
};

macro_rules! system_cursors {
    (
        standard { $($standard:ident = $standard_id:literal => $standard_name:literal),* $(,)? }
        additional { $($additional:ident = $additional_id:literal => $additional_name:literal),* $(,)? }
    ) => {
        /// A predefined Win32 cursor resource loaded from Windows.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[non_exhaustive]
        #[repr(u16)]
        pub enum WindowsSystemCursor {
            $(#[doc = $standard_name] $standard = $standard_id,)*
            $(#[doc = $additional_name] $additional = $additional_id,)*
        }

        impl WindowsSystemCursor {
            /// Standard cursor identifiers listed in the Win32 cursor overview.
            pub const STANDARD: &[Self] = &[$(Self::$standard),*];
            /// Additional predefined resources documented by Microsoft.
            /// Availability can vary by Windows version.
            pub const ADDITIONAL: &[Self] = &[$(Self::$additional),*];

            /// Win32 identifier or descriptive name for the predefined resource.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$standard => $standard_name,)*
                    $(Self::$additional => $additional_name,)*
                }
            }
        }
    }
}

// https://learn.microsoft.com/en-us/windows/win32/menurc/about-cursors
system_cursors! {
    standard {
        Arrow = 32512 => "IDC_ARROW",
        IBeam = 32513 => "IDC_IBEAM",
        Wait = 32514 => "IDC_WAIT",
        Crosshair = 32515 => "IDC_CROSS",
        UpArrow = 32516 => "IDC_UPARROW",
        ResizeNorthwestSoutheast = 32642 => "IDC_SIZENWSE",
        ResizeNortheastSouthwest = 32643 => "IDC_SIZENESW",
        ResizeHorizontal = 32644 => "IDC_SIZEWE",
        ResizeVertical = 32645 => "IDC_SIZENS",
        Move = 32646 => "IDC_SIZEALL",
        Unavailable = 32648 => "IDC_NO",
        Hand = 32649 => "IDC_HAND",
        AppStarting = 32650 => "IDC_APPSTARTING",
        Help = 32651 => "IDC_HELP",
        Pin = 32671 => "IDC_PIN",
        Person = 32672 => "IDC_PERSON",
    }
    additional {
        Pen = 32631 => "Pen",
        ScrollVertical = 32652 => "Scroll north/south",
        ScrollHorizontal = 32653 => "Scroll west/east",
        ScrollAll = 32654 => "Scroll in four directions",
        ScrollNorth = 32655 => "Scroll north",
        ScrollSouth = 32656 => "Scroll south",
        ScrollWest = 32657 => "Scroll west",
        ScrollEast = 32658 => "Scroll east",
        ScrollNorthwest = 32659 => "Scroll northwest",
        ScrollNortheast = 32660 => "Scroll northeast",
        ScrollSouthwest = 32661 => "Scroll southwest",
        ScrollSoutheast = 32662 => "Scroll southeast",
        ArrowCd = 32663 => "Arrow with CD",
    }
}

impl WindowsSystemCursor {
    /// Load the Windows cursor, honoring the system cursor scheme. The returned
    /// handle is shared. The caller must leave it owned by Windows.
    pub fn load(self) -> windows::core::Result<HCURSOR> {
        // MAKEINTRESOURCEW stores a 16-bit resource ID in the pointer value.
        let resource = PCWSTR(self as u16 as usize as *const u16);
        unsafe { LoadCursorW(None, resource) }
    }

    /// Use a system cursor over the client area of a window owned by this
    /// backend. Pass `None` to restore GPUI's normal hitbox-based selection.
    /// A stationary pointer refreshes through the Windows message loop.
    /// Loading an unavailable cursor returns an error and keeps the current
    /// selection.
    pub fn set_for_window(window: &Window, cursor: Option<Self>) -> anyhow::Result<()> {
        let cursor = cursor.map(Self::load).transpose()?;
        crate::window::apply_system_cursor(window, cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_cursors_load_the_windows_resources() {
        use windows::Win32::UI::WindowsAndMessaging::{
            IDC_APPSTARTING, IDC_ARROW, IDC_HAND, IDC_HELP, IDC_SIZEALL, IDC_WAIT,
        };

        for (cursor, resource) in [
            (WindowsSystemCursor::Arrow, IDC_ARROW),
            (WindowsSystemCursor::Hand, IDC_HAND),
            (WindowsSystemCursor::Wait, IDC_WAIT),
            (WindowsSystemCursor::AppStarting, IDC_APPSTARTING),
            (WindowsSystemCursor::Move, IDC_SIZEALL),
            (WindowsSystemCursor::Help, IDC_HELP),
        ] {
            assert_eq!(
                cursor.load().unwrap(),
                unsafe { LoadCursorW(None, resource) }.unwrap()
            );
        }
    }
}
