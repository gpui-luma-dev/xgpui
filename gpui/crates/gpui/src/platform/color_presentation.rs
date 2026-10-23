//! Window presentation intent, independent of native UI color types.

/// Presentation intent for one window. Existing UI colors retain sRGB meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowColorMode {
    /// Preserve the platform's existing presentation behavior.
    #[default]
    Default,
    /// Compose SDR wide-gamut content with ordinary sRGB controls.
    WideGamutSdr,
}

/// Why negotiated output differs from the requested presentation class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowColorFallback {
    /// The host explicitly maps wide content to an SDR display's sRGB output.
    SdrDisplay,
    /// Another platform policy restricts presentation.
    PlatformPolicy,
}

/// An immutable snapshot of one window's negotiated presentation configuration.
/// Effective mode describes the next-frame configuration, not monitor delivery.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowColorStatus {
    requested: WindowColorMode,
    effective: WindowColorMode,
    supports_wide_gamut_sdr: bool,
    display_wide_gamut: Option<bool>,
    fallback: Option<WindowColorFallback>,
}
impl WindowColorStatus {
    /// Construct a backend report. This does not change a window's configuration.
    pub fn new(
        requested: WindowColorMode,
        effective: WindowColorMode,
        supports_wide_gamut_sdr: bool,
        display_wide_gamut: Option<bool>,
        fallback: Option<WindowColorFallback>,
    ) -> Self {
        Self {
            requested,
            effective,
            supports_wide_gamut_sdr,
            display_wide_gamut,
            fallback,
        }
    }
    /// The most recently accepted request.
    pub fn requested(&self) -> WindowColorMode {
        self.requested
    }
    /// The negotiated output class; independent of internal working textures.
    pub fn effective(&self) -> WindowColorMode {
        self.effective
    }
    /// Whether this backend can negotiate wide-gamut SDR presentation.
    pub fn supports_wide_gamut_sdr(&self) -> bool {
        self.supports_wide_gamut_sdr
    }
    /// OS display-profile evidence; `None` means unknown, not unsupported.
    pub fn display_wide_gamut(&self) -> Option<bool> {
        self.display_wide_gamut
    }
    /// The reason for an active output fallback, if any.
    pub fn fallback(&self) -> Option<WindowColorFallback> {
        self.fallback
    }
}
