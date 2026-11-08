# Small release preparation — November 7, 2026

Compared with completed tag `release-2026-11-07-4d980860fda6`
(`4d980860fda6dd9bc309ac48eae86d44c7892fa3`). Reviewed development source:
`b7c59ffe6294f652feb8b551ba1bf492cc43e39c` (PR #177).

Scope: CI/test-runner reliability, Rust release tooling, browser compatibility,
and very-alpha Parley paragraph/typeset/emoji work. See [What's new](../WHATS_NEW.md).
No new upstream sync or core text descriptor migration is included.

## Approved version plan — applied

Parley and wgpu are already bumped on main. The approved patch bumps cover actual
packaged changes, primarily compatible dependency requirements replacing exact
pins. A dependency update does not automatically require a major bump.

| Crate | Published | Approved | Reason |
| --- | --- | --- | --- |
| xgpui | 3.0.0 | 3.0.1 | Compatible macros requirement; core Rust code unchanged. |
| xgpui-apple | 4.0.0 | 4.0.1 | Compatible internal dependency requirements. |
| xgpui-macos | 4.0.0 | 4.0.1 | Compatible internal dependency requirements. |
| xgpui-ios | 1.26.4 | 1.26.5 | Compatible core/Apple requirements. |
| xgpui-linux | 2.0.2 | 2.0.3 | Compatible core/wgpu requirements. |
| xgpui-platform | 2.0.2 | 2.0.3 | Compatible backend requirements. |
| xgpui-web | 2.0.2 | 2.0.3 | Compatible core/wgpu requirements. |
| xgpui-windows | 2.1.0 | 2.1.1 | Compatible core requirement. |
| xgpui-wgpu | 2.1.0 | 2.2.0 | Already prepared; browser image-completion and output thread-safety corrections. |
| xgpui-parley | 0.4.0 | 0.5.0 | Already prepared; additive paragraph options, typeset element, color emoji and fixes; very alpha. |
| xgpui-canvas | 0.3.2 | 0.3.3 | Compatible core/shader requirements. |
| xgpui-gamut | 0.3.2 | 0.3.3 | Compatible requirements and backend-only warning cleanup. |
| xgpui-media | 0.5.2 | 0.5.3 | Compatible requirements, browser-build and warning fixes. |
| xgpui-shader | 0.4.2 | 0.4.3 | Compatible requirements, browser output bounds and test-warning cleanup. |
| xgpui-svg | 0.3.2 | 0.3.3 | Compatible core requirement. |
| xgpui-color | 0.3.1 | unchanged | No packaged changes since the completed release. |
| xgpui-macros | 1.0.2 | unchanged | No packaged changes since the completed release. |

`xgpui-shell` and the lab remain unpublished. The 13 approved patch bumps are applied in this PR, with lockfile identities,
exact consumer requirements and release metadata refreshed. Compatible dependency
minimums remain unchanged where they already accept the new versions. Macros and
color are not bumped. Nothing is merged, tagged, published or deployed.

## Non-extension GPUI public API review

Only `gpui-wgpu/src/custom_draw.rs` has Rust source changes in the non-extension
GPUI crates since the baseline. The core GPUI crate itself has no Rust changes.
Parley's direction/language options and `typeset()` use existing GPUI hooks.

| Public API change | Purpose | Source / serialization compatibility | Migration | Could it live in an extension? |
| --- | --- | --- | --- | --- |
| `WgpuCustomOutput` auto-trait behavior in non-atomic Wasm | Preserve `Send + Sync` for custom outputs while keeping browser upload allocations alive until copy completion on the owner thread. | Existing signatures and serialized formats unchanged. The fix restores compatibility with custom-content bounds; native completion behavior remains intact. No Send/Sync promise is added for atomic Wasm. | Recompile with the fixed backend; do not treat the non-atomic workaround as shared-memory/atomic support. | No: ownership and completion bookkeeping are private to the wgpu backend. An extension cannot safely repair their auto-traits. |

`Recording` and `OwnerThread` are private implementation details, not additional
public APIs. This concrete compatibility repair is presented for final user API
review before release; a documentation or implementation approval alone does not
approve it for publication.

## Verification and audit limits

- Read-only release check with explicit latest completed baseline: **13 crates
  require version bumps, 2 already have unpublished versions, 2 are unchanged**;
  no registry lookup failures or pin-update errors.
- The default history check chose an older baseline because its parser recognizes
  only a three-column verification table; the previous release preserves a
  previous/candidate version table plus verification prose. Its initial 15-bump
  result was therefore overbroad. Use the explicit completed baseline until the
  history parser supports that release-note format. This PR does not alter the parser.
- After the approved updates, the explicit-baseline release audit passes: 15
  unpublished candidate versions, 2 unchanged crates, no missing bumps or pin
  updates. External dependency identities in tracked lockfiles are unchanged.
- Registry payload verification is rerun on this PR; publish-time builds remain
  separate. A version audit alone does not establish publish readiness.
- Parley library regressions: **71 passed, 1 ignored**, including paragraph,
  typeset/font-generation, hard-break and color-emoji tests (macOS maintenance host).
- Rust release-tooling unit/integration/CLI tests passed.
- Bash test-runner and paint-runner self-tests passed, including missing-target
  prerequisite handling and CRLF inventory cases.
- Documentation regenerated; verifier passed for **17 crates, 330 source pages
  and all 25 hosted demos**. Node ran through the available Node runtime because
  the shell did not expose a `node` executable.
- `git diff --check` passed. Full native Windows/Linux execution, browser
  interaction, IME/accessibility and real-font COLR sweep/radial acceptance were
  not performed in this documentation pass. The alpha limits remain explicit.
