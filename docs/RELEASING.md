# Repeatable crate releases

Release preparation and publication are manually triggered. Preparation opens a PR;
it never merges or publishes crates. Successful publication marks its exact source
with a tag and GitHub release automatically.

## Prepare the next release

Run **Prepare daily release PR** on `main` in GitHub Actions, or:

```sh
gh workflow run prepare-daily-release.yml --ref main
```

The workflow compares against the latest ancestor `release-*` tag. An optional
`baseline` input selects a specific release tag. It proposes patch bumps for
changed crate directories and propagates bumps through published dependencies,
updates internal pins and lockfiles, regenerates canonical manifests, and opens
a `codex/release-COMMIT` PR with an old/new version table. Documentation outside
crate directories does not trigger crate bumps. Crate READMEs do: they ship in
archives. Shared packaging inputs conservatively select all crates.

Review the proposed versions: source comparisons cannot decide whether an API
change is breaking. Adjust minor/major versions and related pins before approving
the PR. The release map is `scripts/release-packages.json`. Registry planning is
the final authority on whether existing versions match published payloads.
Generated PRs use `GITHUB_TOKEN`; because those PR events do not start CI, the
workflow explicitly dispatches **Validate repository tooling** on the PR branch.
If that workflow is disabled, it respects the setting and dispatches a no-upload
registry plan instead. This fallback does not claim full native backend coverage.
Repository Actions settings must allow workflows to create pull requests.
Rerunning preparation for the same source reuses its branch/PR without force pushes.

Read-only local proposal:

```sh
python3 scripts/daily_release.py prepare --report /tmp/release-proposal.json --notes /tmp/release-proposal.md
```

## Publish the approved commit

After explicitly approving and merging the version PR, run a registry plan:

```sh
gh workflow run prepare-registry-release.yml --ref main -f mode=plan
```

Review the report, then publish the exact main commit:

```sh
gh workflow run prepare-registry-release.yml --ref main -f mode=publish \
  -f 'confirmation=publish FULL_COMMIT_SHA'
```

The publisher verifies candidates before uploading, skips matching existing
versions, rejects conflicting versions, and verifies registry contents after each
upload. `CARGO_REGISTRY_TOKEN` remains scoped to the publication step.

Only a complete successful publication report can finalize a release. Finalization
creates `release-YYYY-MM-DD-COMMIT` at the verified source commit and a GitHub release
listing all versions and outcomes. The commit suffix permits multiple releases per
day. Retries reuse that identity; an existing tag pointing elsewhere is rejected.
The tag date comes from the source commit in UTC, so retries on a later day keep the same identity.

If finalization fails after crates were uploaded, rerun the **same workflow run**
on the same commit. Matching published crates are verified and skipped; missing
ones resume. Tag/release creation is retried. No version bump is needed just to
recover bookkeeping. The report artifact remains available even on failure.

The initial baseline is `release-2026-10-30`; later releases use the automated naming.
