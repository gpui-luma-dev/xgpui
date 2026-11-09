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

Values resolve per field: standardized portal value, desktop portal extension,
optional native desktop reader, then the consumer's default. Explicit window
options continue to override that result. False, black and `TitlebarAction::None`
are values, not missing settings.

| Preference | Portal sources | Optional native fallback / final default |
| --- | --- | --- |
| Accent | freedesktop appearance `accent-color` | No guessed theme palette; shell blue |
| Contrast | freedesktop appearance `contrast` | GNOME accessibility `high-contrast`; normal |
| Reduced motion | freedesktop `reduced-motion`, GNOME `enable-animations`, KDE `AnimationDurationFactor` | GNOME animation flag or KDE duration factor; unset |
| Text scale | GNOME `text-scaling-factor` | GNOME text scale; 1, shell clamps to 0.5–4 |
| Font description | GNOME `font-name`, KDE General `font` | Native GNOME/KDE font; unset |
| Title-bar actions | GNOME `action-*-click-titlebar` | GNOME actions or portable subset of KWin actions; maximize / none / menu |
| Light/dark | freedesktop `color-scheme` | GNOME `color-scheme`; existing appearance API |
| Button layout, cursors | GNOME portal extensions | GNOME native settings; existing platform defaults |

The native reader is selected by colon-separated `XDG_CURRENT_DESKTOP` tokens:
GNOME/Unity (including `ubuntu:GNOME`) or KDE. Other desktops use the portal and
consumer defaults. Merely installing GNOME schemas on KDE does not activate the
GNOME native reader. KDE/GNOME portal extensions remain optional; they are not
freedesktop standards.

GNOME uses three concurrent, read-only `gsettings list-recursively` schema reads.
KDE uses concurrent `kreadconfig6` queries, with `kreadconfig5` as an optional
fallback, for `kdeglobals` font/animation preferences and KWin title-bar actions.
KConfig handles config search paths, escaping and inheritance; the toolkit does
not parse private INI files. No command uses a shell or writes preferences.
Commands run in the background with five-second deadlines and kill-on-drop;
missing tools, denied sandbox access, invalid values and failed queries leave
individual preferences unavailable. Applications need no command or D-Bus code.

The portal subscribes before reading, deduplicates across reconnects, refreshes
every 15 seconds and retries failed sessions after five seconds. Native results
are cached for 15 seconds during a healthy portal session, so portal signals do
not launch commands repeatedly. During outages native preferences are still read
on retries. Appearance/layout/cursor values retain their last-known values only
when the failing portal and native reader cannot supply them. Extra optional
preferences clear when unavailable. Successful empty snapshots reveal native
fallbacks or restore consumer defaults. Explicit window overrides survive both.

KDE font descriptions retain QFont serialization; GNOME descriptions retain Pango
syntax. Neither is a family name. KDE display DPI is deliberately not converted
to logical text scale (that would risk double scaling). Unsupported KWin actions,
such as shading or vertical-only maximization, remain unavailable. KDE native
button-layout, palette and cursor config are not guessed; their existing portal
or consumer defaults apply. No app-wide fonts, animations or theme are changed.

### Applying the look

The shell-look subtask now applies common desktop font descriptions to the client
frame title and adds a shell-lab inspector for resolved typography, colors,
focus/hover, dimensions and click actions. It uses the production renderer's
style resolver. Application content remains independently styled; reduced motion
is reported as policy because the frame has no animation. See
[the walkthrough and parser limits](shell-lab.md#how-the-look-is-applied).
New visuals remain pending manual acceptance at the end of issue 149.

Provider contracts checked against upstream sources:
[GNOME interface schema](https://github.com/GNOME/gsettings-desktop-schemas/blob/master/schemas/org.gnome.desktop.interface.gschema.xml.in),
[KDE portal settings](https://github.com/KDE/xdg-desktop-portal-kde/blob/master/src/settings.cpp),
and [KWin settings](https://github.com/KDE/kwin/blob/master/src/kwin.kcfg).

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

Issue 149 still requires provider/fallback acceptance on real desktops, optional header slots, visual polish, native input/accessibility validation,
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

### Desktop fallback subtask

Branch `issue-149/desktop-fallbacks` starts from `issue-149/epic` and carries
the prerequisite preference commit from PR #182 without merging either PR.
This subtask adds no public API beyond that separately documented prerequisite;
the native readers and merge policy are private Linux backend details.

Host checks compile the actual Linux source using an isolated Cargo harness
with ashpd, calloop, smol and the local core crate. They cover native normalization,
missing tools, portal precedence, removal, outage behavior and deduplication.
This does not establish live GNOME/KDE behavior or constitute a full Linux build.
Final native validation remains scheduled for the end of issue 149.

Checks for this subtask: 10 actual-source Linux preference tests and six shell
headless tests passed; strict preference-module Clippy passed (only unused-code
warnings caused by the isolated harness were allowed); `cargo fmt` for the Linux
crate, changed core-module rustfmt and `git diff --check` passed. No GUI launched.
