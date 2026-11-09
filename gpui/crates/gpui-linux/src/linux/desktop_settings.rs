//! Optional native readers. Never invoke a shell, mutate settings, or assume that
//! installed GNOME schemas describe a non-GNOME session. Missing tools are normal.
use super::{Snapshot, bounded};
use gpui::{TitlebarAction, WindowAppearance};
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
enum Desktop {
    Gnome,
    Kde,
    Other,
}

fn desktop(current: &str) -> Desktop {
    // XDG_CURRENT_DESKTOP is a colon-separated list (e.g. ubuntu:GNOME).
    for name in current.split(':') {
        if name.eq_ignore_ascii_case("GNOME") || name.eq_ignore_ascii_case("Unity") {
            return Desktop::Gnome;
        }
        if name.eq_ignore_ascii_case("KDE") {
            return Desktop::Kde;
        }
    }
    Desktop::Other
}

pub(super) async fn read() -> Snapshot {
    match desktop(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()) {
        Desktop::Gnome => {
            let schemas = [
                "org.gnome.desktop.interface",
                "org.gnome.desktop.wm.preferences",
                "org.gnome.desktop.a11y.interface",
            ];
            let results = futures::future::join_all(
                schemas
                    .iter()
                    .map(|schema| command("gsettings", vec!["list-recursively", schema])),
            )
            .await;
            let mut values = HashMap::new();
            for text in results.into_iter().flatten() {
                for line in text.lines() {
                    let mut parts = line.splitn(3, ' ');
                    if let (Some(schema), Some(key), Some(value)) =
                        (parts.next(), parts.next(), parts.next())
                    {
                        values.insert(format!("{schema}/{key}"), value.trim().to_owned());
                    }
                }
            }
            gnome(&values)
        }
        Desktop::Kde => {
            // KConfig tools handle layered config, escaping, XDG paths and immutable
            // settings. Do not implement a partial INI parser in the toolkit.
            let keys = [
                ("kdeglobals", "General", "font"),
                ("kdeglobals", "KDE", "AnimationDurationFactor"),
                ("kwinrc", "Windows", "TitlebarDoubleClickCommand"),
                ("kwinrc", "MouseBindings", "CommandActiveTitlebar2"),
                ("kwinrc", "MouseBindings", "CommandActiveTitlebar3"),
            ];
            let results =
                futures::future::join_all(keys.iter().map(|(file, group, key)| async move {
                    let args = vec![
                        "--file",
                        file,
                        "--group",
                        group,
                        "--key",
                        key,
                        "--default",
                        "",
                    ];
                    // Plasma 5 and Plasma 6 installations may offer different tools.
                    let value = command("kreadconfig6", args.clone()).await;
                    match value {
                        Some(value) => Some(value),
                        None => command("kreadconfig5", args).await,
                    }
                }))
                .await;
            let values = keys
                .iter()
                .zip(results)
                .filter_map(|((_, _, key), value)| value.map(|v| (*key, v)))
                .collect();
            kde(&values)
        }
        Desktop::Other => Snapshot::default(),
    }
}

async fn command(program: &str, args: Vec<&str>) -> Option<String> {
    let output = bounded(
        smol::process::Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_owned())
}

// gsettings prints GVariant strings. Decode only the documented quoted string
// representation; unsupported escape sequences are unavailable, never guessed.
fn string(value: &str) -> Option<String> {
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"') || !value.ends_with(quote) || value.len() < 2 {
        return None;
    }
    let mut result = String::new();
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            result.push(match chars.next()? {
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                _ => return None,
            });
        } else if c == quote {
            return None;
        } else {
            result.push(c);
        }
    }
    (!result.trim().is_empty()).then_some(result)
}

fn gnome(values: &HashMap<String, String>) -> Snapshot {
    let get = |schema: &str, key: &str| {
        values
            .get(&format!("org.gnome.desktop.{schema}/{key}"))
            .map(String::as_str)
    };
    let mut s = Snapshot::default();
    s.preferences.font = get("interface", "font-name").and_then(string);
    s.preferences.text_scale = get("interface", "text-scaling-factor")
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0);
    s.preferences.reduced_motion = get("interface", "enable-animations")
        .and_then(|s| s.parse::<bool>().ok())
        .map(|v| !v);
    s.preferences.high_contrast =
        get("a11y.interface", "high-contrast").and_then(|s| s.parse().ok());
    let action = |key| super::action(get("wm.preferences", key).and_then(string));
    s.preferences.titlebar_double_click = action("action-double-click-titlebar");
    s.preferences.titlebar_middle_click = action("action-middle-click-titlebar");
    s.preferences.titlebar_right_click = action("action-right-click-titlebar");
    s.appearance = match get("interface", "color-scheme").and_then(string).as_deref() {
        Some("prefer-dark") => Some(WindowAppearance::Dark),
        Some("default" | "prefer-light") => Some(WindowAppearance::Light),
        _ => None,
    };
    s.button_layout = get("wm.preferences", "button-layout").and_then(string);
    s.cursor_theme = get("interface", "cursor-theme").and_then(string);
    s.cursor_size = get("interface", "cursor-size")
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|v| (1..=512).contains(v));
    s
}

