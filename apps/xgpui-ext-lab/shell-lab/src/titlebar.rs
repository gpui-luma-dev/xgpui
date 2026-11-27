//! App-owned composition: SVG artwork and layout are supplied by this app.
use super::*;
use std::borrow::Cow;
use xgpui_shell::ShellDragRegion;

pub(super) struct HeaderAssets;
const ICONS: [(&str, &str); 8] = [
    // Window-control artwork for the presenter demo (lab-drawn, no third-party assets).
    (
        "shell/close.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4z"/></svg>"#,
    ),
    (
        "shell/minimize.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 11h14v2H5z"/></svg>"#,
    ),
    (
        "shell/maximize.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill-rule="evenodd" d="M5 5h14v14H5V5zm2 2v10h10V7H7z"/></svg>"#,
    ),
    (
        "shell/restore.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill-rule="evenodd" d="M8 4h12v12h-4v4H4V8h4V4zm2 2v2h6v8h2V6h-8zM6 10v8h8v-8H6z"/></svg>"#,
    ),
    (
        "shell/reset.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 4v6h6V8H7a6 6 0 1 1-1 7H4a8 8 0 1 0 2-9V4z"/></svg>"#,
    ),
    (
        "shell/panel.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill-rule="evenodd" d="M3 4h18v16H3V4zm2 2v12h4V6H5zm6 0v12h8V6h-8z"/></svg>"#,
    ),
    (
        "shell/theme.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill-rule="evenodd" d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zm0 2v16a8 8 0 1 1 0-16z"/></svg>"#,
    ),
    (
        "shell/plus.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M11 3h2v8h8v2h-8v8h-2v-8H3v-2h8z"/></svg>"#,
    ),
];
impl AssetSource for HeaderAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, svg)| Cow::Borrowed(svg.as_bytes())))
    }
    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| (*name).into())
            .collect())
    }
}

pub(super) struct LabTitleBar {
    lab: Entity<Lab>,
    _subscription: Subscription,
}
impl LabTitleBar {
    pub(super) fn new(lab: Entity<Lab>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&lab, |_, _, cx| cx.notify());
        Self {
            lab,
            _subscription: subscription,
        }
    }
    fn button(
        &self,
        id: &'static str,
        label: &'static str,
        path: &'static str,
        action: Action,
        color: u32,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let lab = self.lab.clone();
        let key_lab = lab.clone();
        let a11y_lab = lab.clone();
        div()
            .id(id)
            .debug_selector(move || id.into())
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_stop(true)
            .size(px(28.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(transparent_black())
            .cursor_pointer()
            .hover(|s| s.bg(rgba(0x80808050)))
            .focus_visible(|s| s.border_color(rgb(0x3584e4)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |event, window, app| {
                lab.update(app, |lab, cx| {
                    lab.dispatch(action, event.position(), window, cx)
                })
            })
            .on_key_down(move |event, window, app| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space")
                    && !event.is_held
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform
                {
                    app.stop_propagation();
                    key_lab.update(app, |lab, cx| {
                        lab.dispatch(action, Point::default(), window, cx)
                    });
                }
            })
            .on_a11y_action(AccessibleAction::Click, move |_, window, app| {
                a11y_lab.update(app, |lab, cx| {
                    lab.dispatch(action, Point::default(), window, cx)
                });
            })
            // An SVG paints only with an explicit color.
            .child(svg().path(path).size(px(18.)).text_color(rgb(color)))
            .into_any_element()
    }
}
impl Render for LabTitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lab = self.lab.read(cx);
        let options = lab.overrides.shell_options();
        let look = options
            .resolved_preferences(&cx.desktop_preferences())
            .resolved_look(window.appearance(), window.is_window_active());
        let next = if lab.preview_appearance == Some(WindowAppearance::Dark) {
            WindowAppearance::Light
        } else {
            WindowAppearance::Dark
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            // This app chooses its header layout and icons. Shell only wraps it.
            .child(self.button(
                "header-reset",
                "Reset this window's overrides",
                "shell/reset.svg",
                Action::Reset,
                look.colors.text,
                cx,
            ))
            .child(self.button(
                "header-panel",
                "Toggle platform readings",
                "shell/panel.svg",
                Action::TogglePlatformReadings,
                look.colors.text,
                cx,
            ))
            .child(div().w(px(1.)).h(px(20.)).bg(rgb(look.colors.border)))
            .child(
                ShellDragRegion::new(
                    "app-header-drag",
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .px_2()
                        .overflow_hidden()
                        .child(div().truncate().child(window.shell_title())),
                )
                .options(options),
            )
            .child(self.button(
                "header-theme",
                "Toggle preview appearance",
                "shell/theme.svg",
                Action::PreviewAppearance(Some(next)),
                look.colors.text,
                cx,
            ))
            .child(self.button(
                "header-new",
                "Open an independent window",
                "shell/plus.svg",
                Action::NewWindow { fixed: false },
                look.colors.text,
                cx,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn app_owned_svg_assets_load_and_rasterize() {
        let renderer = SvgRenderer::new(std::sync::Arc::new(HeaderAssets));
        for (name, _) in ICONS {
            let bytes = HeaderAssets
                .load(name)
                .unwrap()
                .expect("registered app icon");
            renderer
                .render_single_frame(&bytes, 1.0)
                .expect("valid visible SVG");
        }
        assert!(HeaderAssets.load("unknown.svg").unwrap().is_none());
    }
}
