# Lifecycle exercise

## Purpose and structure

This is the first consumer contract for evolving GPUI's public API. It exercises
window lifecycle, entity ownership, notifications, events, subscription lifetime,
and basic action/keyboard routing. It does not exercise image or GPU rendering
correctness, input methods, complex layout, or platform parity.

The review of [create-gpui-app](https://github.com/zed-industries/create-gpui-app)
confirmed the useful starter shape: a small entry point, a view implementing
`Render`, and a window created through the application context. Its default
[template](https://github.com/zed-industries/create-gpui-app/blob/main/templates/default/src/main.rs)
uses a Zed Git dependency and `Application::new()`. This exercise uses
the captured local crates and their current `gpui_platform::application()` entry
point. It is an independently written consumer, not a new scaffolding generator.

- [main.rs](../apps/lifecycle/src/main.rs) starts the native application and quits
  after its final window closes.
- [lib.rs](../apps/lifecycle/src/lib.rs) registers actions and menus, owns one
  shared session entity, and creates each view through the same `open_window`
  function used by mock tests.
- Each view retains its observation, event, and release subscriptions. Dropping
  observation/event subscriptions cancels their callbacks. They must not retain
  the view after its final strong owner disappears.

## Commands

From the repository root:

```sh
cargo build -p xgpui-lifecycle --locked
cargo test -p xgpui-lifecycle --lib --locked
cargo clippy -p xgpui-lifecycle --all-targets --locked
cargo fmt --package xgpui-lifecycle --check
```

Tests use GPUI's `TestAppContext` and mock windows. They do not execute the native
entry point, create OS windows, or instantiate the real platform application.
All tests call public APIs. The dev-only `gpui/test-support` feature enables the
mock APIs without making them part of the normal application dependency surface.

Launching requires explicit user authorization. Once authorized:

```sh
cargo run -p xgpui-lifecycle --locked
```

On macOS, the normal build compiles Metal shaders and enables font-kit. An
alternative is `--features runtime-shaders`. macOS is the initial validation
platform; Linux and Windows native behavior has not been verified.

## Expected behavior and headless assertions

| Test | Contract |
| --- | --- |
| `checkpoint_actions_reach_both_windows` | An action in one view updates shared state; both views receive the emitted event, and the other view observes the notification |
| `closing_a_window_releases_its_view_and_keeps_the_survivor` | Closing one window emits the close callback and releases its view; its weak handle expires while the remaining window still receives events |
| `an_external_entity_handle_delays_release_after_window_closure` | Closing a window does not release its view while another strong `Entity` exists; dropping that handle releases it |
| `dropping_subscriptions_stops_callbacks_without_affecting_other_views` | Cancelled observers/subscribers stop receiving callbacks while another view's subscriptions continue working |
| `key_bindings_dispatch_checkpoint_and_new_window` | Mock keystrokes route through the bound actions, mutate the shared session, and open another window |

Observations may be batched. Tests check that a notification arrived rather than
freezing an incidental notification count. Event delivery and release counts
are asserted for the specific operations under test. Entity release effects are
flushed at an application update boundary; dropping a handle outside an update
is not a promise of synchronous release callback delivery.

## Native manual procedure — not yet performed

Use Cmd on macOS, Ctrl on other desktop platforms:

1. Launch the authorized application. One readable window should appear, showing
   one opened window, zero closed windows, and zero released views.
2. Press Cmd-N. Two windows should show a shared opened count of two.
3. Press Cmd-Shift-U in either window. Both should show one shared checkpoint
   and one received event. Notification counts may differ due to batching.
4. Press Cmd-W in one window. The survivor should show one closed window and
   one released view. Another checkpoint should still work.
5. Open another window and close it with the native close control. Check the
   same close/release behavior. Exercise the equivalent native menu commands.
6. Close the final window. The application should exit. Relaunch and verify Cmd-Q.

Record the revision, OS, architecture, performed steps, and deviations when this
procedure is actually run. CI cannot establish menu integration, native close
controls, real event-loop startup, or final process exit from these mock tests.
