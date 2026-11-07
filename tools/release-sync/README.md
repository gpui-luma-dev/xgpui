# Rust release tooling

`xgpui-release` owns downstream normalization, release preparation, archive
verification, upstream import/review and explicitly confirmed registry publication.
It replaces the Python release scripts. It is a standalone Cargo workspace;
its tests run in the `release-sync` suite of `just test` on every host.

```sh
just test --only release-sync
just release-check
```

Run commands from the repository root:

```sh
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- COMMAND [OPTIONS]
```

| Command | Options / purpose |
| --- | --- |
| `normalize` | `--check`, `--map PATH`; normalize captured manifests |
| `build-manifests` | `--check`; generate the development workspace |
| `preflight` | Read-only all-target manifest inventory as JSON |
| `prepare-registry` | `--out NEW_DIR`, optional `--archives`; offline release plan / archive inspection |
| `release-check` | Optional `--epoch TAG`, `--report PATH`, `--json`; read-only GitHub/crates.io version check |
| `daily prepare` | `--baseline TAG --report PATH`, optional `--notes PATH --apply`; propose reviewed versions |
| `daily check-locks` | Reject external dependency resolution changes |
| `daily refresh-locks` | Refresh local identities, then check external resolutions; optional `--offline` |
| `daily finalize` | `--report PATH --repo OWNER/REPO`; finalize a verified publication report |
| `registry-release` | `--out NEW_DIR --mode plan\|dry-run\|publish`; isolated archive and registry verification |
| `verify-packages` | `--out NEW_DIR`, optional `--offline --nightly-web`; archive and isolated consumer checks |
| `verify-packages` retry | Existing `--out DIR --retry-consumers`, optional `--profiles-only`; retain previous results |
| `verify-color` | Optional `--out NEW_DIR --offline --native-metal`; dedicated color archive/consumer checks |
| `import VERSION` | Optional `--source CHECKOUT --expected-sha SHA`; requires matching `release/VERSION` branch |
| `review prepare` | `--release VERSION --snapshot NEW_DIR --report PATH --normalizer BINARY --map PATH`, optional `--expected-sha SHA` |
| `review code\|cargo\|metadata` | `--snapshot DIR --report PATH`; apply reviewed tree proposals on a dedicated branch |
| `context` | `--report PATH --output MARKDOWN --json-output JSON`; exact upstream Zed context |
| `review-notes` | `--report PATH --context MARKDOWN --output PATH`; format upstream review notes |
| `check-local-dependencies` | `--metadata PATH`; check Cargo metadata identities against provenance |

All commands accept `--root PATH`. Output directories for package and registry
verification must be new and outside the checkout. Release-check/preparation
reports must also be outside the checkout. Registry HTTP requests require curl;
GitHub context and release finalization use the authenticated `gh` CLI. Cargo
and Git are required. The release path does not invoke Python.

Publication remains a separate explicit action: `registry-release --mode publish`
requires `--confirmation 'publish FULL_COMMIT_SHA'`, a clean source commit, and
`CARGO_REGISTRY_TOKEN`. Credentials are removed from preflight Cargo children and
redacted from upload logs. All collisions are checked before any upload, and
each upload is checksum/payload verified before its dependents proceed. Unknown
upload outcomes stop; quota retries require an explicit HTTP 429 deadline and a
fresh absent-version check. Never run publication as part of tests.

Tests use temporary repositories, synthetic archives and fake registry state;
they do not upload, create tags/releases/PRs, or require registry credentials.
See [the release guide](../../docs/RELEASING.md) for the approval and version policy.
