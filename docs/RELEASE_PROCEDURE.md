# Release recipe

## 1. Prepare

Say **“prepare today's release.”**

- Compare `main` with the last completed release.
- Open a PR with proposed crate versions and release notes: titles, short summaries and change links.
- Run checks and a crates.io plan. Verify unchanged published versions and skip them.

## 2. Review and merge

- Agree on the version numbers and notes.
- Explicitly approve merging the release PR.
- Record the exact merged commit SHA.

## 3. Tag and draft — before uploading

- Create `release-YYYY-MM-DD-COMMIT` at that SHA, using the commit date in UTC and its first 12 hex characters.
- Create a **draft GitHub release** with the reviewed notes and version table.
- Confirm the remote tag points to the approved commit. Never move it after an upload.

## 4. Publish crates

- Explicitly approve publication.
- Run **Prepare registry release → publish**, selecting the tag and entering `publish FULL_COMMIT_SHA`.
- Verify every crate is published and verified, or already published and verified.
- Check that a registry-only consumer builds and runs.

**If publication fails:** keep the tag and draft. Retry the same commit; skip verified uploads and resume missing ones. Do not publish the release record or deploy Pages until verification completes.

## 5. Publish the release record

- Preserve the reviewed highlights and version table.
- Add verification results and the publication workflow link.
- Publish the draft GitHub release.

## 6. Deploy Pages

- Manually run **Publish documentation** after crate publication succeeds.
- Verify `xgpui.dev`, documented versions, galleries and both Wasm bundles.
- Add the deployment link to the release record.

## Manual steps still required

Current automation creates the tag/release **after** uploads and replaces notes with a version table. Until updated, create the tag and draft manually in step 3, then restore the reviewed notes and publish the draft in step 5.

If an unfinished release tag exists, give preparation the last **completed** release tag as its baseline.
