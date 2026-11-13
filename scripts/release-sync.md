# Publishing an XGPUI release

After creating a `release-*` tag, run from the repository root:

```sh
just release-preflight
just release-check
just release-publish
```

These commands use `tools/release-sync`, the Rust release tool. They manage all
crates in [release-packages.json](release-packages.json) in dependency order.
No manual per-crate commands are needed.

| Step | Action | Uploads |
| --- | --- | --- |
| `release-preflight` | Compare tagged package archives with crates.io; list new versions, matching published versions and conflicts | No |
| `release-check` | Verify package/build checks where dependencies are published; identify waiting release dependencies | No |
| `release-publish` | Verify/build and upload new versions in dependency order; verify each upload and skip matching published versions | Yes |

All three commands select the nearest reachable `release-*` tag by default,
including a new tag whose GitHub release is still a draft. They read a snapshot
of that exact tag, not the working tree. No checkout switching is required.
Select a specific tag by using the same option on every step:

```sh
just release-preflight --tag release-2026-11-12-da99e6bb8e6d
just release-check --tag release-2026-11-12-da99e6bb8e6d
just release-publish --tag release-2026-11-12-da99e6bb8e6d
```

`check` requires a successful preflight for the selected commit. `publish`
requires a successful check for that same commit and `CARGO_REGISTRY_TOKEN` set
securely. Moving to a new tag commit requires running the checks again.
Publication always rechecks registry state, so rerunning it skips matching
versions rather than uploading them again. Unknown upload results stop the run.

## Output

Each step prints one table with a row for every crate and one final summary.

| Status | Meaning |
| --- | --- |
| `ready` | Version is available and archive checks passed; ready for the check step |
| `passed` | Registry-backed package/build checks passed without upload |
| `waiting` | Named dependencies are part of this release but not published yet; the publisher verifies this crate after uploading those dependencies |
| `published` | Registry version and packaged contents match; skip |
| `uploaded` | Uploaded and registry-verified during this run |
| `conflict` | Version exists with different content; bump it before proceeding |
| `unchecked` | Verification did not complete; investigate before publishing |

A check can complete with `waiting` crates. That does not mean those crates
passed registry-backed builds; final verification happens in dependency order
during publication. Checks cover the current host, not interactive acceptance
on every platform. Registry permissions are enforced when uploading.

Reports and archives are stored automatically outside the repository under
`../xgpui-release-artifacts/<tag-commit>/`. Successful checks are remembered there.
Cargo runs quietly; each saved Cargo command log is capped at 64 KiB. Reports
are mentioned in the terminal when something fails.

## Configuration

| File | Purpose |
| --- | --- |
| [release-packages.json](release-packages.json) | Managed crates, registry names and intended versions |
| [release-check.json](release-check.json) | Repository and fallback baseline for the lower-level version audit |
| [registry-crates.json](registry-crates.json) | Supporting dependencies' crates.io names |
| [crate-names.json](crate-names.json) | Upstream-to-local names for imports |

`just release-dry-run` is an alias for `just release-check`. The lower-level Rust
`release-check` command remains available for historical version audits; Rust
`preflight` is manifest inventory, not the publication workflow above.

Publishing the GitHub draft release and deploying the website are separate steps.
See [the release procedure](../pages/releasing.html) and [tool README](../tools/release-sync/README.md).

Run tool tests without uploads:

```sh
just test --only release-sync
```
