# Public API exercise coverage

The goal is to make downstream GPUI changes measurable while retaining upstream
improvements. Coverage begins with capabilities, not a count of public symbols.
See [API_POLICY.md](API_POLICY.md) for how changes are evaluated.

**C** = consumer compilation, **H** = named headless behavioral assertions,
**M pending** = native manual procedure exists but has not been performed,
**planned** = no exercise evidence yet. This map does not claim full coverage of
an API family. Test names are evidence identifiers, not exemptions from changes.

| Capability | Public API surface | Evidence / exercise | Gaps |
| --- | --- | --- | --- |
| Native startup and exit | `gpui_platform::application`, `Application::run`, `App::activate`, `App::quit` | C: lifecycle `main`; M pending | No automated real event-loop startup or process-exit assertion |
| Window creation and closure | `App::open_window`, `WindowOptions`, `WindowBounds`, `WindowHandle`, `App::windows`, `App::on_window_closed`, `Window::remove_window` | C/H: `closing_a_window_releases_its_view_and_keeps_the_survivor`, `key_bindings_dispatch_checkpoint_and_new_window`; M pending | Native close controls and OS window activation |
| Entity ownership and release | `App::new`, `Entity::clone/read/update/downgrade`, `WeakEntity::upgrade`, `Context::on_release` | C/H: `an_external_entity_handle_delays_release_after_window_closure`, window closure test | Async references and cross-window entity rehosting |
| Observation and events | `Context::observe/subscribe/notify/emit`, `EventEmitter`, `Subscription` | C/H: `checkpoint_actions_reach_both_windows`, `dropping_subscriptions_stops_callbacks_without_affecting_other_views` | Reentrant callbacks, async emission, ordering between event families |
| Actions and keyboard routing | `actions!`, `KeyBinding`, `App::bind_keys/on_action`, `Context::listener`, `FocusHandle`, `track_focus` | C/H: checkpoint and key-binding tests; M pending | Focus traversal, input methods, conflicting contexts, native menu routing |
| Native menus | `Menu`, `MenuItem`, `App::set_menus` | C: lifecycle configuration; M pending | Menu behavior is not asserted by mock tests |
| Composition and layout | `Render`, `IntoElement`, `div`, `Styled` | C: lifecycle view; headless window rendering is incidental | **planned:** bounds assertions, clipping, scrolling, hit testing |
| Text | text styling and string children | C: lifecycle labels only | **planned:** shaping, selection, fallback fonts, scaling |
| Image and rendering baseline | image upload/display, transforms, clipping, alpha | **planned:** rendering exercise next | No color accuracy, screenshot, or GPU correctness claims |
| Tasks and scheduling | executors, tasks, cancellation, timers | **planned** | No exercise yet |
| Platform and display services | display selection, scale changes, clipboard, dialogs | **planned** | Native hardware and multiple-platform coverage required |
| Custom rendering and wide gamut | API not chosen | **planned:** design after rendering baseline | Preserve existing default rendering behavior; define color and synchronization contracts first |

## First milestone

The [lifecycle exercise](LIFECYCLE.md) uses the same public application functions
in its native executable and five headless tests. CI builds the workspace and
executes those tests. Native checks remain pending until the application is
explicitly launched and the procedure is completed.

The next exercise should establish image rendering, scaling, clipping, alpha,
and resize behavior before introducing shader or color-output API extensions.
