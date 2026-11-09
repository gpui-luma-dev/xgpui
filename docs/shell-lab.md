# Shell Lab

Interactive diagnostics for xgpui issue 149. The lab reads the platform's current
preferences and uses the same `ShellOptions::resolved_preferences` resolver as the
client frame. It never writes desktop settings.

## Run

From the xgpui repository on `issue-149/shell-look`:

```sh
just profile=dev lab-shell

# Ask for the xgpui client frame on Linux (the compositor can decline).
just profile=dev lab-shell --client-decorations

# Exercise a non-resizable window.
just profile=dev lab-shell --client-decorations --fixed-size
```

This branch was created from `issue-149/epic` and carries the prerequisites from
draft PRs #182, #183 and #184 as cherry-picks, without merging those PRs. Reconcile
those prerequisite commits when integrating the epic. Do not target main.

The shell is still experimental. A local path dependency is used; no published
shell crate is required. The first build uses the existing native extension-lab
workspace and its dependencies.

## What to inspect

- **Detected:** actual values exposed by the platform snapshot. Missing values
  say `not reported`; this can mean an absent setting/provider, an unsupported
  platform, or an initial snapshot still pending. The API does not currently
  distinguish those cases, so the lab does not claim a specific reason.
- **Resolved client frame:** actual resolver output after this window's overrides,
  desktop preferences and built-in fallbacks. Native/server decorations retain
  control of their own appearance; resolved values do not prove the compositor
  is displaying xgpui's frame.
- **System / explicit override:** selected buttons show each window's current
  override. Explicit Normal, Black and None are preserved; Reset clears every
  override for this window only.
- **Typography / reduced motion:** common desktop font descriptions now style the
  client-frame title. Per-window typography and motion-policy overrides are available.
  Motion is a resolved policy only: this static frame has no animations.
- **Window state:** requested and platform-reported decoration modes, appearance,
  button layout, activation, maximization/fullscreen, capabilities, bounds,
  viewport and device scale. Decoration requests may be declined or arrive
  asynchronously; inspect the reported result.
- **Recent events:** the last twelve distinct notifications/actions, newest first.
  Refresh View only rereads displayed state; it does not force a portal request.

All interactive buttons support pointer activation, Tab focus, Enter/Space and
accessible click actions. Unsupported operations are dimmed and do not dispatch.

## Client-frame preview on Mac (or any host)

The left pane contains a clearly labeled **simulation**, using the same renderer
as the production client frame. The right pane scrolls independently, keeping the
preview visible while changing per-window overrides.

- Change contrast, accent, typography and logical scale in the right pane to inspect their
  rendered result. Show Focus Ring simulates focus on the decorative close icon
  so accent changes remain visible without making preview controls actionable.
  Show Hover independently simulates the close-button hover background.
- Select Floating, Maximized, Tiled or Fullscreen, Light/Dark/native appearance,
  Active/Inactive, and alternate button layouts in the preview controls.
- Toggle the long title in Window Operations to inspect truncation in the preview.
- Preview buttons are decorative images; clicks, double-clicks, drag regions and
  resize borders do not close, move, resize or change the host window. Use the
  separate native Window Operations controls to operate the real window.
- Simulated choices are never stored in the detected platform snapshot. They do
  not claim to validate Linux compositor behavior or real desktop integration.

Window Menu and decoration requests are disabled on macOS/Windows, where the
current backend operations are no-ops. Dispatch guards enforce the same rule as
the buttons, even if an unsupported action is invoked programmatically. The
Linux-only `--client-decorations` startup option is ignored on these platforms.

## Manual Linux check

1. Start the client-decorated lab on Fedora GNOME or Ubuntu GNOME. Confirm the
   reported decoration mode before judging frame styling.
2. Change appearance, accent, contrast, text scale, button placement and title-bar
   actions through the desktop settings UI. Watch detected values and events.
   Portal values take precedence; optional GNOME/KDE native fallbacks fill missing
   preferences. See [provider coverage](linux-shell-preferences.md).
3. Override contrast, accent, typography, scale and click actions in this window. Check that
   the client frame responds and desktop settings remain unchanged. Click the
   real title bar to exercise its selected action.
4. Open a second window. Verify independent defaults and titles; reset one window
   while leaving the other's overrides intact.
5. Try maximize/restore, fullscreen/exit, window menu, compact/normal size, the long
   title, and drag/resize. Minimize is restored through the desktop. Use desktop
   snap/tiling gestures to test tiling; the lab does not invent a portable tiling API.
6. Open a fixed-size window and verify resize/maximize controls are unavailable.
7. Move windows between displays and inspect device scale separately from logical
   text scale. Repeat with server decorations and another desktop/compositor.

On Windows/macOS, native window chrome remains in place. The inspector and state
controls are available, but resolved client-frame overrides only affect the
visual preview, not native window buttons. No desktop-specific behavior is selected by the displayed distro
or desktop name.

## Build and headless tests

