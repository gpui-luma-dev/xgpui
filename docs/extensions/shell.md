# Desktop shell

> **ALPHA — expect API changes.** `xgpui-shell 0.2.0` is in preparation. Version 0.1.0 is published; the new color overrides require 0.2.0.

`xgpui-shell` supplies window chrome built from GPUI elements. Use its shared Linux client frame, or compose your own title bar while retaining native platform structure. It does not load GTK, libadwaita or third-party widget themes.

## Add to an application

After publication:

```toml
[dependencies]
xgpui = "3.1.0"
xgpui-shell = "0.2.0"
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

See the [Shell Lab walkthrough](shell-lab.md) and [Linux provider details](linux-shell-preferences.md).

## Limits and compatibility

This is the first published interface; APIs may change. Headless macOS, Windows and Linux CI passes, but it does not establish interactive GNOME/KDE acceptance, mixed-DPI behavior or screen-reader acceptance. Unsupported desktop actions remain unavailable; native macOS traffic-light artwork is not restyled.

Compared with the earlier local-only shell, scalar preference options are now optional overrides: use `Some(value)` for an explicit choice, or `..Default::default()` to follow the desktop. The new core APIs are additive, have default platform implementations and introduce no serialization format changes.

## GPUI public API changes: additions and compatibility

The shell uses additive core desktop-preference types, defaulted platform hooks and scoped App subscriptions. Existing platform implementers remain valid; no serialized representation changes. Its rendering and styling remain in this extension. The core 3.1.0 additions were approved and published in the previous release.

See [the core API ledger](gpui-public-api.md#desktop-preferences-for-shells) for the individual additions and why the backend seam belongs in core.

## App-owned shared chrome colors (0.2.0 candidate)

Set `titlebar_background` for a background-only override, or `colors` for the
shared frame palette. Values are opaque sRGB `0xRRGGBB`; `Some(0)` means black.

```rust
use xgpui_shell::{ShellColors, ShellOptions};

let background_only = ShellOptions {
    titlebar_background: Some(0xececed),
    ..Default::default()
};
let themed = ShellOptions {
    colors: Some(ShellColors {
        header: 0x202020,
        text: 0xffffff,
        border: 0x505050,
        hover: 0x404040,
    }),
    accent: Some(0x80bfff),
    ..Default::default()
};
```

Resolution starts with the appearance/activation/high-contrast palette. `colors`
replaces that palette, then `titlebar_background` replaces its header. Explicit
colors win even in high contrast: the app owns their contrast. The focus color
uses the separately resolved accent, or the palette text color in high contrast.
The palette covers shared frame borders, control foregrounds and hover treatment;
the header background includes reserved native-control space and trailing padding.
Application content keeps its own styling.

Resetting `titlebar_background` to `None` exposes `colors.header`, if supplied.
Resetting `colors` exposes the default palette, retaining any background override.
Resetting both restores desktop/default color resolution.

Supplied colors are literal values: the shell does not derive active/inactive or
light/dark variants. Update the returned shell handle on app theme changes and
window activation, retaining your observer subscriptions. For example, inside an
app view's setup callback:

```rust,ignore
let shell = shell_handle; // WindowHandle<ShellWindow> returned when opening
let activation = cx.observe_window_activation(window, move |_, window, cx| {
    let background = if window.is_window_active() { 0xececed } else { 0xf0f0f0 };
    // Use the same operation from your app's theme-change observer.
    cx.defer(move |cx| {
        let _ = shell.update(cx, |shell, _, cx| {
            shell.set_options(ShellOptions {
                titlebar_background: Some(background),
                ..Default::default()
            }, cx);
        });
    });
});
// Store `activation` in your view so it stays subscribed.
```

`set_options` replaces all overrides; pass all options you want to retain. It
redraws the existing content/titlebar entities without rebuilding them. This does
not recolor native macOS traffic lights, synchronize NSWindow appearance, change
native-control geometry, or theme compositor-owned decorations.

Compatibility: callers using exhaustive `ShellOptions` or
`ResolvedShellPreferences` literals must add `titlebar_background: None` and
`colors: None`. Options callers can instead use `..Default::default()`. There is
no serialization change and no new core GPUI API.
