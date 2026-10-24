# Lifecycle lab

A native GPUI consumer for window lifetime, entity ownership, notifications,
events, subscriptions, and keyboard/action routing. Two windows share a session;
closing one should release its view while the other keeps receiving events.

The executable and five headless tests call the same public `configure` and
`open_window` functions. The tests use GPUI's mock windows and deterministic
executor; they do not start an OS event loop or create native windows.

## Run and validate

From the repository root:

```sh
just --justfile apps/lifecycle-lab/justfile ui
just --justfile apps/lifecycle-lab/justfile build
just --justfile apps/lifecycle-lab/justfile test
just --justfile apps/lifecycle-lab/justfile lint
```

Without `just`:

```sh
cargo run --locked --config .cargo/validation.toml --manifest-path apps/lifecycle-lab/Cargo.toml --target-dir target
cargo build --locked --config .cargo/validation.toml --manifest-path apps/lifecycle-lab/Cargo.toml --target-dir target
cargo test --locked --config .cargo/validation.toml --manifest-path apps/lifecycle-lab/Cargo.toml --lib --target-dir target
cargo clippy --locked --config .cargo/validation.toml --manifest-path apps/lifecycle-lab/Cargo.toml --all-targets --target-dir target -- -D warnings
cargo fmt --manifest-path apps/lifecycle-lab/Cargo.toml -- --check
```

This app has its own workspace and lockfile. Root builds do not include it.
On macOS, build-time Metal compilation requires Xcode and the Metal compiler.
Add `--features runtime-shaders` to compile shaders at runtime instead.
The focused macOS CI job builds the native executable and runs Clippy and the
headless tests with `runtime-shaders`, avoiding a build-time Metal compiler
requirement. It does not launch the app or compile shaders on a GPU. Linux/Windows runtime behavior and browser support are unvalidated;
the multi-window native exercise is not a browser demo.

## Automated contracts

| Test | Behavior asserted |
| --- | --- |
| `checkpoint_actions_reach_both_windows` | One action updates shared state; both views receive the event and the other view observes notification. |
| `closing_a_window_releases_its_view_and_keeps_the_survivor` | Closure removes the window, expires its weak view handle, invokes close/release callbacks, and leaves the survivor receiving events. |
| `an_external_entity_handle_delays_release_after_window_closure` | An external strong handle retains the view after closure; dropping it releases the view. |
| `dropping_subscriptions_stops_callbacks_without_affecting_other_views` | Dropped observation/event subscriptions cancel callbacks without disrupting another view. |
| `key_bindings_dispatch_checkpoint_and_new_window` | Mock keyboard input dispatches actions, changes shared state, and opens a second window. |

Notification assertions allow batching. Entity release effects are flushed at an
application update boundary; dropping a handle outside an update does not imply
synchronous callback delivery. Cancellation tests retain the release subscription
while dropping the observation/event subscriptions.

## Native manual check

Use Cmd on macOS and Ctrl on other desktop platforms:

1. Launch `ui`. Check that one readable window shows one opened window, zero
   closed windows, and zero released views.
2. Press Cmd-N. Both windows should show two opened windows.
3. Press Cmd-Shift-U in either window. Both should show one shared checkpoint
   and one received event. Notification counts can differ because of batching.
4. Press Cmd-W in one window. The survivor should show one closed window and
   one released view. Another checkpoint should still work.
5. Open another window and close it with the native close control. Check the
   same close/release behavior. Try the equivalent native menu commands.
6. Close the final window; the process should exit. Relaunch and check Cmd-Q.

Native checks have not been performed for this port. Record the commit, OS,
architecture, steps, and deviations when testing. Mock tests do not validate OS
menus, native close controls, final process exit, GPU pixels, or IME behavior.

See [API coverage](../../docs/API_COVERAGE.md) and
[compatibility guidance](../../docs/API_POLICY.md).
