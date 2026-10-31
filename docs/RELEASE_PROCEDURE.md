# Release recipe

## 1. Prepare the release PR

- Compare `main` with the last completed release.
- Propose crate versions and update dependency pins and lockfiles.
- Write release notes with change titles, short summaries, links and a version table.
- Run validation and a crates.io plan; verify and skip unchanged published versions.

## 2. Review and merge

- Approve the versions, notes and release PR.
- Merge the PR and record the exact commit SHA.

## 3. Create the tag and release record

**Complete this before uploading any crates.**

- Tag the approved commit as `release-YYYY-MM-DD-COMMIT`, using the commit date in UTC and its first 12 hex characters.
- Create the GitHub release record as a **draft**, containing the reviewed notes and version table.
- Verify the remote tag points to the approved commit.

## 4. Publish and verify crates

- Obtain publication approval.
- Run **Prepare registry release → publish** against the tag, confirming `publish FULL_COMMIT_SHA`.
- Verify all intended versions; skip matching versions already published.
- Build and run a registry-only consumer.
- Add the results and workflow link to the existing release record, then publish the draft.

**On failure:** retain the tag and draft. Retry the same commit, skipping verified uploads. Never move a tag after an upload. Keep the record in draft and hold documentation deployment until verification succeeds.

## 5. Deploy documentation

- Manually run **Publish documentation** after successful crate publication.
- Verify the live website, documented versions, galleries and Wasm bundles.
- Add the deployment link to the release record.

## Automation requirements

Tag and release-record creation must precede uploads, and reviewed notes must be preserved. Until the workflows support this order, perform step 3 manually and restore the reviewed notes if the publication finalizer replaces them.

Use the last **completed** release as the preparation baseline; exclude unfinished release tags.
