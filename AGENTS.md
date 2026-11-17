# Repository instructions

## Git workflow (Epic Branch Pattern)

- Never push directly to `main` without explicit user approval.
- When working on an assigned issue (`<ISSUE_ID>`), isolate all related work under `issue-<ISSUE_ID>/epic`.
- Check if `origin/issue-<ISSUE_ID>/epic` exists. If not, create it from `origin/main` and push it.
- Branch subtasks from the epic branch using: `issue-<ISSUE_ID>/<short-task-slug>`.
- Always set the PR base target to `issue-<ISSUE_ID>/epic`. Never target `main`
  directly unless the user explicitly says so for that change.

## Bug fixes

- A bug found while testing work in progress belongs to that work. Fix it on a
  branch off that work's epic (`issue-<ISSUE_ID>/<fix-slug>`), with the PR
  targeting the epic, and describe it in that PR and/or the original issue.
  Don't open a new issue for it.
- A fix branch is named `fix/<short-slug>` when it isn't a subtask of an
  epic. Ask the user which epic (or other branch) it targets; it goes straight to
  `main` only when the user says so.
- When a bug can be reproduced, add or extend a regression check that fails
  before the fix (a unit test, or a real-window probe in `apps/xgpui-ext-lab/tests`
  registered in `labs.toml`).

## Git approval

- Do not merge pull requests without the user's explicit approval.
- Do not push directly to `main` without the user's explicit approval.
- Requests to publish, deploy, or finish work do not by themselves authorize
  merging a pull request or pushing directly to `main`. Ask for explicit
  approval for the specific action before performing it.

## Core GPUI public API review

- Track every public API addition or change in non-extension GPUI crates.
- For each, document its purpose, source/serialization compatibility impact,
  migration requirements, and whether it could live in an extension instead.
- Complete authorized implementation and verification, then present the concrete
  API changes individually for user review before finalizing or releasing them.
- Implementation approval does not imply approval of the final public API.

## Cross-platform testing

- The user tests on Windows and Linux only after changes reach `main`. Don't
  ask for or plan Windows/Linux testing of PRs or branches. Say what is
  unverified on those platforms and rely on CI until it merges.
- A Claude session running on Windows or Linux may reproduce and fix bugs on
  that machine; the rule above is about what to ask the user to test.
- Don't create or suggest `.remote-test.sh` or other remote-run scripts.

## Releases

- Never start or advance a release while any repo involved has uncommitted or
  untracked changes. Don't make trial edits in other repos (such as
  xgpui-luma) as part of a release.
