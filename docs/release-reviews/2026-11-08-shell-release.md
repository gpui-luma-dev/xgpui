# Shell release preparation — November 8, 2026

Baseline: `release-2026-11-08-cb66d2b2238b` (`cb66d2b2238b02dcfa81d14f7142bdb9ca38571f`). Source before preparation: `a88201c3eea13105665b265ed5894adc60e68d6b`.

## Proposed publications

| Crate | Previous | Candidate | Rationale |
| --- | --- | --- | --- |
| xgpui | 3.0.1 | 3.1.0 | Additive desktop-preference types, platform hooks and App subscriptions. |
| xgpui-linux | 2.0.3 | 2.1.0 | New portal-backed desktop preferences with GNOME/KDE fallbacks. |
| xgpui-shell | Unpublished | 0.1.0 | First experimental publication; app title bars and client frames. |

The other 15 published crates retain their versions. Linux and shell require core 3.1.0 because they call the new API. No blanket dependency bumps.

## Core public API review — pending final user approval

Each item is additive, has no serialization-format impact and requires no migration for existing consumers:

| API | Purpose | Compatibility / migration | Core vs extension |
| --- | --- | --- | --- |
| DesktopPreferences | Optional accent, contrast, motion, scale, font and title-bar action snapshot | New type; unset values preserve defaults | Backend-owned platform state, independent of shell styling |
| TitlebarAction | Portable None, ToggleMaximize, Minimize and Menu actions | New enum; qualify imports if colliding with shell's enum | Shared backend/consumer protocol |
| Platform::desktop_preferences | Read snapshot | Default empty snapshot preserves implementers | Private backend state requires platform seam |
| Platform::on_desktop_preferences_changed | Main-thread update notification | Default no-op preserves implementers; callback follows borrow release | Platform event-loop delivery |
| App::desktop_preferences | Application read access | Additive; styling does not change automatically | App exposes platform state to all consumers |
| App::observe_desktop_preferences | Scoped subscription | Additive; retain Subscription to keep observing | App owns observer lifecycle |
| WindowButtonLayout helpers under test-support | Host-independent frame tests | Adds availability under test-support; production platform availability unchanged | Existing core layout type |

Shell-specific rendering, typography, overrides and title-bar composition live in the extension. Shell API changes from the previous local-only implementation do not break a published shell version.

## Evidence and remaining gates

- Read-only baseline audit: two existing crates require bumps; 15 unchanged; no registry check errors. Shell was absent from the old map and is added here.
- crates.io queried: existing versions match the baseline; xgpui-shell returns 404 (first publication).
- Integrated-source shell CI succeeded on macOS, Linux and Windows: https://github.com/gpui-luma-dev/xgpui/actions/runs/42543357264 . This is headless/build coverage, not interactive desktop acceptance.
- Preparation validation results will be recorded below. Archive/build checks against the final source, user docs review, final API approval, PR/merge approval, tag/draft creation and publication approval remain outstanding.
- No tag, PR, push, upload or Pages deployment is part of this local preparation.

## Local preparation validation

macOS host:

- `cargo test --locked --manifest-path tools/release-sync/Cargo.toml`: 61 tests passed.
- `cargo test --locked --config .cargo/validation.toml -p xgpui-shell --lib -- --test-threads=1`: 16 passed.
- `cargo test --locked --config .cargo/validation.toml -p xgpui --features test-support --lib platform::desktop_preferences::tests -- --test-threads=1`: 1 passed.
- `normalize --check`, `build-manifests --check` and offline lock refresh passed. Only core/Linux local lock identities changed; no external dependency updates.
- Generated website verifier passed: 18 crates, 335 source pages, 25 demo routes. Includes shell navigation, usage guide and core compatibility ledger.
- Manifest preflight recognizes 18 packages and shell's dependency ordering. It still reports the existing xim/naga registry-equivalence issues; this is inventory evidence, not an archive or registry build pass.
- Website staged in `target/site`; no preview server started.

Archive/build verification and a registry-only consumer remain later release gates. No claim of new native Linux/Windows or interactive acceptance is made by these local tests.

Prepared-source crates.io audit passed: 3 ready-for-new-version, 15 unchanged, zero conflicts, disabled publications, dependency-pin errors or registry check failures. This remains a version audit, not archive/build verification.