pub(super) fn reduced_motion(value: &str) -> Option<bool> {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v == 0.0)
}

fn kde(values: &HashMap<&str, String>) -> Snapshot {
    let get = |key| values.get(key).map(String::as_str);
    let mut s = Snapshot::default();
    // Preserve QFont serialization as a description; it is not a family name.
    s.preferences.font = get("font")
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned);
    s.preferences.reduced_motion = get("AnimationDurationFactor").and_then(reduced_motion);
    let action = |value| match value {
        Some("Nothing") => Some(TitlebarAction::None),
        Some("Maximize") => Some(TitlebarAction::ToggleMaximize),
        Some("Minimize") => Some(TitlebarAction::Minimize),
        Some("Operations menu") => Some(TitlebarAction::Menu),
        _ => None,
    };
    s.preferences.titlebar_double_click = action(get("TitlebarDoubleClickCommand"));
    s.preferences.titlebar_middle_click = action(get("CommandActiveTitlebar2"));
    s.preferences.titlebar_right_click = action(get("CommandActiveTitlebar3"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_native_tool_is_an_optional_provider() {
        smol::block_on(async {
            assert_eq!(
                command("/nonexistent/xgpui-settings-tool", vec![]).await,
                None
            );
            assert_eq!(
                command("/bin/echo", vec!["literal $(command) argument"])
                    .await
                    .as_deref(),
                Some("literal $(command) argument")
            );
        });
    }

    #[test]
    fn session_detection_does_not_use_installed_schema_defaults_on_other_desktops() {
        assert_eq!(desktop("ubuntu:GNOME"), Desktop::Gnome);
        assert_eq!(desktop("KDE"), Desktop::Kde);
        for session in ["", "sway", "XFCE", "not-gnome"] {
            assert_eq!(desktop(session), Desktop::Other);
        }
    }

    #[test]
    fn gnome_normalizes_values_and_preserves_explicit_disabled_preferences() {
        let values = HashMap::from([
            (
                "org.gnome.desktop.interface/font-name".into(),
                "'User\\'s Sans 11'".into(),
            ),
            (
                "org.gnome.desktop.interface/text-scaling-factor".into(),
                "1.25".into(),
            ),
            (
                "org.gnome.desktop.interface/enable-animations".into(),
                "true".into(),
            ),
            (
                "org.gnome.desktop.a11y.interface/high-contrast".into(),
                "false".into(),
            ),
            (
                "org.gnome.desktop.wm.preferences/action-double-click-titlebar".into(),
                "'none'".into(),
            ),
        ]);
        let p = gnome(&values).preferences;
        assert_eq!(p.font.as_deref(), Some("User's Sans 11"));
        assert_eq!(p.text_scale, Some(1.25));
        assert_eq!(p.reduced_motion, Some(false));
        assert_eq!(p.high_contrast, Some(false));
        assert_eq!(p.titlebar_double_click, Some(TitlebarAction::None));
        assert_eq!(gnome(&HashMap::new()), Snapshot::default());
        for invalid in ["unquoted", "'broken", "'bad\\q'", "''"] {
            assert_eq!(string(invalid), None);
        }
    }

    #[test]
    fn kde_preserves_font_description_and_only_maps_portable_actions() {
        let values = HashMap::from([
            ("font", "Noto Sans,10,-1,5,50,0,0,0,0,0".into()),
            ("AnimationDurationFactor", "0".into()),
            ("TitlebarDoubleClickCommand", "Maximize".into()),
            ("CommandActiveTitlebar2", "Shade".into()),
            ("CommandActiveTitlebar3", "Operations menu".into()),
        ]);
        let p = kde(&values).preferences;
        assert_eq!(p.font.as_deref(), values.get("font").map(String::as_str));
        assert_eq!(p.reduced_motion, Some(true));
        assert_eq!(
            p.titlebar_double_click,
            Some(TitlebarAction::ToggleMaximize)
        );
        assert_eq!(p.titlebar_middle_click, None);
        assert_eq!(p.titlebar_right_click, Some(TitlebarAction::Menu));
        for value in ["NaN", "inf", "-1", "nonsense"] {
            assert_eq!(reduced_motion(value), None);
        }
        assert_eq!(reduced_motion("1"), Some(false));
    }
}
