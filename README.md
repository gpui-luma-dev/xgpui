# xgpui

An extensible GPUI framework with custom shaders, wide-gamut rendering and tools
for building applications such as photo editors, backed by documentation and tests.

XGPUI started from [gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial),
which captures GPUI and its supporting crates from
[Zed](https://github.com/zed-industries/zed). We are moving toward using Zed
directly as our upstream.

**[Read the documentation →](https://xgpui.dev/)**

## How it works

- Modified framework/backend crates are captured under `gpui/`; supporting
  crates use the matching published `gpui-unofficial` packages. `upstream.json`
  records the release, source commit, local packages and registry dependencies.
- The manual **Sync GPUI release** workflow imports a selected release onto
  `upstream`, keeping that branch separate from local development.
- Release updates are reviewed and integrated into `main`. Documentation,
  tooling and experimental changes live alongside the captured code.

## Build

From the repository root:

```sh
just         # List sample apps and development commands.
just build   # Build optimized native libraries.
just lab-shader # Launch the shader examples.
```

Recipes use Bash (on Windows, Git for Windows Bash must be on PATH).
Without just, use `cargo build --release --locked`.

`just lab-build` builds the unpublished extension labs and benchmarks from
`apps/xgpui-ext-lab/`. See [Labs & benchmarks](https://xgpui.dev/extension-lab.html)
for targets, launch commands and why the lab package is unpublished.

`just build` builds the libraries supported by the selected target; it does not
launch an application. See [Local Development](https://xgpui.dev/local-development.html) for Windows,
Linux and macOS build/run commands, or [build harness details](https://xgpui.dev/local-development.html#build-harness-and-test-runner).

## Crate release preflight

Run `just release-check` (Rust tooling; requires Git and curl) while developing or before asking an
agent to prepare a release. It detects crate changes, including staged,
unstaged and non-ignored new files, and checks crates.io for versions that
would prevent those changes from being published.

The tracking epoch is `release-2026-11-03-712732c7cf07`, configured in
`scripts/release-check.json`. Override it with `just release-check --epoch TAG`.
The epoch is a trusted completed release baseline. For each crate, a later
non-draft GitHub release advances that baseline only when its notes contain a
three-column verification row (`Crate | Version | Result` or `Verification`)
with `published-verified` or `already-published`, the version agrees with the
tagged manifest, and that version exists on crates.io. Release agents must
retain these verification rows when editing release notes. Partial/draft or
unverified releases never advance the baseline. Tags must be available locally;
run `git fetch origin --tags` when the command reports a missing release tag.

Results distinguish:

- **Version bump required:** crate files changed, or an internal dependency
  changed, but the current version is already published.
- **Dependency pin update required:** a candidate has a new version but still
  references an old internal dependency version.
- **Ready for new version:** changes have an unpublished version and matching
  internal pins. This is ready for release preparation, not publication approval.
- **Unchanged:** no changes since that crate's verified baseline; no bump needed.

The command exits successfully when no changes would be missed, including when
there is nothing to release. Missing registry/history checks and disabled
publication for changed candidates fail the check. Version bumps remain a
review decision; this command never edits versions or uploads anything.

Change detection conservatively covers crate directories (including tests,
examples and READMEs, excluding Cargo.lock), explicitly configured external
readme/license/build files, and transitive internal dependencies, including
optional/target-specific and versioned development dependencies. Root website
and tooling edits alone do not require crate bumps. This is a Git-based change
check, not a normalized archive comparison; other shared inputs outside crate
directories need release review. Builds, API compatibility and exact payload
verification remain separate release steps.

The check reads public GitHub release metadata and crates.io metadata without
publication credentials. Use `just release-check --report /tmp/xgpui-release-check.json`
to save the machine-readable report outside the checkout, or `--json` for JSON
on stdout. Without just, run `cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- release-check`.

## Documentation website

The website at <https://xgpui.dev/> is the static `pages/` directory: guides,
crate pages and the browser demos. Preview and check it without building:

```sh
just pages-serve
just pages-check
```

See [Edit this website](https://xgpui.dev/local-development.html#edit-this-website) for layout and demos, and [website deployment](https://xgpui.dev/releasing.html#website-deployment) for publishing.

See [documentation instructions](https://xgpui.dev/local-development.html#edit-this-website) for maintenance and checks.

## Contributing

Contributions are welcome, including fully AI-generated code. You are responsible
for its quality. See [Contributing](CONTRIBUTING.md) for the short guide.

## License

[Apache License 2.0](LICENSE), consistent with GPUI from Zed.
Upstream copyright and license notices are preserved.
