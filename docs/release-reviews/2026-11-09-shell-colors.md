# Shell colors release preparation — November 9, 2026

Baseline: `release-2026-11-09-acc3ae63894e`. Candidate includes issue #192 shell changes on `issue-192/shell-colors`. The user requested a source tag and release record from this branch; integration into main remains separate.

| Crate | Published | Candidate | Reason |
| --- | --- | --- | --- |
| xgpui-shell | 0.1.0 | 0.2.0 | App-controlled shared titlebar background and frame palette; new fields affect exhaustive public struct literals. |

User requested this minor bump explicitly. Other 17 crate versions and core 3.1.0 minimum remain unchanged. No core public API changes and no serialization changes.

## Behavior and migration

`ShellOptions::titlebar_background` overrides the complete shared header, including reserved native-control space and trailing padding. `colors` overrides the shared palette; the background-only override wins over its header. Reset either option to None to expose the remaining palette/default; reset both for original resolution.

Explicit palettes take precedence over default high contrast. Applications own contrast and theme/activation variants. Focus uses the resolved accent, or palette text in high contrast. Runtime updates preserve content/titlebar entities, focus and callbacks. Native macOS artwork, native appearance synchronization and compositor decorations are outside this change.

Exhaustive `ShellOptions` and `ResolvedShellPreferences` literals need `titlebar_background: None` and `colors: None`. Options callers may use `..Default::default()`. The lab adapter has been updated.

## Validation

- macOS shell library tests: 17 passed, including explicit black, palette precedence/reset and runtime composition regressions.
- Generated website verifier: passed for 18 crates, 335 source pages and 25 demo routes.
- Read-only registry audit: 1 ready-for-new-version, 17 unchanged, no check failures or incompatible dependency requirements.
- Offline lock refresh changed the local shell identity only; external dependencies retained.
- `cargo package --locked --manifest-path extensions/crates/xgpui-shell/Cargo.toml --allow-dirty`: archive creation and compile verification passed against published dependencies; README, Apache license and all three Rust sources included.
- Native Windows/Linux and interactive visual/accessibility acceptance remain separate; the workflow now covers the issue-192 epic.

## Publication handoff

The user will publish from the command line. The agent must not upload this crate. Before upload, create and verify the source commit-derived tag and draft GitHub release record as requested. This does not authorize merging into the epic or main; any later merge still requires explicit approval. Preserve notes, then verify the uploaded version/checksum and a fresh registry consumer afterward. Docs deployment is separate.
