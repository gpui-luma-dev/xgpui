# xgpui release imports

`main` is the development branch. `upstream` is a persistent sequence of clean
release snapshots. Do not add local patches to upstream, delete it, or force-push
it. Merge sync PRs with merge commits, never squash or rebase merges.

## Import a release

Run Actions > Sync GPUI release from main, specifying a release such as `1.25.2`.
Optionally provide its full source SHA to verify the fetched branch. Or run:

```sh
gh workflow run sync.yml --repo gpui-luma-dev/xgpui --ref main -f release=1.25.2
```

The workflow replaces `gpui/` on upstream, records the exact source commit in
`upstream.json`, generates `.cargo/config.toml`, and opens or updates one PR into
main. Re-running an unchanged release does not create another import commit.
An import already merged into main does not create another PR. Finish reviewing
one import before requesting another if you want separate release reviews.

The entire captured tree is preserved, including upstream's nested
`crates/transformed-crates` staging copy. Only direct `crates/*/Cargo.toml`
packages generate overrides; nested staging copies are not additional packages.
The source SHA belongs to gpui-unofficial. When available, the import also records
the original Zed SHA and tag reported by upstream's transform-metadata.json.

## Build and validate

The captured crates are standalone Cargo workspaces. There is intentionally no
root Cargo workspace yet. Run Cargo from the repository root so it discovers
the generated local overrides:

```sh
cargo --config .cargo/validation.toml generate-lockfile --manifest-path gpui/crates/gpui-platform-gpui-unofficial/Cargo.toml
cargo --config .cargo/validation.toml generate-lockfile --manifest-path gpui/crates/gpui-unofficial/Cargo.toml
cargo --config .cargo/validation.toml metadata --manifest-path gpui/crates/gpui-platform-gpui-unofficial/Cargo.toml --format-version 1 > /tmp/xgpui-metadata.json
python3 scripts/check_local_dependencies.py /tmp/xgpui-metadata.json
cargo --config .cargo/validation.toml check --manifest-path gpui/crates/gpui-platform-gpui-unofficial/Cargo.toml --features font-kit,runtime_shaders
cargo clippy --config .cargo/validation.toml --manifest-path gpui/crates/gpui-platform-gpui-unofficial/Cargo.toml --features font-kit,runtime_shaders
cargo --config .cargo/validation.toml test --manifest-path gpui/crates/gpui-unofficial/Cargo.toml --lib --features gpui_platform/runtime_shaders
cargo fmt --manifest-path gpui/crates/gpui-unofficial/Cargo.toml --check
python3 -m unittest discover -s scripts -p 'test_*.py' -v
```

`runtime_shaders` avoids requiring precompiled Metal shaders for checks. These
commands do not launch GUI examples. macOS checks do not establish iOS, Linux,
Windows, or browser compatibility. Keep any inherited upstream failures visible;
do not fix them on the clean upstream branch.

Cargo patches participate in version resolution; they do not force a lower
version over a newer compatible registry release. The dependency check rejects
such drift. If needed, use `cargo update --manifest-path MANIFEST -p PACKAGE
--precise CAPTURED_VERSION`, then rerun metadata and validation. Establish and
retain development lockfiles when adding reproducible bench applications.

Generated `.cargo/config.toml` is reserved for importer output. Configuration in
this repository is not inherited by a separate consumer such as gpui-luma;
that consumer must supply its own patches to one pinned xgpui commit. The separate
`.cargo/validation.toml` restores Zed's Proptest dependency and is not generated
or overwritten by synchronization. Pass it explicitly to Cargo when testing.

## Local modifications and conflicts

Add documentation under docs, test applications under apps, and eventual Rust
changes under gpui on main. If a sync PR conflicts, create an integration branch
from main, merge upstream there, resolve conflicts and validate, then open its PR
into main. Keep upstream as the unmodified import history. Do not resolve
conflicts by merging main into upstream.

## GitHub settings

- Default branch: main.
- Enable merge commits; preserve the upstream branch after PR merges.
- Allow Actions to create pull requests. sync.yml requests contents and PR write
  permissions; no personal access token is stored in the repository.
- If a token-created PR's checks require approval, approve the workflow runs in
  GitHub, or manually dispatch validate.yml on upstream.

The sync workflow never publishes crates, merges PRs, or launches applications.
Imported licenses remain with the captured source. README files are preserved
as imported, without local edits.

### Current organization policy

gpui-luma-dev currently prevents GitHub Actions from opening pull requests.
Repository variable `SYNC_AUTO_PR=false` selects the manual-PR fallback: the
workflow still imports and pushes upstream, and its summary provides a compare
link. Open or update the PR manually after each import:

```sh
gh pr create --repo gpui-luma-dev/xgpui --base main --head upstream --title "Sync gpui-unofficial RELEASE" --body "Review the imported release and validation results. Merge with a merge commit and preserve upstream."
```

If an organization administrator later enables Actions PR creation, set
`SYNC_AUTO_PR=true` and enable the repository's Actions PR creation setting.
No organization-wide policy is changed by this setup.

## Initial 1.25.2 validation baseline

The initial import was checked with Rust 1.99.0 on macOS:

- Snapshot Git tree exactly matches source commit
  `4eadf3cd8c4bcbdd4e8652aa0fbe5e20712cc8c6`.
- All resolved captured packages use local sources.
- Platform `cargo check` and `cargo clippy` pass with `font-kit,runtime_shaders`.
- All four importer regression tests pass.
- With Zed's Proptest revision restored, core library tests pass: 369 passed,
  0 failed, 2 ignored.
- Core `cargo fmt --check` reports formatting differences in captured source.

Formatting differences are preserved in the clean import. They are not a CI
formatting gate for captured upstream source.

### Cause and fix of the initial test compilation failure

The original [Zed workspace manifest](https://github.com/zed-industries/zed/blob/d25fa01ccd788d812f236810b71af4feff6b2e38/Cargo.toml)
pins Proptest to Git revision `3dca198a8fef1b32e3a66f1e1897c955b4dc5b5b`.
The captured release instead declares `proptest = "1"`, which resolved to 1.12.0
and proptest-macro 0.6.0 during setup. GPUI's wrapper emits
`proptest_path = "::gpui::proptest"`; Zed's pinned macro accepts this string,
whereas the newer published macro expects a Rust path. Its error generation
panics while parsing, producing the misleading unexpected-token diagnostic at
`randomized_element_tree.rs:1648` before any tests run.

Validation now restores the exact Git dependency with `.cargo/validation.toml`
and resolves fresh lockfiles for the standalone crates.
It verifies both Proptest packages resolve to that Git revision. No captured
Rust source is changed or test disabled. Review the override when adopting
another GPUI release.

The test command also enables `gpui_platform/runtime_shaders`, since features
from the separate platform build command do not carry into the library test
command. This avoids requiring precompiled Metal shaders for headless tests.

To validate upstream using the current workflow and dependency configuration:

```sh
gh workflow run validate.yml --repo gpui-luma-dev/xgpui --ref main -f source_ref=upstream
```
