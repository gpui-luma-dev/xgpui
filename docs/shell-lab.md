# Shell Lab

Interactive diagnostics for xgpui issue 149. The lab reads the platform's current
preferences and uses the same `ShellOptions::resolved_preferences` resolver as the
client frame. It never writes desktop settings.

## Run

From the xgpui repository on `issue-149/shell-lab`:

```sh
just profile=dev lab-shell

# Ask for the xgpui client frame on Linux (the compositor can decline).
just profile=dev lab-shell --client-decorations

# Exercise a non-resizable window.
just profile=dev lab-shell --client-decorations --fixed-size
```

This branch carries the desktop-preferences prerequisite from draft PR #182.
Neither PR has been merged into the epic yet. Merge/reconcile #182 into
`issue-149/epic` before integrating the lab PR. Do not target main.

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
- **Font / reduced motion:** detected values are displayed, but font typography
  integration remains future work and the current frame has no animation.
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

- Change contrast, accent and logical scale in the right pane to inspect their
  rendered result. Show Focus Ring simulates focus on the decorative close icon
  so accent changes remain visible without making preview controls actionable.
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
   Only exposed portal keys can be reported; some GNOME settings are optional
   provider extensions, not standardized cross-desktop settings.
3. Override contrast, accent, scale and click actions in this window. Check that
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
