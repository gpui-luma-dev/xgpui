# Release tool: xgpui-release

`tools/release-sync` is the standalone Rust workspace that implements this
repository's release tooling. Its executable is `xgpui-release`. The Justfile
recipes below are thin shortcuts to that tool; release policy and baseline
selection live in Rust, not in shell recipes.

Run commands from the repository root. See [the release procedure](../pages/releasing.html)
for review, approval, tagging, publication and deployment requirements.

## Everyday commands

| Command | What it checks | Network | Uploads |
| --- | --- | --- | --- |
| `just release-preflight` | Publication readiness for every XGPUI crate: archive/registry comparison and available package/build checks | Registry/index/download requests | No |
| `just release-check` | Changed crates, version availability and internal dependency requirements against completed release history | GitHub and crates.io | No |
| `just release-dry-run` | Candidate archives, registry payload comparison and build verification where dependencies are published | Registry/index/download requests | No |

`release-check` chooses the most recent completed release in the current source
history, using published GitHub release records with the verification table
(`published-verified` or `already-published`). Draft and unverified releases are
excluded. If no newer completed record exists, it uses the configured fallback.
Tags must exist locally; use `git fetch origin --tags` to refresh them.

Override the comparison baseline when needed:

```sh
just release-check --epoch release-2026-11-09-acc3ae63894e
```

A version audit does not compile code or compare the final archive payload.
For example, `ready-for-new-version: 18` means 18 candidate versions are
unpublished and their version proposals pass the audit. It does not mean
publication completed. `unchanged: 1` means one crate needs no new version;
zero error counts mean no corresponding preflight problems were detected.

## Invoke the Rust tool directly

```sh
cargo run --quiet --locked --manifest-path tools/release-sync/Cargo.toml -- COMMAND [OPTIONS]
```

Useful additional commands:

| Command | Purpose |
| --- | --- |
| `prepare-registry --out NEW_DIR --archives` | Offline archive creation and content inspection, without registry-backed build verification |
| `registry-release --mode plan --out NEW_DIR` | Compare all candidate archives with registry payloads without building or uploading |
| `registry-release --mode dry-run --out NEW_DIR` | Plan plus registry-backed package/build verification where prerequisites exist |
| `daily prepare --baseline TAG --report PATH --notes PATH` | Preview version suggestions; `--apply` modifies versions and requires a clean checkout and a reviewed version plan |
| `daily refresh-locks --offline` | Refresh local package identities while rejecting external dependency resolution changes |
| `daily check-locks` | Check that external dependency resolution has not changed |
| `build-manifests --check` | Check generated development manifests |

Both `just release-preflight` and `just release-dry-run` invoke the existing Rust
registry dry-run. They print each crate as READY, PUBLISHED, BLOCKED or NOT VERIFIED,
plus a final answer about publishing all remaining crates. READY means package/build
checks passed; registry ownership/permissions and release approvals are separate.
Dependency blockers name the crates that must be published first.

Dry-run creates a uniquely named directory under `../xgpui-release-artifacts/`
by default. Override it with `just release-preflight --out /absolute/path/new-dir`.
The Rust `preflight` subcommand is only manifest inventory; it does not answer
whether crates can be published.

`registry-release` requires a clean committed source. Its output directory must
be new and outside the checkout. Preparation report paths must also be outside
the checkout. Keep a named persistent directory for evidence, rather than
creating temporary checkouts.

Dry-run results distinguish `verified-not-uploaded`, `already-published` and
`blocked-unpublished-dependencies`. Blocked packages have not passed
registry-backed builds: rerun verification in dependency order as prerequisites
become published. A no-upload run does not establish `release_ready: true`.

Cargo commands run quietly. Each saved Cargo command log is capped at 64 KiB,
retaining its final diagnostics and marking truncation. Reports and archives
remain in the chosen output directory. Credentials are removed from preflight
children and redacted from upload logs.

## Configuration in scripts/

| File | Role |
| --- | --- |
| [release-packages.json](release-packages.json) | Publishable crate map, registry names and intended versions |
| [release-check.json](release-check.json) | GitHub repository and fallback release-history epoch |
| [registry-crates.json](registry-crates.json) | Supporting dependency names on crates.io |
| [crate-names.json](crate-names.json) | Upstream-to-local package naming used during normalization/import |

These files are tracked inputs, not generated logs or publication receipts.

## Publication and validation

To publish the current clean checkout, with `CARGO_REGISTRY_TOKEN` set securely:

```sh
just release-publish
```

This uploads to crates.io. The wrapper supplies the exact current commit SHA to
the existing publisher; it does not switch source commits. Run it from the
approved source. Matching published versions are verified and skipped. An output
directory is generated outside the repository; pass `--out NEW_DIR` to override it.

Publication is a separate explicit action. `registry-release --mode publish`
requires `--confirmation 'publish FULL_COMMIT_SHA'`, a clean source commit and
`CARGO_REGISTRY_TOKEN`. Never invoke publish as a test or include credentials in
command history. Follow the release procedure for explicit approval and the
immutable source tag/draft before any upload. Documentation deployment is
separate from crate publication.

Run the tool's regression tests without publishing:

```sh
just test --only release-sync
```

The [tool README](../tools/release-sync/README.md) lists upstream import/review,
consumer verification and other commands.
