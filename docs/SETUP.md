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
cargo metadata --manifest-path gpui/crates/gpui-platform-gpui-unofficial/Cargo.toml --format-version 1 > /tmp/xgpui-metadata.json
python3 scripts/check_local_dependencies.py /tmp/xgpui-metadata.json
cargo check --manifest-path gpui/crates/gpui-platform-gpui-unofficial/Cargo.toml --features font-kit,runtime_shaders
cargo clippy --manifest-path gpui/crates/gpui-platform-gpui-unofficial/Cargo.toml --features font-kit,runtime_shaders
cargo test --manifest-path gpui/crates/gpui-unofficial/Cargo.toml --lib
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
that consumer must supply its own patches to one pinned xgpui commit.

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
