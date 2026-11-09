//! Provides a [calloop] event source from [XDG Desktop Portal] events
//!
//! This module uses the [ashpd] crate

use ashpd::desktop::settings::{Namespace, Settings};
use calloop::channel::{Channel, Sender};
use calloop::{EventSource, Poll, PostAction, Readiness, Token, TokenFactory};
use futures::future::{Either, select};
use futures::{StreamExt, pin_mut};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[path = "desktop_settings.rs"]
mod desktop_settings;

use gpui::{BackgroundExecutor, Task, WindowAppearance};

pub enum Event {
    WindowAppearance(WindowAppearance),
    #[cfg_attr(feature = "x11", allow(dead_code))]
    CursorTheme(String),
    #[cfg_attr(feature = "x11", allow(dead_code))]
    CursorSize(u32),
    ButtonLayout(String),
    DesktopPreferences(gpui::DesktopPreferences),
}

/// Forwards desktop settings from the portal to the event loop.
///
/// Owns the task that listens to the portal, so removing the source from the loop cancels it and
/// releases its D-Bus subscriptions.
pub struct XDPEventSource {
    channel: Channel<Event>,
    _listener: Task<()>,
}

impl XDPEventSource {
    pub fn new(executor: &BackgroundExecutor) -> Self {
        let (sender, channel) = calloop::channel::channel();
        let listener = executor.spawn(async move {
            if let Err(error) = listen(sender).await {
                log::debug!("stopped listening to desktop portal settings: {error:#}");
            }
        });
        Self {
            channel,
            _listener: listener,
        }
    }
}

const NAMESPACES: [&str; 5] = [
    "org.freedesktop.appearance",
    "org.gnome.desktop.interface",
    "org.gnome.desktop.wm.preferences",
    "org.kde.kdeglobals.General",
    "org.kde.kdeglobals.KDE",
];

// Missing or disconnected portal services must not permanently disable live settings.
async fn listen(sender: Sender<Event>) -> anyhow::Result<()> {
    let mut previous = None;
    loop {
        if let Err(error) = listen_session(&sender, &mut previous).await {
            log::debug!("desktop settings unavailable; retrying: {error:#}");
            let snapshot = desktop_settings::read()
                .await
                .with_last_known_window_settings(previous.as_ref());
            snapshot.emit_changes(previous.as_ref(), &sender)?;
            previous = Some(snapshot);
        }
        smol::Timer::after(Duration::from_secs(5)).await;
    }
}

