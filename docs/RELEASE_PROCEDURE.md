# Release recipe

This procedure is for the agent to execute. The user requests a release, reviews
its proposed versions and notes, and gives the approvals below. The agent runs
the commands, monitors workflows and handles failures.

## 1. Prepare — agent

When the user says **“prepare today's release”**:

- Fetch current `main` and identify the last successfully completed release.
- Compare code, packaged documentation, dependencies and publication changes.
- Run release preparation and open a PR with proposed old → new crate versions.
- Prepare release notes with change titles, short summaries, links and the version table.
- Review automatic version proposals against the project's agreed release policy.
- Run relevant checks and a no-upload crates.io plan; resolve failures and version conflicts.
- Present the PR, proposed versions and validation results to the user.

**User:** review the proposal and approve or revise the version numbers. No commands or manual preparation are required.

## 2. Merge — agent, after approval

**User:** explicitly approve merging the release PR.

**Agent:** merge the approved PR, remove its branch, record the exact merged commit SHA and confirm the final registry plan applies to that source. Preserve unrelated local edits.

## 3. Create the tag and release record — agent

**Before any crates.io upload:**

- Tag the approved commit as `release-YYYY-MM-DD-COMMIT`, using its UTC commit date and first 12 hex characters.
- Create a draft GitHub release containing the reviewed notes and version table.
- Verify the remote tag points to the approved commit and present the draft to the user.

## 4. Publish and verify crates — agent, after approval

**User:** explicitly approve crate publication.

**Agent:**

- Publish the exact tagged commit in dependency order.
- Verify and skip matching versions already published; verify each new upload.
- Confirm all intended versions are verified and a registry-only consumer builds and runs.
- Add results and the workflow link to the existing release record, preserve its highlights, and publish the draft.
- Report completion to the user.

**On failure:** retain the tag and draft, diagnose the failure and retry the same commit where safe, skipping verified uploads. Never move a tag after an upload. A source fix requires a new reviewed release source and appropriate versions. Keep the record in draft and hold docs deployment until verification succeeds.

## 5. Deploy documentation — agent, after approval

**User:** approve documentation deployment.

**Agent:** manually dispatch Pages after successful crate publication, monitor it to completion, verify the live site, documented versions, galleries and Wasm bundles, and add the deployment link to the release record. Report the release complete.

## Agent responsibility for automation gaps

The agent performs any steps the workflows do not yet automate. Current workflows create the tag/record after uploads and can replace notes with a version table: create the tag and draft beforehand, preserve the reviewed notes, and restore them before publishing the record.

Always use the last **completed** release as the preparation baseline, excluding unfinished release tags. Ask the user only for a missing decision, required approval or a blocker the agent cannot resolve.
