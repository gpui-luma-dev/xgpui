# Repository instructions

## Git workflow (Epic Branch Pattern)

- Never push directly to `main` without explicit user approval.
- When working on an assigned issue (`<ISSUE_ID>`), isolate all related work under `issue-<ISSUE_ID>/epic`.
- Check if `origin/issue-<ISSUE_ID>/epic` exists. If not, create it from `origin/main` and push it.
- Branch subtasks from the epic branch using: `issue-<ISSUE_ID>/<short-task-slug>`.
- Always set the PR base target to `issue-<ISSUE_ID>/epic` (never target `main` directly).

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

## Remote machine tests

- Manual tests on the Windows and Fedora machines go through
  `scripts/remote-test.sh`. The tester runs `xgpui-test`, picks the PR or branch
  and the runner switches, pulls and runs that branch's `.remote-test.sh`.
- Never hand the tester commands, branch names or checkout steps. Put every step
  in the branch's `.remote-test.sh` at the repository root, using the helpers
  documented at the top of `scripts/remote-test.sh` (`note`, `run_example`,
  `step "action" "expected result"`).
- Commit and push the working branch with its `.remote-test.sh` as soon as it is
  ready for a run, then give the tester the branch, PR number and short hash.
- `.remote-test.sh` is temporary: delete it on the epic branch before the epic
  merges to `main`. CI (`remote-test-guard.yml`) fails if it reaches `main`.
