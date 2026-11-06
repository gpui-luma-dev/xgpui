//! Shared application support for the unpublished native labs.
use gpui::{prelude::*, *};
pub use xgpui_shell::ShellWindow as LabWindow;
use xgpui_shell::{ShellApp, ShellOptions, ShellWindowTitle};

/// Compatibility adapter for existing lab callers; reusable shell code lives in xgpui-shell.
pub trait LabWindowTitle {
    fn set_lab_title(&mut self, title: &str);
    fn lab_title(&self) -> String;
}
impl LabWindowTitle for Window {
    fn set_lab_title(&mut self, title: &str) {
        self.set_shell_title(title);
    }
    fn lab_title(&self) -> String {
        self.shell_title()
    }
}
pub trait LabApp {
    fn open_lab_window<V: Render + 'static>(
        &mut self,
        options: WindowOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<WindowHandle<LabWindow>>;
}
impl LabApp for App {
    fn open_lab_window<V: Render + 'static>(
        &mut self,
        options: WindowOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<WindowHandle<LabWindow>> {
        self.open_shell_window(options, ShellOptions::default(), build)
    }
}

/// Attempt native wide SDR, retaining a usable default presentation on failure.
/// A notice is returned whenever the effective presentation is standard SDR.
pub fn negotiate_wide(window: &mut Window) -> Option<String> {
    negotiate_with(|mode| window.set_color_mode(mode))
}

fn negotiate_with(
    mut set_mode: impl FnMut(WindowColorMode) -> anyhow::Result<WindowColorStatus>,
) -> Option<String> {
    match set_mode(WindowColorMode::WideGamutSdr) {
        Ok(status) if status.effective() == WindowColorMode::WideGamutSdr => None,
        result => {
            let reason = match result {
                Ok(_) => "the backend selected standard SDR".to_owned(),
                Err(error) => error.to_string(),
            };
            eprintln!("Wide-gamut presentation diagnostic: {reason}");
            let fallback = set_mode(WindowColorMode::Default);
            Some(match fallback {
                Ok(_) => "Wide-gamut output isn’t available with the current display and graphics setup. Using standard SDR.".into(),
                Err(error) => {
                    format!("Wide-gamut output isn’t available with the current display and graphics setup. Could not switch to standard SDR: {error}")
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn unsupported_wide_resets_to_sdr_with_a_user_facing_notice() {
        let mut requests = Vec::new();
        let notice = negotiate_with(|mode| {
            requests.push(mode);
            if mode == WindowColorMode::WideGamutSdr {
                anyhow::bail!("unsupported backend");
            }
            Ok(WindowColorStatus::default())
        })
        .unwrap();
        assert_eq!(
            requests,
            [WindowColorMode::WideGamutSdr, WindowColorMode::Default]
        );
        assert!(notice.contains("Using standard SDR"));
        assert!(!notice.contains("unsupported backend"));
    }

    #[::core::prelude::v1::test]
    fn accepted_request_with_effective_sdr_still_reports_fallback() {
        let mut requests = Vec::new();
        let notice = negotiate_with(|mode| {
            requests.push(mode);
            Ok(WindowColorStatus::new(
                mode,
                WindowColorMode::Default,
                true,
                Some(false),
                Some(WindowColorFallback::SdrDisplay),
            ))
        });
        assert!(notice.unwrap().contains("Using standard SDR"));
        assert_eq!(requests.len(), 2);
    }

    #[::core::prelude::v1::test]
    fn supported_wide_remains_enabled_without_an_sdr_request() {
        let mut requests = Vec::new();
        assert!(
            negotiate_with(|mode| {
                requests.push(mode);
                Ok(WindowColorStatus::new(mode, mode, true, Some(true), None))
            })
            .is_none()
        );
        assert_eq!(requests, [WindowColorMode::WideGamutSdr]);
    }

    #[::core::prelude::v1::test]
    fn failed_sdr_request_is_not_reported_as_active_sdr() {
        let notice = negotiate_with(|_| anyhow::bail!("request rejected")).unwrap();
        assert!(notice.contains("Could not switch to standard SDR"));
        assert!(!notice.contains("Using standard SDR"));
    }
}