```sh
cargo build --locked --config .cargo/validation.toml \
  --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shell-lab
cargo test --locked --config .cargo/validation.toml \
  --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shell-lab
cargo test --locked --config .cargo/validation.toml -p xgpui-shell --lib
```

The lab's headless tests verify per-window override/reset isolation and rejection
of unavailable fixed-window operations. Shared shell tests cover preference
resolution. New headless tests verify Mac operation guards, simulation-state
isolation and that clicks/double-clicks on rendered preview controls preserve the
host window, its bounds, decorations and insets. These do not establish real GNOME/KDE input, rendering, accessibility,
portal service behavior or resolution of the recorded Wayland frame stall.

The lab adds no core platform public API beyond the separate #182 prerequisite.
The shell extension exposes its existing resolver and result fields for accurate
diagnostics instead of duplicating resolution logic in the app.

## How the look is applied

The new left-pane inspector uses `ResolvedShellPreferences::resolved_look`, the
same resolver consumed by the production client frame and its safe preview.
It displays the exact header, text, border, hover and effective focus colors;
requested title font family/weight/style, scaled logical size, header height and
button target size; and the resolved click actions and reduced-motion policy.

1. Choose typography and logical scale in the right pane. The title changes;
   the sample body and lab controls keep their independent styles.
2. Select Light/Dark and Active/Inactive. Compare the displayed color swatches
   with the header and title. Contrast selects the accessibility palette.
3. Choose an accent and toggle Show Focus Ring. In high contrast, focus uses the
   foreground color instead of the accent. Toggle Show Hover to inspect its
   background independently; both can be visible together.
4. Toggle reduced-motion policy. The inspector shows the resolved policy without
   pretending there is an animation to disable. Current hover/focus changes are instant.
5. Reset this window. Desktop values or built-in fallbacks return. Open another
   window to check independent settings. Actual pointer/keyboard behavior on a
   native Linux client frame remains part of final platform validation.

Fonts are **requested**, not reported as actual matched faces. GPUI may substitute
an installed family or fall back for particular glyphs. The close icon is a vector;
other control symbols use stable system typography independently of the title.
Native/server decorations retain OS-controlled typography and colors.

### Typography contract and limits

`ShellTypography { font: gpui::Font, size: f32 }` is an extension API for explicit
client-frame typography. `ShellOptions::typography` overrides the desktop font;
`None` parses a desktop description or falls back to system UI at 14 logical px.
Size is bounded to 8–48 before text scale (0.5–4), and the header grows to contain
larger titles. Points convert at 96/72 logical px; device DPI is applied later by
the renderer, never multiplied into this value again.

The parser supports common Pango family/size/weight/style descriptions, including
`px` and comma-terminated family names, and the family/size/weight/style subset of
Qt 5/6 QFont strings. It is intentionally not a complete Pango/Qt interpreter:
Pango family lists, stretch, variants, features and variation syntax are unsupported;
QFont attributes beyond family/size/weight/style are not applied. Unsupported or
invalid descriptions fall back; the raw description remains visible. Explicit
`gpui::Font` overrides can supply features/fallbacks/variations directly.

Format references: [Pango descriptions](https://docs.gtk.org/Pango/type_func.FontDescription.from_string.html)
and [QFont serialization](https://doc.qt.io/qt-6/qfont.html#toString).

### Experimental extension API migration

This subtask adds `ShellTypography`, `ShellColors`, `ResolvedShellLook`,
`ResolvedShellPreferences::resolved_look`, typography/motion option and result
fields, and `ClientFramePreview::show_hover`. Use `..Default::default()` when
constructing the unpublished `ShellOptions` with explicit fields. The resolved
preference type is now `Clone` rather than `Copy` because it owns font data;
clone it if needed. No serialization format or core GPUI API changes are introduced.

Checks: 12 shell and five lab headless tests pass, including common font parsing,
scale/override policy and rendered content-style isolation. The preview pointer
safety and Mac unsupported-operation regressions still pass. No GUI was launched;
new typography and look-inspector visuals still need manual acceptance.

Strict shell library/test Clippy and focused shell-lab binary Clippy passed, as
did shell Cargo formatting, lab rustfmt, `git diff --check` and the lab build.
Using `--tests` for Clippy in the shared lab package also selects unrelated lab
binaries and hits existing media-lab warnings; those files were not changed.

### Platform readings, icon source and hover target

The right pane now starts with **Platform preferences — actual readings**. On
macOS/Windows it explicitly says desktop font/accent/contrast/motion/title-bar
action detection is not implemented; empty readings are not native OS defaults.
Appearance and window state are still reported. The API currently has no native
decoration-theme or icon-artwork preference. Client-frame icons are xgpui-shell
artwork; platform button layout controls placement/availability, not their shape.

**Simulate Close hover** forces the close icon's hover background; it does not
enable pointer interactions. An adjacent ON/OFF readout identifies the exact
header and hover colors. Fullscreen hides chrome, so hover/focus simulation
controls are disabled in that mode. A regression checks that simulated hover
sets the control background; existing preview tests keep actions noninteractive.
