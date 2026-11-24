# #439: simulating window hover in tests

Status: **approved by the user on 2026-11-24** following review of PR #440.
Approval covers the APIs and behavior documented here, through code commit
`e34b594`. Further API/behavior changes require review. This records API
approval only; it does not authorize a pull-request merge or release.

Core GPUI public API changes, presented individually. All are in
test-support code (`TestPlatform` / `TestAppContext`), not in production paths.

## 1. `TestWindow::simulate_hover_status_change(&self, hovered: bool)` (new)

- Purpose: let tests drive the platform hover callback that sets
  `Window::is_window_hovered` on Windows, Linux and FreeBSD. Test windows never
  fired it, so hover-gated code (cursor updates, drag auto-scroll in
  xgpui-luma) could not be tested headlessly on those hosts.
- Shape: modeled on the existing `simulate_*` methods: store the state, take the
  callback, call it, put it back.
- Source/serialization compatibility: additive; no serialized formats.
- Migration: none.
- Extension alternative: no. Hover lives in private `Window` state that only the
  platform callback can set; an extension can't reach it.
- [x] Approved: `TestWindow::simulate_hover_status_change`.

## 2. `TestAppContext::simulate_window_hover_status_change(window, hovered)` and `VisualTestContext::simulate_hover_status_change(hovered)` (new)

- Purpose: expose (1) the same way the other per-window helpers
  (`simulate_window_visibility_change` / `simulate_visibility_change`) are
  exposed, so external crates don't need to reach `TestWindow` directly.
- Source/serialization compatibility: additive.
- Migration: none.
- Extension alternative: no, same reason as (1).
- [x] Approved: both context helpers.

## 3. Behavior change: test windows are hovered while active

- Change: activating a test window now also fires its hover callback with
  `true`, and deactivating it fires `false`. `TestWindow::is_hovered()` now
  returns the stored state instead of always `false`.
- Purpose: `is_window_hovered()` follows active status on macOS, so before
  this change the same test passed on macOS and failed on Windows/Linux. Now it
  gives the same answer on every host OS without each test opting in.
- Compatibility: on Windows/Linux/FreeBSD, tests with an active window now see
  `is_window_hovered() == true`, so `reset_cursor_style` sets the test
  platform's cursor where it previously didn't. No existing gpui test depends
  on the old behavior (full `xgpui` lib suite passes on Windows). No change on
  macOS, where the stored hover value is not read.
- Migration: a test that needs an active-but-unhovered window calls
  `simulate_hover_status_change(false)` after activating it.
- [x] Approved: test windows hovered while active.

## Verification

- `window::tests::test_window_hover_follows_activation_and_simulation` fails
  before the fix on Windows ("an active test window must report hovered") and
  passes after it. Its explicit-simulation assertions run only on
  Windows/Linux/FreeBSD, the hosts that read the hover flag.
- Verified locally on Windows only; macOS and Linux rely on CI.