async fn listen_session(
    sender: &Sender<Event>,
    previous: &mut Option<Snapshot>,
) -> anyhow::Result<()> {
    let settings = bounded(Settings::new()).await??;
    // Subscribe before reading so an intervening preference change is not lost.
    let changes = bounded(settings.receive_setting_changed()).await??;
    pin_mut!(changes);
    let mut fallback = desktop_settings::read().await;
    let mut refreshed = Instant::now();
    loop {
        if refreshed.elapsed() >= Duration::from_secs(15) {
            fallback = desktop_settings::read().await;
            refreshed = Instant::now();
        }
        let values = bounded(settings.read_all(&NAMESPACES)).await??;
        let snapshot = Snapshot::from_values(&values).with_fallback(&fallback);
        snapshot.emit_changes(previous.as_ref(), sender)?;
        *previous = Some(snapshot);
        // Periodic refresh detects portal owner restarts and removed settings even when
        // no SettingChanged signal is delivered. Unrelated namespaces do not redraw.
        let refresh = smol::Timer::after(Duration::from_secs(15));
        pin_mut!(refresh);
        loop {
            match select(changes.next(), &mut refresh).await {
                Either::Left((Some(setting), _)) => {
                    if NAMESPACES.contains(&setting.namespace()) {
                        break;
                    }
                }
                Either::Left((None, _)) => anyhow::bail!("desktop settings stream closed"),
                Either::Right(_) => break,
            }
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "portal I/O needs a real timeout independent of the UI scheduler"
)]
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> anyhow::Result<T> {
    pin_mut!(future);
    match select(future, Box::pin(smol::Timer::after(Duration::from_secs(5)))).await {
        Either::Left((value, _)) => Ok(value),
        Either::Right(_) => anyhow::bail!("desktop settings request timed out"),
    }
}

#[derive(Debug, Default, PartialEq)]
struct Snapshot {
    preferences: gpui::DesktopPreferences,
    appearance: Option<WindowAppearance>,
    cursor_theme: Option<String>,
    cursor_size: Option<u32>,
    button_layout: Option<String>,
}

impl Snapshot {
    fn from_values(values: &HashMap<String, Namespace>) -> Self {
        let appearance = "org.freedesktop.appearance";
        let interface = "org.gnome.desktop.interface";
        let wm = "org.gnome.desktop.wm.preferences";
        let accent =
            value::<(f64, f64, f64)>(values, appearance, "accent-color").and_then(|(r, g, b)| {
                [r, g, b]
                    .iter()
                    .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
                    .then_some([r as f32, g as f32, b as f32])
            });
        let high_contrast = preference(value(values, appearance, "contrast"));
        let reduced_motion = preference(value(values, appearance, "reduced-motion"))
            .or_else(|| value::<bool>(values, interface, "enable-animations").map(|v| !v))
            .or_else(|| {
                value::<String>(values, "org.kde.kdeglobals.KDE", "AnimationDurationFactor")
                    .and_then(|s| desktop_settings::reduced_motion(&s))
            });
        let text_scale = value::<f64>(values, interface, "text-scaling-factor")
            .filter(|v| v.is_finite() && *v > 0.0 && *v <= f32::MAX as f64)
            .map(|v| v as f32)
            .filter(|v| *v > 0.0);
        Self {
            preferences: gpui::DesktopPreferences {
                accent_color: accent,
                high_contrast,
                reduced_motion,
                text_scale,
                font: value::<String>(values, interface, "font-name")
                    .or_else(|| value(values, "org.kde.kdeglobals.General", "font"))
                    .filter(|s| !s.trim().is_empty()),
                titlebar_double_click: action(value(values, wm, "action-double-click-titlebar")),
                titlebar_middle_click: action(value(values, wm, "action-middle-click-titlebar")),
                titlebar_right_click: action(value(values, wm, "action-right-click-titlebar")),
            },
            appearance: match value::<u32>(values, appearance, "color-scheme") {
                Some(1) => Some(WindowAppearance::Dark),
                Some(0 | 2) => Some(WindowAppearance::Light),
                _ => None,
            },
            cursor_theme: value(values, interface, "cursor-theme"),
            cursor_size: value::<i32>(values, interface, "cursor-size")
                .filter(|size| (1..=512).contains(size))
                .map(|size| size as u32),
            button_layout: value(values, wm, "button-layout"),
        }
    }

    // During a portal outage preserve legacy appearance/layout/cursor state if
    // the native reader cannot supply it. Optional extra preferences remain honest
    // about current availability instead of retaining stale accessibility values.
    fn with_last_known_window_settings(mut self, previous: Option<&Self>) -> Self {
        if let Some(old) = previous {
            self.appearance = self.appearance.or(old.appearance);
            self.button_layout = self.button_layout.or_else(|| old.button_layout.clone());
            self.cursor_theme = self.cursor_theme.or_else(|| old.cursor_theme.clone());
            self.cursor_size = self.cursor_size.or(old.cursor_size);
        }
        self
    }

    // Fill each absent value independently. Explicit false, black, and no-action win.
    fn with_fallback(mut self, fallback: &Self) -> Self {
        let p = &mut self.preferences;
        let f = &fallback.preferences;
        p.accent_color = p.accent_color.or(f.accent_color);
        p.high_contrast = p.high_contrast.or(f.high_contrast);
        p.reduced_motion = p.reduced_motion.or(f.reduced_motion);
        p.text_scale = p.text_scale.or(f.text_scale);
        p.font = p.font.take().or_else(|| f.font.clone());
        p.titlebar_double_click = p.titlebar_double_click.or(f.titlebar_double_click);
        p.titlebar_middle_click = p.titlebar_middle_click.or(f.titlebar_middle_click);
        p.titlebar_right_click = p.titlebar_right_click.or(f.titlebar_right_click);
        self.appearance = self.appearance.or(fallback.appearance);
        self.cursor_theme = self.cursor_theme.or_else(|| fallback.cursor_theme.clone());
        self.cursor_size = self.cursor_size.or(fallback.cursor_size);
        self.button_layout = self
            .button_layout
            .or_else(|| fallback.button_layout.clone());
        self
    }

    fn emit_changes(&self, previous: Option<&Self>, sender: &Sender<Event>) -> anyhow::Result<()> {
        if previous.is_none_or(|old| old.preferences != self.preferences) {
            sender.send(Event::DesktopPreferences(self.preferences.clone()))?;
        }
        if previous.is_none_or(|old| old.appearance != self.appearance) {
            sender.send(Event::WindowAppearance(
                self.appearance.unwrap_or(WindowAppearance::Light),
            ))?;
        }
        if previous.is_none_or(|old| old.button_layout != self.button_layout) {
            sender.send(Event::ButtonLayout(
                self.button_layout
                    .clone()
                    .unwrap_or_else(|| ":minimize,maximize,close".into()),
            ))?;
        }
        if previous.is_none_or(|old| old.cursor_theme != self.cursor_theme)
            && let Some(theme) = &self.cursor_theme
        {
            sender.send(Event::CursorTheme(theme.clone()))?;
        }
        if previous.is_none_or(|old| old.cursor_size != self.cursor_size)
            && let Some(size) = self.cursor_size
        {
            sender.send(Event::CursorSize(size))?;
        }
        Ok(())
    }
}

fn value<T: TryFrom<ashpd::zbus::zvariant::OwnedValue>>(
    values: &HashMap<String, Namespace>,
    namespace: &str,
    key: &str,
) -> Option<T> {
    values
        .get(namespace)?
        .get(key)?
        .try_clone()
        .ok()?
        .try_into()
        .ok()
}

fn preference(value: Option<u32>) -> Option<bool> {
    match value {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}

fn action(value: Option<String>) -> Option<gpui::TitlebarAction> {
    use gpui::TitlebarAction;
    match value.as_deref()? {
        "none" => Some(TitlebarAction::None),
        "toggle-maximize" => Some(TitlebarAction::ToggleMaximize),
        "minimize" => Some(TitlebarAction::Minimize),
        "menu" => Some(TitlebarAction::Menu),
        // Other desktop actions have no portable GPUI operation yet.
        _ => None,
    }
}

impl EventSource for XDPEventSource {
    type Event = Event;
    type Metadata = ();
    type Ret = ();
    type Error = anyhow::Error;

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        token: Token,
        mut callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        self.channel.process_events(readiness, token, |evt, _| {
            if let calloop::channel::Event::Msg(msg) = evt {
                (callback)(msg, &mut ())
            }
        })?;

        Ok(PostAction::Continue)
    }

    fn register(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        self.channel.register(poll, token_factory)?;

        Ok(())
    }

    fn reregister(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        self.channel.reregister(poll, token_factory)?;

        Ok(())
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        self.channel.unregister(poll)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ashpd::zbus::zvariant::{OwnedValue, Value};

    fn owned(value: impl Into<Value<'static>>) -> OwnedValue {
        value.into().try_into().unwrap()
    }

    #[test]
    fn portal_values_win_per_field_and_removal_reveals_fallback() {
        let fallback = Snapshot {
            preferences: gpui::DesktopPreferences {
                accent_color: Some([1.0, 0.0, 0.0]),
                high_contrast: Some(true),
                reduced_motion: Some(true),
                font: Some("Desktop font".into()),
                titlebar_double_click: Some(gpui::TitlebarAction::ToggleMaximize),
                ..Default::default()
            },
            appearance: Some(WindowAppearance::Dark),
            ..Default::default()
        };
        let portal = Snapshot {
            preferences: gpui::DesktopPreferences {
                accent_color: Some([0.0; 3]),
                high_contrast: Some(false),
                titlebar_double_click: Some(gpui::TitlebarAction::None),
                ..Default::default()
            },
            appearance: Some(WindowAppearance::Light),
            ..Default::default()
        }
        .with_fallback(&fallback);
        assert_eq!(portal.preferences.accent_color, Some([0.0; 3]));
        assert_eq!(portal.preferences.high_contrast, Some(false));
        assert_eq!(
            portal.preferences.titlebar_double_click,
            Some(gpui::TitlebarAction::None)
        );
        assert_eq!(portal.preferences.font.as_deref(), Some("Desktop font"));
        assert_eq!(portal.appearance, Some(WindowAppearance::Light));
        let removed = Snapshot::default().with_fallback(&fallback);
        assert_eq!(removed, fallback);
        let unavailable = Snapshot::default().with_fallback(&Snapshot::default());
        assert_eq!(unavailable.preferences, gpui::DesktopPreferences::default());
        let (sender, receiver) = calloop::channel::channel();
        removed.emit_changes(Some(&portal), &sender).unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            Event::DesktopPreferences(_)
        ));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            Event::WindowAppearance(WindowAppearance::Dark)
        ));
        removed.emit_changes(Some(&removed), &sender).unwrap();
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn outage_retains_legacy_window_settings_but_not_stale_optional_preferences() {
        let old = Snapshot {
            appearance: Some(WindowAppearance::Dark),
            button_layout: Some("close:".into()),
            preferences: gpui::DesktopPreferences {
                high_contrast: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let unavailable = Snapshot::default().with_last_known_window_settings(Some(&old));
        assert_eq!(unavailable.appearance, old.appearance);
        assert_eq!(unavailable.button_layout, old.button_layout);
        assert_eq!(unavailable.preferences.high_contrast, None);
        let local = Snapshot {
            appearance: Some(WindowAppearance::Light),
            ..Default::default()
        }
        .with_last_known_window_settings(Some(&old));
        assert_eq!(local.appearance, Some(WindowAppearance::Light));
    }

    #[test]
    fn kde_portal_extensions_are_strings_and_standard_values_win() {
        let values = HashMap::from([
            (
                NAMESPACES[0].into(),
                HashMap::from([("reduced-motion".into(), owned(0_u32))]),
            ),
            (
                "org.kde.kdeglobals.General".into(),
                HashMap::from([("font".into(), owned("Noto Sans,10,-1,5,50,0,0,0,0,0"))]),
            ),
            (
                "org.kde.kdeglobals.KDE".into(),
                HashMap::from([("AnimationDurationFactor".into(), owned("0"))]),
            ),
        ]);
        let p = Snapshot::from_values(&values).preferences;
        assert_eq!(p.reduced_motion, Some(false));
        assert!(p.font.unwrap().starts_with("Noto Sans,"));
    }

    #[test]
    fn snapshots_keep_explicit_false_and_none_distinct() {
        let values = HashMap::from([
            (
                NAMESPACES[0].into(),
                HashMap::from([
                    ("accent-color".into(), owned((0.25_f64, 0.5_f64, 1.0_f64))),
                    ("contrast".into(), owned(0_u32)),
                    ("reduced-motion".into(), owned(1_u32)),
                ]),
            ),
            (
                NAMESPACES[1].into(),
                HashMap::from([
                    ("text-scaling-factor".into(), owned(1.5_f64)),
                    ("font-name".into(), owned("Cantarell 11")),
                ]),
            ),
            (
                NAMESPACES[2].into(),
                HashMap::from([("action-double-click-titlebar".into(), owned("none"))]),
            ),
        ]);
        let p = Snapshot::from_values(&values).preferences;
        assert_eq!(p.accent_color, Some([0.25, 0.5, 1.0]));
        assert_eq!(p.high_contrast, Some(false));
        assert_eq!(p.reduced_motion, Some(true));
        assert_eq!(p.text_scale, Some(1.5));
        assert_eq!(p.font.as_deref(), Some("Cantarell 11"));
        assert_eq!(p.titlebar_double_click, Some(gpui::TitlebarAction::None));
        assert_eq!(p.titlebar_middle_click, None);
        assert_eq!(
            Snapshot::from_values(&HashMap::new()).preferences,
            gpui::DesktopPreferences::default()
        );
    }

    #[test]
    fn invalid_types_ranges_and_unsupported_actions_fall_back_safely() {
        for accent in [(f64::NAN, 0.5, 0.5), (0.5, -0.1, 0.5), (0.5, 0.5, 1.1)] {
            let values = HashMap::from([(
                NAMESPACES[0].into(),
                HashMap::from([
                    ("accent-color".into(), owned(accent)),
                    ("contrast".into(), owned(9_u32)),
                ]),
            )]);
            let p = Snapshot::from_values(&values).preferences;
            assert_eq!(p.accent_color, None);
            assert_eq!(p.high_contrast, None);
        }
        for scale in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            let values = HashMap::from([(
                NAMESPACES[1].into(),
                HashMap::from([
                    ("text-scaling-factor".into(), owned(scale)),
                    ("cursor-size".into(), owned(-1_i32)),
                    ("font-name".into(), owned(42_u32)),
                ]),
            )]);
            let s = Snapshot::from_values(&values);
            assert_eq!(s.preferences.text_scale, None);
            assert_eq!(s.preferences.font, None);
            assert_eq!(s.cursor_size, None);
        }
        assert_eq!(action(Some("unsupported".into())), None);
    }

    #[test]
    fn unchanged_snapshots_do_not_notify_and_removal_clears_preferences() {
        let (sender, receiver) = calloop::channel::channel();
        let mut old = Snapshot::default();
        old.preferences.high_contrast = Some(true);
        old.emit_changes(Some(&old), &sender).unwrap();
        assert!(receiver.try_recv().is_err());
        Snapshot::default()
            .emit_changes(Some(&old), &sender)
            .unwrap();
        let Event::DesktopPreferences(p) = receiver.try_recv().unwrap() else {
            panic!("expected cleared desktop preferences");
        };
        assert_eq!(p.high_contrast, None);
        assert!(receiver.try_recv().is_err());
    }
}
