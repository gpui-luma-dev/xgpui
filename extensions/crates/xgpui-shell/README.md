# Experimental desktop shell

Initial implementation for issue #149. This unpublished extension owns the shared
Linux frame used by the native labs. It renders GPUI elements; it does not load GTK,
libadwaita or third-party widget themes. No non-extension GPUI APIs are changed.

## Integration

Add a path dependency on this crate, then import `ShellApp` and `ShellWindowTitle`.
Replace `App::open_window(options, build)` with
`App::open_shell_window(options, ShellOptions::default(), build)`. The content
builder still returns the application's own view. The returned handle refers to
`ShellWindow`, whose `set_options` method updates that window's overrides.

Set the initial title through `WindowOptions.titlebar.title`, or call
`window.set_shell_title("Document title")` in the builder and whenever it changes.
The native title and client title stay synchronized. Titles are stored per window
and removed when its shell entity drops. The labs retain their existing
`open_lab_window` / `set_lab_title` adapters.

Linux and FreeBSD draw the frame only after the platform negotiates
`Decorations::Client`, and suppress it in fullscreen. Server decorations and the
existing Windows/macOS native chrome remain in charge elsewhere. Hidden capture
windows retain the previous lab behavior of omitting client chrome.

## Implemented preferences and behavior

- Live GPUI window appearance selects light/dark palette tokens; inactive windows
  use subdued text and header colors. Appearance, button-layout, activation and
  bounds observers invalidate the frame.
- Live platform button layout controls both order and omissions. Unsupported or
  application-disabled minimize/maximize controls are omitted. Fixed-size windows
  have no resize hit regions. Actions recheck capabilities when invoked.
- Equal-width side groups center the title independently of the number of buttons.
  A long title truncates before the controls; the title area remains draggable.
- Window buttons expose accessible button roles and labels, tooltips, Tab stops,
  Enter/Space activation and a visible keyboard focus ring. Screen-reader click
  actions dispatch directly to the corresponding window operation.
- Right-click on the title opens the platform window menu when supported.
  Double-click defaults to maximize/restore. Resize edges/corners respect tiling;
  maximized windows suppress resize edges and insets; tiled/maximized windows have
  square content corners; fullscreen clears client insets.
- All sizes are logical pixels. GPUI handles device/fractional scaling. The optional
  `text_scale` override scales title typography, header height and button targets
  once (finite values clamped to 1–3).

`ShellOptions` also allows application-supplied high contrast, an RGB accent for
focus indication, and `TitlebarAction::{ToggleMaximize, Minimize, None}`. These
values can be changed with `ShellWindow::set_options`. They are **not automatically
read from desktop preferences** in this first increment.

## Work remaining in #149

- Read/watch contrast, accent, text-scaling and title-bar action settings through
  supported Linux desktop interfaces, including sensible missing-portal fallbacks.
- Add optional app action/view-switcher slots and responsive placement, preserving
  a blank drag region and keeping essential actions available at narrow widths.
- Replace text control glyphs with consistent drawn symbolic icons; finish frame
  shadows/outer-corner treatment and contrast-aware focus styling.
- Extend automated state-transition and keyboard/accessibility acceptance coverage.
- Perform real GNOME and another desktop's acceptance: runtime preferences,
  minimize/maximize/fullscreen/tile transitions, drag/resize/menu, mixed DPI and
  system text scaling, screen-reader announcements, keyboard traversal, text-input
  focus and app shortcuts. Weston/Xvfb smoke is not equivalent to those checks.

This crate is intentionally absent from the published extension catalog and
release manifest until its application-facing interface has stabilized.

## Validation

From the repository root:

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-shell/Cargo.toml --lib
just lab-test
```

The existing Linux real-window smoke uses the labs' adapters and therefore tests
this shell. Linux CI includes the shell suite; the native platform matrix checks
that its implementation compiles on Linux, macOS and Windows.

### Initial local results (Fedora 44)

The lab regression suite, shell unit/lifecycle suite, strict shell Clippy and
workflow actionlint checks pass. The native two-window XWayland smoke passes.
On the current GNOME Wayland session both the pre-change compiled probe and the
new shell probe stop after their second rendered frame and time out. This is a
recorded live-session limitation, not successful GNOME acceptance; investigate
frame delivery separately before completing #149. Windows/macOS compile checks
have been added to CI but have not run for this local branch.
