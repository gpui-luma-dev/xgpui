# Experimental desktop shell

> **ALPHA — expect API changes.** `xgpui-shell 0.1.0` is being prepared for its first crates.io release. The code is available locally; publication is still pending.

`xgpui-shell` supplies window chrome built from GPUI elements. Use its shared Linux client frame, or compose your own title bar while retaining native platform structure. It does not load GTK, libadwaita or third-party widget themes.

## Add to an application

After publication:

```toml
[dependencies]
xgpui = "3.1.0"
xgpui-shell = "0.1.0"
```

While reviewing this release, use local path dependencies instead.

```rust
use xgpui_shell::{ShellApp, ShellOptions, ShellWindowTitle};

// Inside an App callback; MyView is your application's Render implementation.
cx.open_shell_window(options, ShellOptions::default(), |window, cx| {
    window.set_shell_title("My application");
    cx.new(|_| MyView::new())
})?;
```

The returned handle refers to `ShellWindow`. Its `set_options` method updates that window's overrides. Set the initial title through `WindowOptions.titlebar.title`, or keep it synchronized with `set_shell_title`.

## Compose a title bar

Use `ShellApp::open_shell_window_with_titlebar` with a builder returning `(Entity<Titlebar>, Entity<Content>)`. Place title text or empty space inside `ShellDragRegion`; keep interactive buttons and inputs outside that drag region.

macOS retains native traffic lights. Windows uses app-composed content and shell controls. Linux/FreeBSD draw the client frame only when the platform reports client decorations; server decorations remain compositor-owned. Fullscreen suppresses the client frame. A decoration request is not proof that the compositor accepted it.

## Follow desktop preferences

`ShellOptions::default()` follows available desktop values. Explicit optional fields override them per window, including `Some(false)`, a black accent and `Some(TitlebarAction::None)`. Resetting an override restores desktop/default resolution.

The Linux backend reads accent, contrast, reduced motion, logical text scale, font descriptions and supported title-bar actions. Settings portal values take precedence over optional GNOME/KDE native fallbacks. Missing tools or settings leave fields unset. Existing appearance and button-layout APIs remain separate.

Desktop font descriptions style the shell title; logical text scale is clamped to 0.5–4 and stays separate from display DPI. Reduced motion is exposed as policy; the static frame has no animations. Application content retains its own styling.

Core `App::desktop_preferences()` reads the snapshot, and `App::observe_desktop_preferences(...)` subscribes to changes. Keep the returned subscription alive. Other platforms currently return an empty optional snapshot.

## Try the lab

```sh
just profile=dev lab-shell
# Linux: request a client frame
just profile=dev lab-shell --client-decorations
# Check capabilities on a fixed-size window
just profile=dev lab-shell --client-decorations --fixed-size
```

The lab shows detected and resolved preferences, per-window overrides and window state. Its client-frame preview is a simulation: preview controls never move, resize or close the host window.

See the [Shell Lab walkthrough](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/shell-lab.md) and [Linux provider details](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/linux-shell-preferences.md).

## Limits and compatibility

This is the first published interface; APIs may change. Headless macOS, Windows and Linux CI passes, but it does not establish interactive GNOME/KDE acceptance, mixed-DPI behavior or screen-reader acceptance. Unsupported desktop actions remain unavailable; native macOS traffic-light artwork is not restyled.

Compared with the earlier local-only shell, scalar preference options are now optional overrides: use `Some(value)` for an explicit choice, or `..Default::default()` to follow the desktop. The new core APIs are additive, have default platform implementations and introduce no serialization format changes.

## GPUI public API changes: additions and compatibility

The shell uses additive core desktop-preference types, defaulted platform hooks and scoped App subscriptions. Existing platform implementers remain valid; no serialized representation changes. Its rendering and styling remain in this extension. Final API release approval is pending.

See [the core API ledger](https://xgpui.dev/gpui-public-api.html#desktop-preferences-for-shells) for the individual additions and why the backend seam belongs in core.
