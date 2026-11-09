# Linux shell desktop preferences — issue 149

This is the first subtask of `issue-149/epic`, not completion of the shell or
approval to release the new core API. Native desktop acceptance remains pending.

## Integration

`App::desktop_preferences()` returns optional platform values.
`App::observe_desktop_preferences(...)` reports changes; keep the returned
subscription alive. The shell subscribes once per shell entity and redraws that
entity when the desktop snapshot changes. Applications do not need a D-Bus client.

`ShellOptions::default()` follows the available desktop values. An explicit
`Some(...)` overrides the corresponding value, including `Some(false)`,
`Some(0)` for a black accent and `Some(TitlebarAction::None)` for disabled actions.
`ShellWindow::set_options` replaces the window's overrides and redraws it.

The existing light/dark and button-layout APIs remain responsible for those
preferences. Other platforms currently return an empty desktop snapshot; their
native window chrome is unchanged.

## Providers and fallbacks

| Preference | Provider | Fallback |
| --- | --- | --- |
| Accent | `org.freedesktop.appearance/accent-color` | Shell blue focus accent |
| Contrast | `org.freedesktop.appearance/contrast` | Normal contrast |
| Reduced motion | `org.freedesktop.appearance/reduced-motion`, then GNOME `enable-animations` | Unset; this static shell has no animations to disable |
| Text scale | GNOME `text-scaling-factor`, when exposed by the portal | 1; shell clamps logical target/text scale to 0.5–4 |
| Font description | GNOME `font-name`, when exposed by the portal | Unset; typography integration is a later subtask |
| Title-bar actions | GNOME `action-*-click-titlebar`, when exposed by the portal | Double: maximize/restore; middle: none; right: platform menu |

Only the freedesktop appearance keys are standardized. GNOME namespaces are
optional provider extensions, not promises about KDE or other desktops. This
subtask does not shell out to `gsettings` or parse private KDE configuration.
Unsupported actions are left unset. Requesting a supported action still checks
the window's current capabilities before dispatch.

The platform subscribes before reading an initial snapshot, deduplicates repeated
snapshots and checks again every 15 seconds. Portal operations have a five-second
timeout; failed sessions retry after five seconds. This runs in the background
and does not block window creation. On failure the extra optional preferences
are cleared; existing light/dark, layout and cursor values retain their last-known
values until a new snapshot is available. A successful empty snapshot restores
the documented appearance/layout defaults. GNOME cursor size is validated before
conversion; a missing theme/size retains the previous platform cursor selection.

Invalid types, non-finite/out-of-range accents and invalid text scales become
unset. Removing a previously present optional preference clears it; explicit
window overrides continue to win across updates and reconnects.

## Experimental shell migration

The unpublished shell's former scalar options are now optional overrides. Replace
`high_contrast: true` with `high_contrast: Some(true)`, and similarly wrap explicit
accent, scale and double-click values. New middle/right-click override fields can
be omitted with `..Default::default()`. Callers already using `ShellOptions::default()`
need no source change. Local lab adapters use that default constructor.

## Core public API review required by AGENTS.md

| Addition | Purpose | Compatibility and migration | Why core rather than extension |
| --- | --- | --- | --- |
| `DesktopPreferences` and its optional fields | Normalized, toolkit-independent preference snapshot | Additive; no Serialize/Deserialize implementation or existing serialized format change; unset values preserve consumer defaults | The platform backend owns the portal connection and event loop |
| `TitlebarAction::{None, ToggleMaximize, Minimize, Menu}` | Represent the portable subset of desktop title-bar actions | Additive type; unsupported actions remain unset; name may require explicit qualification in glob-importing consumers | Shared protocol between backend and any window-frame implementation |
| `Platform::desktop_preferences()` | Read the current snapshot | Default implementation returns an empty snapshot, so existing implementers remain valid | An extension cannot read private backend state |
| `Platform::on_desktop_preferences_changed(...)` | Notify after a snapshot update | Default no-op preserves implementers; callback is main-thread and invoked after releasing backend state borrows | Backend-to-App event delivery belongs in the platform contract |
| `App::desktop_preferences()` | Consumer-facing read access | Additive; does not automatically mutate app styling or reduced-motion policy | Exposes the existing platform contract to shell and other consumers |
| `App::observe_desktop_preferences(...)` | Scoped consumer change notification | Additive; dropping Subscription stops callbacks | Uses App observer ownership and update lifecycle |

Existing Linux button-layout helpers are also compiled when `test-support` is
enabled so the Linux frame can be type-checked and unit-tested on a macOS host.
Their production platform availability is unchanged. `TestPlatform` additions
are private test plumbing.

These core additions must receive explicit final user review before this subtask
is finalized or released. No API approval is inferred from implementation approval.

## Validation and remaining work

The initial shell baseline passed two headless host tests. This subtask adds
normalization/removal/deduplication tests, explicit override precedence tests,
and a core observer lifetime/readback regression. Linux frame code is included
in the shell's headless test build so platform-only type errors can be caught.

This macOS environment cannot establish GNOME/KDE behavior. Portal parsing is also
tested through an isolated host harness including the actual Linux source; that
is not a full Linux backend build or a live portal/service-restart test.

Issue 149 still requires provider/fallback acceptance on real desktops, typography
and optional header slots, visual polish, native input/accessibility validation,
diagnosis of the recorded GNOME frame stall, and a separately reviewed release.
No GUI launch or publication is authorized by this document.

### Checks recorded for this subtask

- Core headless unit suite: 427 passed, 2 ignored.
- Shell suite: 6 passed, including the Linux frame path compiled on the host.
- Actual portal module in isolated host harness: 3 normalization/deduplication tests passed.
- Strict shell clippy (`--no-deps`): passed; isolated portal clippy: passed.
- Shell Cargo formatting, changed Rust modules and `git diff --check`: passed.
- Full-workspace formatting is already divergent in unrelated lab/example files.
- Normal core clippy is blocked by the existing `relative(1.618_034)` in
  `geometry.rs`, now diagnosed as `clippy::approx_constant` by Rust 1.99;
  strict core clippy additionally reports existing warnings. These are not changed here.
- Full Linux backend builds, real portal restart tests and native GUI acceptance
  have not been run in this macOS environment.
