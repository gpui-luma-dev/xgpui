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
- Review automatic version proposals against the versioning policy below. Include a reason for every bump in the PR.
- Run relevant checks and a no-upload crates.io plan; resolve failures and version conflicts.
- Present the PR, proposed versions and validation results to the user.

**User:** review the proposal and approve or revise the version numbers. No commands or manual preparation are required.

## 2. Merge — agent, after approval

**User:** explicitly approve merging the release PR.

**Agent:** merge the approved PR, remove its branch, record the exact merged commit SHA and confirm the final registry plan applies to that source. Preserve unrelated local edits.

## 3. Create the tag and release record — agent

**Before any crates.io upload:**

- Tag the approved commit as `release-YYYY-MM-DD-COMMIT`, using its UTC commit date and first 12 hex characters.
- Support multiple releases per day: each distinct approved commit gets its own tag, for example `release-2026-10-31-ed4469267251` and `release-2026-10-31-a1b2c3d4e5f6`. Retries of the same release reuse its tag and record; do not create a new release identity for a retry.
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

## Versioning policy

Apply this policy during preparation, starting with the next release:

- **Patch:** compatible fixes, internal backend changes, packaged documentation and compatible dependency updates.
- **Minor:** new capabilities that preserve supported public APIs.
- **Major:** deliberate breaking changes to supported public APIs.
- **Before 1.0:** use a minor bump for breaking changes or new capabilities; use a patch for compatible fixes, internal changes and packaged docs. State whether a minor release contains breaking changes.
- **Dependent crates:** bump only when packaged content or dependency requirements change. Do not automatically propagate a major bump; review whether changed dependency types affect the crate's public API.
- **Compatibility review:** inspect removed or renamed APIs, signatures, exhaustive enum variants, features and minimum supported Rust version. Explain their impact and the proposed version in the PR.
- **Exceptions:** obtain explicit user approval and record the compatibility impact in the release notes.

Require a version rationale for every changed crate. Automated Rust API checks should inform this review; they must not assign versions blindly. Until those checks are implemented, the agent performs the review.

## Documentation-only publishing

Website fixes and improvements may be published independently, without a crate release, release tag or crate version bump. The agent validates the docs, obtains deployment approval and manually runs Pages.

Keep the website accurate for currently published crates. Do not announce unreleased APIs as available. Record which source commit was deployed; a docs-only deployment is not a new crate-release baseline.

Documentation inside a published crate archive is different: shipping its updated contents requires a new crate version.

## Agent responsibility for automation gaps

The agent performs any steps the workflows do not yet automate. Current workflows create the tag/record after uploads and can replace notes with a version table: create the tag and draft beforehand, preserve the reviewed notes, and restore them before publishing the record.

Mark abandoned partial releases **incomplete** or **superseded**, preserving their source tags and publication results. Never use them as completed baselines.

Always use the most recent **completed** release in the approved source history as the preparation baseline, including an earlier release from the same day. Exclude unfinished release tags. Ask the user only for a missing decision, required approval or a blocker the agent cannot resolve.
